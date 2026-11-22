use crate::GifFps;
use crate::encode::{EncodeError, ManualSettings, encode_gif};
use crate::resize::{downscale, scaled_size};
use crate::writer::Rgb;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EncodeSettings {
    Manual(ManualSettings),
    MaxSize { max_bytes: usize, dither: f32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QualityLevel {
    pub scale_percent: u32,
    pub colors: usize,
    pub tolerance: u8,
}

const fn level(scale_percent: u32, colors: usize, tolerance: u8) -> QualityLevel {
    QualityLevel { scale_percent, colors, tolerance }
}

pub const LADDER: [QualityLevel; 10] = [
    level(100, 255, 0),
    level(100, 255, 4),
    level(100, 255, 8),
    level(100, 128, 8),
    level(75, 128, 8),
    level(75, 64, 12),
    level(50, 64, 12),
    level(50, 32, 16),
    level(33, 32, 16),
    level(25, 16, 24),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Encoded {
    pub gif: Vec<u8>,
    pub width: u16,
    pub height: u16,
    pub fitted: Option<Fitted>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fitted {
    pub level: QualityLevel,
    pub fits: bool,
}

impl EncodeSettings {
    pub fn check(self) -> Result<(), EncodeError> {
        match self {
            EncodeSettings::Manual(manual) => manual.check(),
            EncodeSettings::MaxSize { max_bytes: 0, .. } => Err(EncodeError::SizeLimit(0)),
            EncodeSettings::MaxSize { dither, .. } => at_level(LADDER[0], dither).check(),
        }
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
    match settings {
        EncodeSettings::Manual(manual) => {
            let gif = encode_gif(width, height, frames, fps, manual)?;
            Ok(Encoded { gif, width, height, fitted: None })
        }
        EncodeSettings::MaxSize { max_bytes, dither } => {
            let try_level = |index: usize| {
                let encoded = encode_at_level(width, height, frames, fps, LADDER[index], dither)?;
                let bytes = encoded.gif.len();
                Ok((encoded, bytes))
            };
            let (index, encoded, fits) = first_fitting(LADDER.len(), max_bytes, try_level)?;
            Ok(Encoded { fitted: Some(Fitted { level: LADDER[index], fits }), ..encoded })
        }
    }
}

fn at_level(level: QualityLevel, dither: f32) -> ManualSettings {
    ManualSettings { colors: level.colors, dither, tolerance: level.tolerance }
}

fn encode_at_level(
    width: u16,
    height: u16,
    frames: &[&[Rgb]],
    fps: GifFps,
    level: QualityLevel,
    dither: f32,
) -> Result<Encoded, EncodeError> {
    let from = (u32::from(width), u32::from(height));
    let to = scaled_size(from.0, from.1, level.scale_percent).expect("every level has a scale in range");
    let expected = usize::from(width) * usize::from(height);

    let smaller: Vec<Vec<Rgb>> = frames
        .iter()
        .enumerate()
        .map(|(frame, pixels)| {
            downscale(pixels, from, to).map_err(|_| EncodeError::FrameSize { frame, expected, actual: pixels.len() })
        })
        .collect::<Result<_, _>>()?;
    let smaller: Vec<&[Rgb]> = smaller.iter().map(Vec::as_slice).collect();

    let (width, height) = (to.0 as u16, to.1 as u16);
    let gif = encode_gif(width, height, &smaller, fps, at_level(level, dither))?;
    Ok(Encoded { gif, width, height, fitted: None })
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

    fn encode_film(settings: EncodeSettings) -> Result<Encoded, EncodeError> {
        let frames = noisy_film(64, 48, 6);
        let slices: Vec<&[Rgb]> = frames.iter().map(Vec::as_slice).collect();
        encode(64, 48, &slices, GifFps::Fps10, settings)
    }

    #[test]
    fn manual_settings_give_the_same_file_as_encode_gif() {
        let settings = ManualSettings { colors: 64, dither: 0.5, tolerance: 2 };
        let frames = noisy_film(64, 48, 6);
        let slices: Vec<&[Rgb]> = frames.iter().map(Vec::as_slice).collect();
        let direct = encode_gif(64, 48, &slices, GifFps::Fps10, settings).unwrap();

        let encoded = encode_film(EncodeSettings::Manual(settings)).unwrap();

        assert_eq!(encoded, Encoded { gif: direct, width: 64, height: 48, fitted: None });
    }

    #[test]
    fn a_generous_limit_keeps_full_quality() {
        let encoded = encode_film(EncodeSettings::MaxSize { max_bytes: 10_000_000, dither: 1.0 }).unwrap();
        assert_eq!(encoded.fitted, Some(Fitted { level: LADDER[0], fits: true }));
        assert_eq!((encoded.width, encoded.height), (64, 48));
    }

    #[test]
    fn a_tight_limit_gives_a_file_within_it_at_a_lower_quality() {
        let full = encode_film(EncodeSettings::MaxSize { max_bytes: 10_000_000, dither: 1.0 }).unwrap();
        let limit = full.gif.len() / 3;

        let encoded = encode_film(EncodeSettings::MaxSize { max_bytes: limit, dither: 1.0 }).unwrap();

        let fitted = encoded.fitted.unwrap();
        assert!(fitted.fits && encoded.gif.len() <= limit, "{} bytes for a limit of {limit}", encoded.gif.len());
        assert_ne!(fitted.level, LADDER[0]);
        assert!(encoded.gif.starts_with(b"GIF89a"));
    }

    #[test]
    fn the_size_written_in_the_file_matches_the_reported_size() {
        let encoded = encode_film(EncodeSettings::MaxSize { max_bytes: 1, dither: 1.0 }).unwrap();

        assert_eq!(encoded.fitted, Some(Fitted { level: LADDER[9], fits: false }));
        assert_eq!((encoded.width, encoded.height), (16, 12));
        assert_eq!(encoded.gif[6..10], [16, 0, 12, 0]);
    }

    #[test]
    fn settings_out_of_range_are_rejected() {
        let no_limit = EncodeSettings::MaxSize { max_bytes: 0, dither: 1.0 };
        let bad_dither = EncodeSettings::MaxSize { max_bytes: 1000, dither: 2.0 };
        let bad_manual = EncodeSettings::Manual(ManualSettings { colors: 1, dither: 0.0, tolerance: 0 });

        assert_eq!(no_limit.check(), Err(EncodeError::SizeLimit(0)));
        assert_eq!(bad_dither.check(), Err(EncodeError::Dither(2.0)));
        assert_eq!(bad_manual.check(), Err(EncodeError::Colors(1)));
        assert_eq!(encode_film(no_limit), Err(EncodeError::SizeLimit(0)));
    }
}
