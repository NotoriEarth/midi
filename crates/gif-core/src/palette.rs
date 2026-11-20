use std::collections::HashMap;

use crate::writer::Rgb;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette(Vec<Rgb>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteError {
    NoPixels,
    ColorLimit(usize),
}

type CountedColor = (Rgb, u64);

impl Palette {
    pub const MAX_COLORS: usize = 256;

    pub fn from_frames(frames: &[&[Rgb]], max_colors: usize) -> Result<Palette, PaletteError> {
        if !(1..=Self::MAX_COLORS).contains(&max_colors) {
            return Err(PaletteError::ColorLimit(max_colors));
        }

        let mut counts: HashMap<Rgb, u64> = HashMap::new();
        for &pixel in frames.iter().copied().flatten() {
            *counts.entry(pixel).or_default() += 1;
        }
        if counts.is_empty() {
            return Err(PaletteError::NoPixels);
        }
        let mut every_color: Vec<CountedColor> = counts.into_iter().collect();
        every_color.sort();

        let mut boxes = vec![every_color];
        while boxes.len() < max_colors {
            let Some(widest) = widest_box_that_can_split(&boxes) else {
                break;
            };
            let (lower, upper) = split_at_median(boxes.swap_remove(widest));
            boxes.push(lower);
            boxes.push(upper);
        }

        Ok(Palette(boxes.iter().map(|colors| average(colors)).collect()))
    }

    pub fn colors(&self) -> &[Rgb] {
        &self.0
    }

    pub fn to_indices(&self, pixels: &[Rgb]) -> Vec<u8> {
        let mut known: HashMap<Rgb, u8> = HashMap::new();
        pixels
            .iter()
            .map(|&pixel| *known.entry(pixel).or_insert_with(|| self.nearest(pixel)))
            .collect()
    }

    fn nearest(&self, pixel: Rgb) -> u8 {
        let index = (0..self.0.len())
            .min_by_key(|&index| squared_distance(self.0[index], pixel))
            .expect("a palette always has at least one color");
        index as u8
    }
}

fn squared_distance(a: Rgb, b: Rgb) -> u32 {
    (0..3).map(|channel| u32::from(a[channel].abs_diff(b[channel])).pow(2)).sum()
}

fn widest_channel(colors: &[CountedColor]) -> (u8, usize) {
    (0..3)
        .map(|channel| {
            let values = colors.iter().map(|(color, _)| color[channel]);
            let range = values.clone().max().unwrap_or(0) - values.min().unwrap_or(0);
            (range, channel)
        })
        .rev()
        .max_by_key(|&(range, _)| range)
        .expect("there are three channels")
}

fn pixel_count(colors: &[CountedColor]) -> u64 {
    colors.iter().map(|(_, count)| count).sum()
}

fn widest_box_that_can_split(boxes: &[Vec<CountedColor>]) -> Option<usize> {
    (0..boxes.len())
        .rev()
        .filter(|&index| boxes[index].len() > 1)
        .max_by_key(|&index| (widest_channel(&boxes[index]).0, pixel_count(&boxes[index])))
}

fn split_at_median(mut colors: Vec<CountedColor>) -> (Vec<CountedColor>, Vec<CountedColor>) {
    let (_, channel) = widest_channel(&colors);
    colors.sort_by_key(|&(color, _)| (color[channel], color));

    let half = pixel_count(&colors).div_ceil(2);
    let mut seen = 0;
    let reaches_half = colors.iter().position(|(_, count)| {
        seen += count;
        seen >= half
    });
    let cut = reaches_half.map_or(1, |position| position + 1).clamp(1, colors.len() - 1);

    let upper = colors.split_off(cut);
    (colors, upper)
}

fn average(colors: &[CountedColor]) -> Rgb {
    let total = pixel_count(colors);
    std::array::from_fn(|channel| {
        let sum: u64 = colors.iter().map(|(color, count)| u64::from(color[channel]) * count).sum();
        ((sum + total / 2) / total) as u8
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GifFps;
    use crate::writer::write_gif;

    fn gray(level: u8) -> Rgb {
        [level, level, level]
    }

    fn channel_distance(a: Rgb, b: Rgb) -> u8 {
        (0..3).map(|channel| a[channel].abs_diff(b[channel])).max().unwrap()
    }

    #[test]
    fn an_image_within_the_limit_keeps_its_exact_colors() {
        let pixels = [[255, 0, 0], [0, 255, 0], [255, 0, 0], [0, 0, 255]];
        let palette = Palette::from_frames(&[&pixels], 256).unwrap();

        assert_eq!(palette.colors().len(), 3);
        let restored: Vec<Rgb> =
            palette.to_indices(&pixels).iter().map(|&index| palette.colors()[usize::from(index)]).collect();
        assert_eq!(restored, pixels);
    }

    #[test]
    fn more_colors_than_the_limit_are_reduced_to_exactly_the_limit() {
        let pixels: Vec<Rgb> = (0..=255).flat_map(|r| (0..=255).step_by(5).map(move |g| [r, g, 128])).collect();
        let palette = Palette::from_frames(&[&pixels], 16).unwrap();
        assert_eq!(palette.colors().len(), 16);
    }

    #[test]
    fn a_gray_ramp_cut_into_sixteen_stays_within_half_a_step_of_the_original() {
        let pixels: Vec<Rgb> = (0..=255).map(gray).collect();
        let palette = Palette::from_frames(&[&pixels], 16).unwrap();

        for (&pixel, &index) in pixels.iter().zip(&palette.to_indices(&pixels)) {
            let shown = palette.colors()[usize::from(index)];
            assert!(channel_distance(pixel, shown) <= 8, "{pixel:?} shown as {shown:?}");
        }
    }

    #[test]
    fn two_clusters_get_one_color_each() {
        let reds = (200..=255).map(|r| [r, 10, 10]);
        let blues = (200..=255).map(|b| [10, 10, b]);
        let pixels: Vec<Rgb> = reds.chain(blues).collect();
        let palette = Palette::from_frames(&[&pixels], 2).unwrap();

        let mut colors = palette.colors().to_vec();
        colors.sort();
        assert_eq!(colors, [[10, 10, 228], [228, 10, 10]]);
    }

    #[test]
    fn a_color_that_fills_the_image_pulls_its_palette_entry_toward_itself() {
        let mut pixels = vec![gray(100); 999];
        pixels.push(gray(200));
        pixels.push([255, 0, 0]);
        let palette = Palette::from_frames(&[&pixels], 2).unwrap();
        assert!(palette.colors().contains(&gray(100)), "{:?}", palette.colors());
    }

    #[test]
    fn colors_from_every_frame_are_counted() {
        let first = [[255, 0, 0]];
        let second = [[0, 0, 255]];
        let palette = Palette::from_frames(&[&first, &second], 256).unwrap();
        let mut colors = palette.colors().to_vec();
        colors.sort();
        assert_eq!(colors, [[0, 0, 255], [255, 0, 0]]);
    }

    #[test]
    fn each_pixel_maps_to_the_nearest_palette_color() {
        let pixels = [gray(0), gray(255)];
        let palette = Palette::from_frames(&[&pixels], 2).unwrap();
        let black = palette.to_indices(&[gray(0)])[0];
        let white = palette.to_indices(&[gray(255)])[0];
        assert_ne!(black, white);

        assert_eq!(palette.to_indices(&[gray(10), gray(200), [120, 130, 140]]), [black, white, white]);
    }

    #[test]
    fn the_same_input_always_gives_the_same_palette() {
        let pixels: Vec<Rgb> = (0..5000u32).map(|n| [(n * 7) as u8, (n * 13) as u8, (n * 29) as u8]).collect();
        let first = Palette::from_frames(&[&pixels], 32).unwrap();
        for _ in 0..5 {
            assert_eq!(Palette::from_frames(&[&pixels], 32).unwrap(), first);
        }
    }

    #[test]
    fn input_that_cannot_make_a_palette_is_rejected() {
        let pixel = [gray(0)];
        assert_eq!(Palette::from_frames(&[], 256), Err(PaletteError::NoPixels));
        assert_eq!(Palette::from_frames(&[&[]], 256), Err(PaletteError::NoPixels));
        assert_eq!(Palette::from_frames(&[&pixel], 0), Err(PaletteError::ColorLimit(0)));
        assert_eq!(Palette::from_frames(&[&pixel], 257), Err(PaletteError::ColorLimit(257)));
    }

    #[test]
    fn rgb_frames_become_a_gif_that_decodes_to_the_same_colors() {
        let frame: Vec<Rgb> = (0..16 * 16).map(|n| [(n % 16 * 17) as u8, (n / 16 * 17) as u8, 90]).collect();
        let palette = Palette::from_frames(&[&frame], 256).unwrap();
        let indices = palette.to_indices(&frame);
        let bytes = write_gif(16, 16, palette.colors(), &[&indices], GifFps::Fps10).unwrap();

        let mut options = gif::DecodeOptions::new();
        options.set_color_output(gif::ColorOutput::RGBA);
        let mut decoder = options.read_info(&bytes[..]).unwrap();
        let decoded = decoder.read_next_frame().unwrap().unwrap();
        let shown: Vec<Rgb> = decoded.buffer.chunks(4).map(|rgba| [rgba[0], rgba[1], rgba[2]]).collect();
        assert_eq!(shown, frame);
    }
}
