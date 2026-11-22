use crate::GifFps;
use crate::dither::ordered_dither;
use crate::palette::{ColorCounts, Palette};
use crate::stabilize::Stabilizer;
use crate::writer::{GifWriter, Rgb};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ManualSettings {
    pub colors: usize,
    pub dither: f32,
    pub tolerance: u8,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EncodeError {
    EmptyImage,
    NoFrames,
    FrameSize { frame: usize, expected: usize, actual: usize },
    Colors(usize),
    Dither(f32),
    Tolerance(u8),
    SizeLimit(usize),
    Level(usize),
    FrameUnreadable { frame: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameError {
    Unreadable,
    PixelCount { expected: usize, actual: usize },
}

pub trait FrameSource {
    fn frame_count(&self) -> usize;
    fn slot(&self, index: usize) -> usize;
    fn read_frame(&mut self, index: usize, pixels: &mut Vec<Rgb>) -> Result<(), FrameError>;
}

impl FrameSource for &[&[Rgb]] {
    fn frame_count(&self) -> usize {
        self.len()
    }

    fn slot(&self, index: usize) -> usize {
        index
    }

    fn read_frame(&mut self, index: usize, pixels: &mut Vec<Rgb>) -> Result<(), FrameError> {
        pixels.clear();
        pixels.extend_from_slice(self[index]);
        Ok(())
    }
}

impl ManualSettings {
    pub const MIN_COLORS: usize = 2;
    pub const MAX_COLORS: usize = Palette::MAX_COLORS;
    pub const MAX_TOLERANCE: u8 = 32;

    pub fn check(self) -> Result<(), EncodeError> {
        if !(Self::MIN_COLORS..=Self::MAX_COLORS).contains(&self.colors) {
            return Err(EncodeError::Colors(self.colors));
        }
        if !(0.0..=1.0).contains(&self.dither) {
            return Err(EncodeError::Dither(self.dither));
        }
        if self.tolerance > Self::MAX_TOLERANCE {
            return Err(EncodeError::Tolerance(self.tolerance));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub done: usize,
    pub total: usize,
}

pub fn encode_gif(
    width: u16,
    height: u16,
    mut frames: &[&[Rgb]],
    fps: GifFps,
    settings: ManualSettings,
) -> Result<Vec<u8>, EncodeError> {
    encode_frames(width, height, &mut frames, fps, settings, &mut |_| {})
}

pub fn encode_frames(
    width: u16,
    height: u16,
    frames: &mut dyn FrameSource,
    fps: GifFps,
    settings: ManualSettings,
    on_progress: &mut dyn FnMut(Progress),
) -> Result<Vec<u8>, EncodeError> {
    settings.check()?;
    let pixel_count = usize::from(width) * usize::from(height);
    if pixel_count == 0 {
        return Err(EncodeError::EmptyImage);
    }
    let frame_count = frames.frame_count();
    if frame_count == 0 {
        return Err(EncodeError::NoFrames);
    }

    let mut progress = Progress { done: 0, total: frame_count * 2 };
    let mut one_more_step = || {
        progress.done += 1;
        on_progress(progress);
    };
    let mut frame = Vec::new();

    let mut counts = ColorCounts::new();
    let mut steady = Stabilizer::new(settings.tolerance);
    for index in 0..frame_count {
        read(frames, index, pixel_count, &mut frame)?;
        counts.add(steady.show(&frame));
        one_more_step();
    }
    let palette = Palette::from_counts(&counts, settings.colors)
        .expect("the color limit is in range and the frames have pixels");
    drop(counts);

    let mut steady = Stabilizer::new(settings.tolerance);
    let mut nearest = palette.lookup();
    let mut writer = GifWriter::new(width, height, palette.colors(), fps)
        .expect("the size is not empty and the palette has 1 to 255 colors");
    for index in 0..frame_count {
        read(frames, index, pixel_count, &mut frame)?;
        let indices = ordered_dither(&mut nearest, steady.show(&frame), usize::from(width), settings.dither)
            .expect("every frame is width x height pixels");
        let slot = frames.slot(index);
        let next_slot = if index + 1 < frame_count { frames.slot(index + 1) } else { slot + 1 };
        writer
            .add_frame(&indices, slot..next_slot.max(slot + 1))
            .expect("the indices come from the palette and the frame fits the size");
        one_more_step();
    }
    Ok(writer.finish().expect("at least one frame was written"))
}

fn read(
    frames: &mut dyn FrameSource,
    frame: usize,
    pixel_count: usize,
    pixels: &mut Vec<Rgb>,
) -> Result<(), EncodeError> {
    match frames.read_frame(frame, pixels) {
        Err(FrameError::Unreadable) => Err(EncodeError::FrameUnreadable { frame }),
        Err(FrameError::PixelCount { expected, actual }) => Err(EncodeError::FrameSize { frame, expected, actual }),
        Ok(()) if pixels.len() != pixel_count => {
            Err(EncodeError::FrameSize { frame, expected: pixel_count, actual: pixels.len() })
        }
        Ok(()) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writer::write_gif;

    const SHARP: ManualSettings = ManualSettings { colors: 255, dither: 0.0, tolerance: 0 };

    fn five_small_frames() -> Vec<Vec<Rgb>> {
        (0..5u8).map(|number| (0..8 * 6).map(|index| [index as u8 * 5, number * 40, 90]).collect()).collect()
    }

    #[test]
    fn progress_counts_up_one_step_at_a_time_to_the_total() {
        let frames = five_small_frames();
        let slices: Vec<&[Rgb]> = frames.iter().map(Vec::as_slice).collect();
        let mut reported = Vec::new();

        encode_frames(8, 6, &mut &slices[..], GifFps::Fps10, SHARP, &mut |progress| reported.push(progress)).unwrap();

        let reading_each_frame_twice = 10;
        let expected: Vec<Progress> = (1..=reading_each_frame_twice).map(|done| Progress { done, total: 10 }).collect();
        assert_eq!(reported, expected);
    }

    struct MadeOnDemand {
        frame_count: usize,
        slots: Option<Vec<usize>>,
        unreadable: Option<usize>,
        reads: Vec<usize>,
    }

    impl FrameSource for MadeOnDemand {
        fn frame_count(&self) -> usize {
            self.frame_count
        }

        fn slot(&self, index: usize) -> usize {
            self.slots.as_ref().map_or(index, |slots| slots[index])
        }

        fn read_frame(&mut self, index: usize, pixels: &mut Vec<Rgb>) -> Result<(), FrameError> {
            self.reads.push(index);
            if self.unreadable == Some(index) {
                return Err(FrameError::Unreadable);
            }
            pixels.clear();
            pixels.extend_from_slice(&five_small_frames()[index]);
            Ok(())
        }
    }

    #[test]
    fn frames_that_exist_one_at_a_time_give_the_same_file_as_frames_held_together() {
        let frames = five_small_frames();
        let slices: Vec<&[Rgb]> = frames.iter().map(Vec::as_slice).collect();
        let lossy = ManualSettings { colors: 16, dither: 1.0, tolerance: 8 };
        let mut on_demand = MadeOnDemand { frame_count: 5, slots: None, unreadable: None, reads: Vec::new() };

        let streamed = encode_frames(8, 6, &mut on_demand, GifFps::Fps10, lossy, &mut |_| {});

        assert_eq!(streamed, encode_gif(8, 6, &slices, GifFps::Fps10, lossy));
        assert_eq!(on_demand.reads, [0, 1, 2, 3, 4, 0, 1, 2, 3, 4], "once to choose colors, once to write");
    }

    fn delays_of(gif: &[u8]) -> Vec<u16> {
        let mut decoder = gif::DecodeOptions::new().read_info(gif).expect("a readable GIF header");
        let mut delays = Vec::new();
        while let Some(frame) = decoder.read_next_frame().expect("a readable frame") {
            delays.push(frame.delay);
        }
        delays
    }

    #[test]
    fn a_frame_taken_late_is_shown_until_the_next_frame_was_taken() {
        let mut late = MadeOnDemand { frame_count: 5, slots: Some(vec![0, 1, 4, 5, 6]), unreadable: None, reads: Vec::new() };

        let gif = encode_frames(8, 6, &mut late, GifFps::Fps10, SHARP, &mut |_| {}).unwrap();

        assert_eq!(delays_of(&gif), [10, 30, 10, 10, 10]);
    }

    #[test]
    fn frames_taken_on_time_keep_the_steady_delays() {
        let frames = five_small_frames();
        let slices: Vec<&[Rgb]> = frames.iter().map(Vec::as_slice).collect();
        let gif = encode_gif(8, 6, &slices, GifFps::Fps30, SHARP).unwrap();
        assert_eq!(delays_of(&gif), [3, 3, 4, 3, 3]);
    }

    #[test]
    fn a_frame_that_cannot_be_read_stops_the_encode_and_names_the_frame() {
        let mut broken = MadeOnDemand { frame_count: 5, slots: None, unreadable: Some(3), reads: Vec::new() };

        let stopped = encode_frames(8, 6, &mut broken, GifFps::Fps10, SHARP, &mut |_| {});

        assert_eq!(stopped, Err(EncodeError::FrameUnreadable { frame: 3 }));
        assert_eq!(broken.reads, [0, 1, 2, 3]);
    }

    #[test]
    fn input_that_is_refused_reports_no_progress() {
        let mut reported = Vec::new();
        let refused = encode_frames(8, 6, &mut &[][..], GifFps::Fps10, SHARP, &mut |progress| reported.push(progress));
        assert_eq!((refused, reported), (Err(EncodeError::NoFrames), vec![]));
    }

    fn shown_frames(bytes: &[u8]) -> Vec<(u16, Vec<Rgb>)> {
        let mut options = gif::DecodeOptions::new();
        options.set_color_output(gif::ColorOutput::Indexed);
        let mut decoder = options.read_info(bytes).expect("a readable GIF header");
        let palette: Vec<Rgb> = decoder
            .global_palette()
            .expect("a global palette")
            .chunks(3)
            .map(|color| [color[0], color[1], color[2]])
            .collect();
        let (width, height) = (usize::from(decoder.width()), usize::from(decoder.height()));

        let mut canvas = vec![[0, 0, 0]; width * height];
        let mut frames = Vec::new();
        while let Some(frame) = decoder.read_next_frame().expect("a readable frame") {
            let (left, top) = (usize::from(frame.left), usize::from(frame.top));
            let patch_width = usize::from(frame.width);
            for (position, &index) in frame.buffer.iter().enumerate() {
                if Some(index) != frame.transparent {
                    let (x, y) = (left + position % patch_width, top + position / patch_width);
                    canvas[y * width + x] = palette[usize::from(index)];
                }
            }
            frames.push((frame.delay, canvas.clone()));
        }
        frames
    }

    fn ramp(width: usize, height: usize, shift: usize) -> Vec<Rgb> {
        (0..width * height)
            .map(|index| {
                let (x, y) = (index % width, index / width);
                [(x * 255 / (width - 1)) as u8, (y * 255 / (height - 1)) as u8, ((x + y + shift) % 256) as u8]
            })
            .collect()
    }

    #[test]
    fn a_few_flat_colors_come_back_exactly() {
        let red = [255, 0, 0];
        let blue = [0, 0, 255];
        let first = [red, red, blue, blue];
        let second = [red, blue, blue, red];

        let bytes = encode_gif(2, 2, &[&first, &second], GifFps::Fps10, SHARP).unwrap();
        assert_eq!(shown_frames(&bytes), [(10, first.to_vec()), (10, second.to_vec())]);
    }

    #[test]
    fn the_picture_never_uses_more_colors_than_asked() {
        let frame = ramp(64, 48, 0);
        let settings = ManualSettings { colors: 16, dither: 1.0, tolerance: 0 };
        let bytes = encode_gif(64, 48, &[&frame], GifFps::Fps10, settings).unwrap();

        let mut used = shown_frames(&bytes)[0].1.clone();
        used.sort();
        used.dedup();
        assert!((2..=16).contains(&used.len()), "{} colors used", used.len());
    }

    #[test]
    fn it_gives_the_same_file_as_running_each_step_by_hand() {
        let frames = [ramp(48, 32, 0), ramp(48, 32, 40)];
        let slices: Vec<&[Rgb]> = frames.iter().map(Vec::as_slice).collect();
        let settings = ManualSettings { colors: 64, dither: 0.5, tolerance: 0 };

        let palette = Palette::from_frames(&slices, 64).unwrap();
        let indexed: Vec<Vec<u8>> =
            slices.iter().map(|frame| ordered_dither(&mut palette.lookup(), frame, 48, 0.5).unwrap()).collect();
        let indexed: Vec<&[u8]> = indexed.iter().map(Vec::as_slice).collect();
        let by_hand = write_gif(48, 32, palette.colors(), &indexed, GifFps::Fps15).unwrap();

        assert_eq!(encode_gif(48, 32, &slices, GifFps::Fps15, settings), Ok(by_hand));
    }

    fn flickering_still(width: usize, height: usize, frames: usize) -> Vec<Vec<Rgb>> {
        (0..frames)
            .map(|number| {
                (0..width * height)
                    .map(|index| {
                        let flicker = ((index * 7 + number * 13) % 5) as u8;
                        [100 + flicker, 150 - flicker, 60 + flicker]
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn flicker_within_the_tolerance_is_left_out_of_the_file() {
        let frames = flickering_still(40, 30, 8);
        let slices: Vec<&[Rgb]> = frames.iter().map(Vec::as_slice).collect();
        let encode = |tolerance| {
            encode_gif(40, 30, &slices, GifFps::Fps10, ManualSettings { tolerance, ..SHARP }).unwrap()
        };

        let exact = encode(0);
        let steady = encode(4);

        assert!(steady.len() * 2 < exact.len(), "exact {} bytes, steady {} bytes", exact.len(), steady.len());
        let shown = shown_frames(&steady);
        assert!(shown.iter().all(|(_, picture)| *picture == shown[0].1), "every frame shows the first picture");
    }

    #[test]
    fn a_tolerance_above_the_limit_is_rejected() {
        let frame = [[0, 0, 0]];
        let settings = ManualSettings { tolerance: 33, ..SHARP };
        assert_eq!(encode_gif(1, 1, &[&frame], GifFps::Fps10, settings), Err(EncodeError::Tolerance(33)));
    }

    #[test]
    fn settings_outside_their_range_are_rejected() {
        let frame = [[0, 0, 0]];
        let encode = |colors, dither| encode_gif(1, 1, &[&frame], GifFps::Fps10, ManualSettings { colors, dither, tolerance: 0 });

        assert_eq!(encode(1, 0.0), Err(EncodeError::Colors(1)));
        assert_eq!(encode(256, 0.0), Err(EncodeError::Colors(256)));
        assert_eq!(encode(16, -0.1), Err(EncodeError::Dither(-0.1)));
        assert_eq!(encode(16, 1.5), Err(EncodeError::Dither(1.5)));
        assert!(matches!(encode(16, f32::NAN), Err(EncodeError::Dither(_))));
        assert!(encode(2, 0.0).is_ok());
        assert!(encode(255, 1.0).is_ok());
    }

    #[test]
    fn frames_that_do_not_fit_the_size_are_rejected() {
        let two = [[0, 0, 0]; 2];
        let three = [[0, 0, 0]; 3];
        assert_eq!(encode_gif(0, 2, &[&two], GifFps::Fps10, SHARP), Err(EncodeError::EmptyImage));
        assert_eq!(encode_gif(2, 1, &[], GifFps::Fps10, SHARP), Err(EncodeError::NoFrames));
        assert_eq!(
            encode_gif(2, 1, &[&two, &three], GifFps::Fps10, SHARP),
            Err(EncodeError::FrameSize { frame: 1, expected: 2, actual: 3 })
        );
    }
}
