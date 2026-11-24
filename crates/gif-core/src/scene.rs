use crate::writer::Rgb;

const SAMPLE_STEP: usize = 4;
const COLOR_BIN_BITS: u32 = 3;
const COLOR_BINS: usize = 1 << (3 * COLOR_BIN_BITS);
const MIN_BRIGHTNESS_JUMP: u64 = 20;
const MIN_COLOR_SHIFT_PERCENT: u64 = 18;
const MIN_SCENE_FRAMES: usize = 8;
const MAX_SCENES: usize = 32;

#[derive(Debug, Clone)]
pub(crate) struct SceneCuts {
    width: usize,
    previous: Option<Sample>,
    previous_brightness_change: u64,
    frames_in_scene: usize,
    scenes: usize,
}

#[derive(Debug, Clone)]
struct Sample {
    brightness: Vec<u8>,
    color_bins: Vec<u32>,
}

#[derive(Debug, Clone, Copy)]
struct Change {
    brightness: u64,
    colors_moved: u64,
    samples: u64,
}

impl SceneCuts {
    pub(crate) fn new(width: usize) -> Self {
        Self { width, previous: None, previous_brightness_change: 0, frames_in_scene: 0, scenes: 1 }
    }

    pub(crate) fn starts_new_scene(&mut self, frame: &[Rgb]) -> bool {
        let sample = Sample::of(frame, self.width);
        let change = self.previous.as_ref().map(|previous| previous.change_to(&sample));
        self.previous = Some(sample);

        let sudden = change.is_some_and(|change| change.is_sudden(self.previous_brightness_change));
        self.previous_brightness_change = change.map_or(0, |change| change.brightness);
        let new_scene = sudden && self.frames_in_scene >= MIN_SCENE_FRAMES && self.scenes < MAX_SCENES;
        if new_scene {
            self.frames_in_scene = 0;
            self.scenes += 1;
        }
        self.frames_in_scene += 1;
        new_scene
    }
}

impl Sample {
    fn of(frame: &[Rgb], width: usize) -> Self {
        let mut brightness = Vec::new();
        let mut color_bins = vec![0; COLOR_BINS];
        for row in frame.chunks_exact(width).step_by(SAMPLE_STEP) {
            for &[red, green, blue] in row.iter().step_by(SAMPLE_STEP) {
                let [red, green, blue] = [red, green, blue].map(u32::from);
                brightness.push(((77 * red + 150 * green + 29 * blue) >> 8) as u8);
                let bin = |channel: u32| (channel >> (8 - COLOR_BIN_BITS)) as usize;
                color_bins[bin(red) << (2 * COLOR_BIN_BITS) | bin(green) << COLOR_BIN_BITS | bin(blue)] += 1;
            }
        }
        Self { brightness, color_bins }
    }

    fn change_to(&self, next: &Sample) -> Change {
        let brightness = self.brightness.iter().zip(&next.brightness).map(|(a, b)| u64::from(a.abs_diff(*b))).sum();
        let bins_apart: u64 = self.color_bins.iter().zip(&next.color_bins).map(|(a, b)| u64::from(a.abs_diff(*b))).sum();
        Change { brightness, colors_moved: bins_apart / 2, samples: next.brightness.len() as u64 }
    }
}

impl Change {
    fn is_sudden(self, previous_brightness: u64) -> bool {
        let jump = self.brightness.min(self.brightness.abs_diff(previous_brightness));
        jump >= MIN_BRIGHTNESS_JUMP * self.samples && self.colors_moved * 100 >= MIN_COLOR_SHIFT_PERCENT * self.samples
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDTH: usize = 32;
    const HEIGHT: usize = 24;

    fn shade(level: u8) -> Vec<Rgb> {
        (0..WIDTH * HEIGHT).map(|index| [level, level / 2, (index % WIDTH) as u8 * 4]).collect()
    }

    fn first_frames_of_new_scenes(frames: &[Vec<Rgb>]) -> Vec<usize> {
        let mut cuts = SceneCuts::new(WIDTH);
        (0..frames.len()).filter(|&index| cuts.starts_new_scene(&frames[index])).collect()
    }

    #[test]
    fn a_hard_cut_starts_a_new_scene_at_the_first_frame_after_it() {
        let frames: Vec<Vec<Rgb>> = (0..20).map(|index| shade(if index < 10 { 30 } else { 220 })).collect();
        assert_eq!(first_frames_of_new_scenes(&frames), [10]);
    }

    #[test]
    fn a_steady_fade_is_one_scene() {
        let frames: Vec<Vec<Rgb>> = (0..40).map(|index| shade(index * 6)).collect();
        assert_eq!(first_frames_of_new_scenes(&frames), []);
    }

    #[test]
    fn stripes_sliding_fast_keep_their_colors_and_stay_one_scene() {
        let stripes = |offset: usize| -> Vec<Rgb> {
            let white_at = |index: usize| (index % WIDTH + offset) % 8 < 4;
            (0..WIDTH * HEIGHT).map(|index| if white_at(index) { [250, 250, 250] } else { [0, 0, 0] }).collect()
        };
        let frames: Vec<Vec<Rgb>> = (0..20).map(|index| stripes(index * 4)).collect();
        assert_eq!(first_frames_of_new_scenes(&frames), []);
    }

    #[test]
    fn a_cut_too_soon_after_the_last_one_stays_in_the_same_scene() {
        let level = |index| if (10..15).contains(&index) { 220 } else { 30 };
        let frames: Vec<Vec<Rgb>> = (0..30).map(|index| shade(level(index))).collect();
        assert_eq!(first_frames_of_new_scenes(&frames), [10]);
    }

    #[test]
    fn a_clip_that_keeps_cutting_is_split_no_finer_than_the_shortest_scene() {
        let frames: Vec<Vec<Rgb>> = (0..80).map(|index| shade(if index % 3 == 2 { 230 } else { 20 })).collect();
        let starts = first_frames_of_new_scenes(&frames);
        assert!(starts.len() > 1, "{starts:?}");
        assert!(starts[0] >= MIN_SCENE_FRAMES, "{starts:?}");
        assert!(starts.windows(2).all(|pair| pair[1] - pair[0] >= MIN_SCENE_FRAMES), "{starts:?}");
    }

    #[test]
    fn a_long_clip_of_cuts_stops_splitting_at_the_scene_limit() {
        let frames: Vec<Vec<Rgb>> = (0..MIN_SCENE_FRAMES * MAX_SCENES * 2)
            .map(|index| shade(if (index / MIN_SCENE_FRAMES).is_multiple_of(2) { 20 } else { 230 }))
            .collect();
        assert_eq!(first_frames_of_new_scenes(&frames).len(), MAX_SCENES - 1);
    }
}
