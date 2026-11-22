use crate::GifFps;
use crate::encode::{EncodeError, ManualSettings, encode_gif};
use crate::writer::Rgb;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QualityLevel {
    pub colors: usize,
    pub tolerance: u8,
}

const fn level(colors: usize, tolerance: u8) -> QualityLevel {
    QualityLevel { colors, tolerance }
}

pub const LEVELS: [QualityLevel; 6] =
    [level(255, 0), level(255, 4), level(255, 8), level(128, 8), level(64, 12), level(32, 16)];

pub const BEST_LEVEL: usize = 1;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Quality {
    Level(usize),
    WithinBytes(usize),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EncodeSettings {
    pub quality: Quality,
    pub dither: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Encoded {
    pub gif: Vec<u8>,
    pub level: usize,
    pub within_limit: Option<bool>,
}

impl EncodeSettings {
    pub fn check(self) -> Result<(), EncodeError> {
        match self.quality {
            Quality::Level(level) if !(BEST_LEVEL..=LEVELS.len()).contains(&level) => Err(EncodeError::Level(level)),
            Quality::WithinBytes(0) => Err(EncodeError::SizeLimit(0)),
            _ => self.at_level(BEST_LEVEL).check(),
        }
    }

    fn at_level(self, level: usize) -> ManualSettings {
        let QualityLevel { colors, tolerance } = LEVELS[level - BEST_LEVEL];
        ManualSettings { colors, dither: self.dither, tolerance }
    }
}

pub fn encode(
    width: u16,
    height: u16,
    frames: &[&[Rgb]],
    fps: GifFps,
    settings: EncodeSettings,
) -> Result<Encoded, EncodeError> {
    settings.check()?;
    let encode_at = |level: usize| encode_gif(width, height, frames, fps, settings.at_level(level));

    match settings.quality {
        Quality::Level(level) => Ok(Encoded { gif: encode_at(level)?, level, within_limit: None }),
        Quality::WithinBytes(max_bytes) => {
            let try_level = |index: usize| {
                let gif = encode_at(index + BEST_LEVEL)?;
                let bytes = gif.len();
                Ok((gif, bytes))
            };
            let (index, gif, fits) = first_fitting(LEVELS.len(), max_bytes, try_level)?;
            Ok(Encoded { gif, level: index + BEST_LEVEL, within_limit: Some(fits) })
        }
    }
}

fn first_fitting<T>(
    levels: usize,
    max_bytes: usize,
    mut try_level: impl FnMut(usize) -> Result<(T, usize), EncodeError>,
) -> Result<(usize, T, bool), EncodeError> {
    let (best, best_bytes) = try_level(0)?;
    if best_bytes <= max_bytes || levels == 1 {
        return Ok((0, best, best_bytes <= max_bytes));
    }

    let last = levels - 1;
    let (smallest, smallest_bytes) = try_level(last)?;
    if smallest_bytes > max_bytes {
        return Ok((last, smallest, false));
    }

    let (mut too_big, mut fits, mut fitting) = (0, last, smallest);
    while fits - too_big > 1 {
        let middle = (too_big + fits) / 2;
        let (output, bytes) = try_level(middle)?;
        if bytes <= max_bytes {
            (fits, fitting) = (middle, output);
        } else {
            too_big = middle;
        }
    }
    Ok((fits, fitting, true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn search(sizes: &[usize], max_bytes: usize) -> ((usize, bool), Vec<usize>) {
        let tried = RefCell::new(Vec::new());
        let try_level = |level: usize| {
            tried.borrow_mut().push(level);
            Ok((level, sizes[level]))
        };
        let (level, output, fits) = first_fitting(sizes.len(), max_bytes, try_level).unwrap();
        assert_eq!(output, level, "the output belongs to the level that was picked");
        ((level, fits), tried.into_inner())
    }

    #[test]
    fn the_best_quality_is_used_as_is_when_it_already_fits() {
        assert_eq!(search(&[900, 700, 500, 300], 1000), ((0, true), vec![0]));
    }

    #[test]
    fn the_first_level_that_fits_is_picked() {
        let sizes = [900, 800, 700, 600, 500, 400, 300, 200];
        for (limit, expected) in [(850, 1), (750, 2), (650, 3), (450, 5), (250, 7)] {
            assert_eq!(search(&sizes, limit).0, (expected, true), "limit {limit}");
        }
    }

    #[test]
    fn a_size_exactly_at_the_limit_fits() {
        assert_eq!(search(&[900, 700, 500], 700).0, (1, true));
    }

    #[test]
    fn when_nothing_fits_the_smallest_is_returned_and_marked() {
        assert_eq!(search(&[900, 700, 500], 100).0, (2, false));
    }

    #[test]
    fn ten_levels_never_take_more_than_six_encodes() {
        let sizes = [1000, 900, 800, 700, 600, 500, 400, 300, 200, 100];
        for limit in (50..=1050).step_by(50) {
            let (_, tried) = search(&sizes, limit);
            assert!(tried.len() <= 6, "limit {limit} tried {tried:?}");
        }
    }

    #[test]
    fn a_failed_encode_stops_the_search() {
        let failing = |_| Err::<((), usize), _>(EncodeError::NoFrames);
        assert_eq!(first_fitting(4, 100, failing).err(), Some(EncodeError::NoFrames));
    }

    fn noisy_film(width: usize, height: usize, frames: usize) -> Vec<Vec<Rgb>> {
        (0..frames)
            .map(|number| {
                (0..width * height)
                    .map(|index| {
                        let (x, y) = (index % width, index / width);
                        let noise = ((x * 31 + y * 17 + number * 101) % 23) as u8;
                        [(x * 3) as u8 + noise, (y * 4) as u8 + noise, ((x + y + number * 5) % 256) as u8]
                    })
                    .collect()
            })
            .collect()
    }

    fn at(quality: Quality) -> EncodeSettings {
        EncodeSettings { quality, dither: 1.0 }
    }

    fn encode_film(settings: EncodeSettings) -> Result<Encoded, EncodeError> {
        let frames = noisy_film(64, 48, 6);
        let slices: Vec<&[Rgb]> = frames.iter().map(Vec::as_slice).collect();
        encode(64, 48, &slices, GifFps::Fps10, settings)
    }

    #[test]
    fn a_chosen_level_gives_the_file_its_colors_and_tolerance_would() {
        let frames = noisy_film(64, 48, 6);
        let slices: Vec<&[Rgb]> = frames.iter().map(Vec::as_slice).collect();
        let by_hand = ManualSettings { colors: 128, dither: 1.0, tolerance: 8 };
        let direct = encode_gif(64, 48, &slices, GifFps::Fps10, by_hand).unwrap();

        let encoded = encode_film(at(Quality::Level(4))).unwrap();

        assert_eq!(encoded, Encoded { gif: direct, level: 4, within_limit: None });
    }

    #[test]
    fn a_lower_level_gives_a_smaller_file() {
        let sizes: Vec<usize> =
            (1..=LEVELS.len()).map(|level| encode_film(at(Quality::Level(level))).unwrap().gif.len()).collect();
        assert!(sizes.windows(2).all(|pair| pair[0] >= pair[1]), "{sizes:?}");
        assert!(sizes[0] > sizes[5], "{sizes:?}");
    }

    #[test]
    fn a_generous_limit_keeps_the_best_level() {
        let encoded = encode_film(at(Quality::WithinBytes(10_000_000))).unwrap();
        assert_eq!((encoded.level, encoded.within_limit), (BEST_LEVEL, Some(true)));
    }

    #[test]
    fn a_tight_limit_gives_a_file_within_it_at_a_lower_level() {
        let best = encode_film(at(Quality::Level(BEST_LEVEL))).unwrap().gif.len();
        let worst = encode_film(at(Quality::Level(LEVELS.len()))).unwrap().gif.len();
        let limit = (best + worst) / 2;

        let encoded = encode_film(at(Quality::WithinBytes(limit))).unwrap();

        assert_eq!(encoded.within_limit, Some(true));
        assert!(encoded.gif.len() <= limit, "{} bytes for a limit of {limit}", encoded.gif.len());
        assert!(encoded.level > BEST_LEVEL);
        assert_eq!(encoded.gif, encode_film(at(Quality::Level(encoded.level))).unwrap().gif);
    }

    #[test]
    fn the_picture_keeps_its_size_however_tight_the_limit() {
        let encoded = encode_film(at(Quality::WithinBytes(1))).unwrap();

        assert_eq!((encoded.level, encoded.within_limit), (LEVELS.len(), Some(false)));
        assert_eq!(encoded.gif[6..10], [64, 0, 48, 0]);
    }

    #[test]
    fn settings_out_of_range_are_rejected() {
        assert_eq!(at(Quality::Level(0)).check(), Err(EncodeError::Level(0)));
        assert_eq!(at(Quality::Level(7)).check(), Err(EncodeError::Level(7)));
        assert_eq!(at(Quality::WithinBytes(0)).check(), Err(EncodeError::SizeLimit(0)));
        assert_eq!(EncodeSettings { quality: Quality::Level(1), dither: 2.0 }.check(), Err(EncodeError::Dither(2.0)));
        assert_eq!(encode_film(at(Quality::Level(7))), Err(EncodeError::Level(7)));
        assert_eq!(at(Quality::Level(6)).check(), Ok(()));
    }
}
