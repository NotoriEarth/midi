use crate::GifFps;
use crate::dither::ordered_dither;
use crate::palette::Palette;
use crate::stabilize::stabilize;
use crate::writer::{Rgb, write_gif};

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

pub fn encode_gif(
    width: u16,
    height: u16,
    frames: &[&[Rgb]],
    fps: GifFps,
    settings: ManualSettings,
) -> Result<Vec<u8>, EncodeError> {
    check_input(width, height, frames, settings)?;

    let steady = stabilize(frames, settings.tolerance);
    let steady: Vec<&[Rgb]> = steady.iter().map(Vec::as_slice).collect();
    let frames = &steady[..];

    let palette = Palette::from_frames(frames, settings.colors)
        .expect("the color limit is in range and the frames have pixels");
    let indexed: Vec<Vec<u8>> = frames
        .iter()
        .map(|frame| {
            ordered_dither(&palette, frame, usize::from(width), settings.dither)
                .expect("every frame is width x height pixels")
        })
        .collect();
    let indexed: Vec<&[u8]> = indexed.iter().map(Vec::as_slice).collect();

    let bytes = write_gif(width, height, palette.colors(), &indexed, fps)
        .expect("the indices come from the palette and the frames fit the size");
    Ok(bytes)
}

fn check_input(
    width: u16,
    height: u16,
    frames: &[&[Rgb]],
    settings: ManualSettings,
) -> Result<(), EncodeError> {
    settings.check()?;

    let expected = usize::from(width) * usize::from(height);
    if expected == 0 {
        return Err(EncodeError::EmptyImage);
    }
    if frames.is_empty() {
        return Err(EncodeError::NoFrames);
    }
    match frames.iter().position(|frame| frame.len() != expected) {
        Some(frame) => Err(EncodeError::FrameSize { frame, expected, actual: frames[frame].len() }),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHARP: ManualSettings = ManualSettings { colors: 255, dither: 0.0, tolerance: 0 };

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
            slices.iter().map(|frame| ordered_dither(&palette, frame, 48, 0.5).unwrap()).collect();
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
