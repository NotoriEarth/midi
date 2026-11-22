use crate::GifFps;
use crate::encode::{EncodeError, FrameError, FrameSource, ManualSettings, Progress, encode_frames};
use crate::resize::{downscale, scale_by_percent};
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
pub const FULL_SIZE: u32 = 100;
pub const SHRINK_STEPS: [u32; 8] = [90, 80, 70, 60, 50, 40, 33, 25];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Adjust {
    Level,
    Size { level: usize },
    LevelThenSize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quality {
    Level(usize),
    WithinBytes { max_bytes: usize, adjust: Adjust },
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
    pub width: u16,
    pub height: u16,
    pub within_limit: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    pub attempt: usize,
    pub progress: Progress,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Candidate {
    level: usize,
    scale_percent: u32,
}

fn candidates(adjust: Adjust) -> Vec<Candidate> {
    let lowest = LEVELS.len();
    let every_level = (BEST_LEVEL..=lowest).map(|level| Candidate { level, scale_percent: FULL_SIZE });
    let shrinking = |level| SHRINK_STEPS.map(|scale_percent| Candidate { level, scale_percent });

    match adjust {
        Adjust::Level => every_level.collect(),
        Adjust::Size { level } => {
            std::iter::once(Candidate { level, scale_percent: FULL_SIZE }).chain(shrinking(level)).collect()
        }
        Adjust::LevelThenSize => every_level.chain(shrinking(lowest)).collect(),
    }
}

impl EncodeSettings {
    pub fn check(self) -> Result<(), EncodeError> {
        let in_range = |level: usize| (BEST_LEVEL..=LEVELS.len()).contains(&level);
        match self.quality {
            Quality::Level(level) | Quality::WithinBytes { adjust: Adjust::Size { level }, .. } if !in_range(level) => {
                Err(EncodeError::Level(level))
            }
            Quality::WithinBytes { max_bytes: 0, .. } => Err(EncodeError::SizeLimit(0)),
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
    frames: &mut dyn FrameSource,
    fps: GifFps,
    settings: EncodeSettings,
    on_step: &mut dyn FnMut(Step),
) -> Result<Encoded, EncodeError> {
    settings.check()?;
    let mut attempt = 0;
    let mut encode_as = |candidate: Candidate| {
        attempt += 1;
        let mut on_progress = |progress| on_step(Step { attempt, progress });
        encode_candidate(width, height, frames, fps, settings, candidate, &mut on_progress)
    };

    match settings.quality {
        Quality::Level(level) => encode_as(Candidate { level, scale_percent: FULL_SIZE }),
        Quality::WithinBytes { max_bytes, adjust } => {
            let choices = candidates(adjust);
            let mut try_choice = |index: usize| {
                let encoded = encode_as(choices[index])?;
                let bytes = encoded.gif.len();
                Ok((encoded, bytes))
            };
            let (_, encoded, fits) = first_fitting(choices.len(), max_bytes, &mut try_choice)?;
            Ok(Encoded { within_limit: Some(fits), ..encoded })
        }
    }
}

fn encode_candidate(
    width: u16,
    height: u16,
    frames: &mut dyn FrameSource,
    fps: GifFps,
    settings: EncodeSettings,
    candidate: Candidate,
    on_progress: &mut dyn FnMut(Progress),
) -> Result<Encoded, EncodeError> {
    let manual = settings.at_level(candidate.level);
    let encoded = |gif, (width, height): (u32, u32)| Encoded {
        gif,
        level: candidate.level,
        width: width as u16,
        height: height as u16,
        within_limit: None,
    };

    let from = (u32::from(width), u32::from(height));
    if candidate.scale_percent == FULL_SIZE {
        return Ok(encoded(encode_frames(width, height, frames, fps, manual, on_progress)?, from));
    }

    let to = scale_by_percent(from, candidate.scale_percent);
    let mut smaller = Shrunk { frames, from, to, full_size: Vec::new() };
    Ok(encoded(encode_frames(to.0 as u16, to.1 as u16, &mut smaller, fps, manual, on_progress)?, to))
}

struct Shrunk<'a> {
    frames: &'a mut dyn FrameSource,
    from: (u32, u32),
    to: (u32, u32),
    full_size: Vec<Rgb>,
}

impl FrameSource for Shrunk<'_> {
    fn frame_count(&self) -> usize {
        self.frames.frame_count()
    }

    fn read_frame(&mut self, index: usize, pixels: &mut Vec<Rgb>) -> Result<(), FrameError> {
        self.frames.read_frame(index, &mut self.full_size)?;
        let expected = (self.from.0 * self.from.1) as usize;
        *pixels = downscale(&self.full_size, self.from, self.to)
            .map_err(|_| FrameError::PixelCount { expected, actual: self.full_size.len() })?;
        Ok(())
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
    use crate::encode::encode_gif;
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
    fn fourteen_choices_never_take_more_than_six_encodes() {
        let sizes: Vec<usize> = (1..=14).rev().map(|step| step * 100).collect();
        for limit in (50..=1450).step_by(50) {
            let (_, tried) = search(&sizes, limit);
            assert!(tried.len() <= 6, "limit {limit} tried {tried:?}");
        }
    }

    #[test]
    fn a_failed_encode_stops_the_search() {
        let failing = |_| Err::<((), usize), _>(EncodeError::NoFrames);
        assert_eq!(first_fitting(4, 100, failing).err(), Some(EncodeError::NoFrames));
    }

    fn choices(adjust: Adjust) -> Vec<(usize, u32)> {
        candidates(adjust).iter().map(|candidate| (candidate.level, candidate.scale_percent)).collect()
    }

    #[test]
    fn adjusting_the_level_keeps_full_size_and_walks_down_the_levels() {
        assert_eq!(choices(Adjust::Level), [(1, 100), (2, 100), (3, 100), (4, 100), (5, 100), (6, 100)]);
    }

    #[test]
    fn adjusting_the_size_keeps_the_level_and_stops_at_a_quarter() {
        let expected = [(2, 100), (2, 90), (2, 80), (2, 70), (2, 60), (2, 50), (2, 40), (2, 33), (2, 25)];
        assert_eq!(choices(Adjust::Size { level: 2 }), expected);
    }

    #[test]
    fn adjusting_both_lowers_the_level_first_and_shrinks_only_after_the_lowest_level() {
        let both = choices(Adjust::LevelThenSize);
        assert_eq!(both[..6], [(1, 100), (2, 100), (3, 100), (4, 100), (5, 100), (6, 100)]);
        assert_eq!(both[6..], [(6, 90), (6, 80), (6, 70), (6, 60), (6, 50), (6, 40), (6, 33), (6, 25)]);
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

    fn within(max_bytes: usize, adjust: Adjust) -> EncodeSettings {
        at(Quality::WithinBytes { max_bytes, adjust })
    }

    fn encode_film(settings: EncodeSettings) -> Result<Encoded, EncodeError> {
        let frames = noisy_film(64, 48, 6);
        let slices: Vec<&[Rgb]> = frames.iter().map(Vec::as_slice).collect();
        encode(64, 48, &mut &slices[..], GifFps::Fps10, settings, &mut |_| {})
    }

    fn steps_of(settings: EncodeSettings) -> Vec<Step> {
        let frames = noisy_film(64, 48, 6);
        let slices: Vec<&[Rgb]> = frames.iter().map(Vec::as_slice).collect();
        let mut steps = Vec::new();
        encode(64, 48, &mut &slices[..], GifFps::Fps10, settings, &mut |step| steps.push(step)).unwrap();
        steps
    }

    fn finished_attempts(steps: &[Step]) -> Vec<usize> {
        let finished = steps.iter().filter(|step| step.progress.done == step.progress.total);
        finished.map(|step| step.attempt).collect()
    }

    #[test]
    fn a_chosen_level_is_one_attempt_that_runs_to_the_end() {
        let steps = steps_of(at(Quality::Level(3)));

        assert_eq!(steps.first().map(|step| step.attempt), Some(1));
        assert_eq!(finished_attempts(&steps), [1]);
        assert_eq!(steps.last().unwrap().progress, Progress { done: 12, total: 12 });
    }

    #[test]
    fn each_encode_toward_a_limit_is_numbered_and_runs_to_the_end() {
        let limit = (bytes_at(1) + bytes_at(6)) / 2;

        let steps = steps_of(within(limit, Adjust::Level));

        let attempts = finished_attempts(&steps);
        assert!(attempts.len() >= 3, "the first, the last and at least one between: {attempts:?}");
        assert_eq!(attempts, (1..=attempts.len()).collect::<Vec<_>>());
        assert!(steps.windows(2).all(|pair| pair[0].attempt <= pair[1].attempt));
    }

    fn bytes_at(level: usize) -> usize {
        encode_film(at(Quality::Level(level))).unwrap().gif.len()
    }

    fn summary(encoded: &Encoded) -> (usize, (u16, u16), Option<bool>) {
        (encoded.level, (encoded.width, encoded.height), encoded.within_limit)
    }

    #[test]
    fn a_chosen_level_gives_the_file_its_colors_and_tolerance_would() {
        let frames = noisy_film(64, 48, 6);
        let slices: Vec<&[Rgb]> = frames.iter().map(Vec::as_slice).collect();
        let by_hand = ManualSettings { colors: 128, dither: 1.0, tolerance: 8 };
        let direct = encode_gif(64, 48, &slices, GifFps::Fps10, by_hand).unwrap();

        let encoded = encode_film(at(Quality::Level(4))).unwrap();

        assert_eq!(encoded, Encoded { gif: direct, level: 4, width: 64, height: 48, within_limit: None });
    }

    #[test]
    fn a_lower_level_gives_a_smaller_file() {
        let sizes: Vec<usize> = (1..=LEVELS.len()).map(bytes_at).collect();
        assert!(sizes.windows(2).all(|pair| pair[0] >= pair[1]), "{sizes:?}");
        assert!(sizes[0] > sizes[5], "{sizes:?}");
    }

    #[test]
    fn a_generous_limit_keeps_the_best_level_and_the_full_size() {
        for adjust in [Adjust::Level, Adjust::LevelThenSize] {
            let encoded = encode_film(within(10_000_000, adjust)).unwrap();
            assert_eq!(summary(&encoded), (BEST_LEVEL, (64, 48), Some(true)), "{adjust:?}");
        }
    }

    #[test]
    fn with_the_size_fixed_a_tight_limit_lowers_the_level_only() {
        let limit = (bytes_at(1) + bytes_at(6)) / 2;

        let encoded = encode_film(within(limit, Adjust::Level)).unwrap();

        assert!(encoded.gif.len() <= limit, "{} bytes for a limit of {limit}", encoded.gif.len());
        assert!(encoded.level > BEST_LEVEL);
        assert_eq!(((encoded.width, encoded.height), encoded.within_limit), ((64, 48), Some(true)));
    }

    #[test]
    fn with_the_size_fixed_an_impossible_limit_gives_the_lowest_level_at_full_size() {
        let encoded = encode_film(within(1, Adjust::Level)).unwrap();

        assert_eq!(summary(&encoded), (6, (64, 48), Some(false)));
        assert_eq!(encoded.gif[6..10], [64, 0, 48, 0]);
    }

    #[test]
    fn with_the_level_fixed_a_tight_limit_shrinks_the_picture_only() {
        let limit = bytes_at(2) / 2;

        let encoded = encode_film(within(limit, Adjust::Size { level: 2 })).unwrap();

        assert!(encoded.gif.len() <= limit, "{} bytes for a limit of {limit}", encoded.gif.len());
        assert_eq!((encoded.level, encoded.within_limit), (2, Some(true)));
        assert!(encoded.width < 64 && encoded.height < 48, "{}x{}", encoded.width, encoded.height);
        assert_eq!(encoded.gif[6..10], [encoded.width as u8, 0, encoded.height as u8, 0]);
    }

    #[test]
    fn with_the_level_fixed_an_impossible_limit_stops_at_a_quarter_of_the_size() {
        let encoded = encode_film(within(1, Adjust::Size { level: 2 })).unwrap();
        assert_eq!(summary(&encoded), (2, (16, 12), Some(false)));
    }

    #[test]
    fn with_nothing_fixed_the_level_drops_before_the_size_does() {
        let reachable_by_level = (bytes_at(1) + bytes_at(6)) / 2;
        let by_level = encode_film(within(reachable_by_level, Adjust::LevelThenSize)).unwrap();
        assert_eq!(((by_level.width, by_level.height), by_level.within_limit), ((64, 48), Some(true)));

        let below_every_level = bytes_at(6) / 2;
        let by_size = encode_film(within(below_every_level, Adjust::LevelThenSize)).unwrap();
        assert!(by_size.gif.len() <= below_every_level);
        assert_eq!((by_size.level, by_size.within_limit), (6, Some(true)));
        assert!(by_size.width < 64, "{}x{}", by_size.width, by_size.height);
    }

    #[test]
    fn with_nothing_fixed_an_impossible_limit_gives_the_lowest_level_at_a_quarter() {
        let encoded = encode_film(within(1, Adjust::LevelThenSize)).unwrap();
        assert_eq!(summary(&encoded), (6, (16, 12), Some(false)));
    }

    #[test]
    fn settings_out_of_range_are_rejected() {
        assert_eq!(at(Quality::Level(0)).check(), Err(EncodeError::Level(0)));
        assert_eq!(at(Quality::Level(7)).check(), Err(EncodeError::Level(7)));
        assert_eq!(within(1000, Adjust::Size { level: 7 }).check(), Err(EncodeError::Level(7)));
        assert_eq!(within(0, Adjust::Level).check(), Err(EncodeError::SizeLimit(0)));
        assert_eq!(EncodeSettings { quality: Quality::Level(1), dither: 2.0 }.check(), Err(EncodeError::Dither(2.0)));
        assert_eq!(encode_film(at(Quality::Level(7))), Err(EncodeError::Level(7)));
        assert_eq!(at(Quality::Level(6)).check(), Ok(()));
    }
}
