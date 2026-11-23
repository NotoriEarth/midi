use std::collections::HashMap;
use std::ops::Range;

use crate::writer::Rgb;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette(Vec<Rgb>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteError {
    NoPixels,
    ColorLimit(usize),
}

type CountedColor = (Rgb, u32);

impl Palette {
    pub const MAX_COLORS: usize = NOT_ASKED_YET as usize;

    pub fn from_frames(frames: &[&[Rgb]], max_colors: usize) -> Result<Palette, PaletteError> {
        if !(1..=Self::MAX_COLORS).contains(&max_colors) {
            return Err(PaletteError::ColorLimit(max_colors));
        }
        let mut counts = ColorCounts::new();
        for frame in frames {
            counts.add(frame);
        }
        Self::from_counts(&counts, max_colors)
    }

    pub fn from_counts(counts: &ColorCounts, max_colors: usize) -> Result<Palette, PaletteError> {
        if !(1..=Self::MAX_COLORS).contains(&max_colors) {
            return Err(PaletteError::ColorLimit(max_colors));
        }
        let mut colors = counts.in_color_order();
        if colors.is_empty() {
            return Err(PaletteError::NoPixels);
        }

        let mut boxes = Vec::with_capacity(max_colors);
        boxes.push(0..colors.len());
        while boxes.len() < max_colors {
            let Some(widest) = widest_box_that_can_split(&colors, &boxes) else {
                break;
            };
            let Range { start, end } = boxes.swap_remove(widest);
            let cut = start + split_at_median(&mut colors[start..end]);
            boxes.push(start..cut);
            boxes.push(cut..end);
        }

        Ok(Palette(boxes.into_iter().map(|range| average(&colors[range])).collect()))
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

    pub fn lookup(&self) -> NearestColor<'_> {
        NearestColor { palette: self, answers: vec![NOT_ASKED_YET; EVERY_RGB_COLOR] }
    }

    pub fn nearest(&self, pixel: Rgb) -> u8 {
        let index = (0..self.0.len())
            .min_by_key(|&index| squared_distance(self.0[index], pixel))
            .expect("a palette always has at least one color");
        index as u8
    }
}

const EVERY_RGB_COLOR: usize = 1 << 24;
const NOT_ASKED_YET: u8 = u8::MAX;

#[derive(Debug, Clone)]
pub struct NearestColor<'a> {
    palette: &'a Palette,
    answers: Vec<u8>,
}

impl NearestColor<'_> {
    pub fn palette(&self) -> &Palette {
        self.palette
    }

    pub fn of(&mut self, pixel: Rgb) -> u8 {
        let slot = slot_of(pixel);
        if self.answers[slot] == NOT_ASKED_YET {
            self.answers[slot] = self.palette.nearest(pixel);
        }
        self.answers[slot]
    }
}

fn slot_of(pixel: Rgb) -> usize {
    let [red, green, blue] = pixel.map(usize::from);
    red << 16 | green << 8 | blue
}

fn color_of(slot: usize) -> Rgb {
    [(slot >> 16) as u8, (slot >> 8) as u8, slot as u8]
}

#[derive(Debug, Clone)]
pub struct ColorCounts(Vec<u32>);

impl ColorCounts {
    pub fn new() -> Self {
        Self(vec![0; EVERY_RGB_COLOR])
    }

    pub fn add(&mut self, pixels: &[Rgb]) {
        for &pixel in pixels {
            let count = &mut self.0[slot_of(pixel)];
            *count = count.saturating_add(1);
        }
    }

    fn in_color_order(&self) -> Vec<CountedColor> {
        let mut colors = Vec::with_capacity(self.0.iter().filter(|&&count| count > 0).count());
        let seen = self.0.iter().enumerate().filter(|&(_, &count)| count > 0);
        colors.extend(seen.map(|(slot, &count)| (color_of(slot), count)));
        colors
    }
}

impl Default for ColorCounts {
    fn default() -> Self {
        Self::new()
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
    colors.iter().map(|&(_, count)| u64::from(count)).sum()
}

fn widest_box_that_can_split(colors: &[CountedColor], boxes: &[Range<usize>]) -> Option<usize> {
    (0..boxes.len())
        .rev()
        .filter(|&index| boxes[index].len() > 1)
        .max_by_key(|&index| {
            let colors = &colors[boxes[index].clone()];
            (widest_channel(colors).0, pixel_count(colors))
        })
}

fn split_at_median(colors: &mut [CountedColor]) -> usize {
    let (_, channel) = widest_channel(colors);
    colors.sort_unstable_by_key(|&(color, _)| (color[channel], color));

    let half = pixel_count(colors).div_ceil(2);
    let mut seen = 0;
    let reaches_half = colors.iter().position(|&(_, count)| {
        seen += u64::from(count);
        seen >= half
    });
    reaches_half.map_or(1, |position| position + 1).clamp(1, colors.len() - 1)
}

fn average(colors: &[CountedColor]) -> Rgb {
    let total = pixel_count(colors);
    std::array::from_fn(|channel| {
        let sum: u64 = colors.iter().map(|&(color, count)| u64::from(color[channel]) * u64::from(count)).sum();
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
        let palette = Palette::from_frames(&[&pixels], Palette::MAX_COLORS).unwrap();

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
        let palette = Palette::from_frames(&[&first, &second], Palette::MAX_COLORS).unwrap();
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

    fn many_colors() -> Vec<Rgb> {
        (0..5000u32).map(|n| [(n * 7) as u8, (n * 13) as u8, (n * 29) as u8]).collect()
    }

    #[test]
    fn the_lookup_answers_exactly_like_searching_every_palette_color() {
        let palette = Palette::from_frames(&[&many_colors()], Palette::MAX_COLORS).unwrap();
        let mut lookup = palette.lookup();

        let between_palette_colors = (0..4000u32).map(|n| [(n * 31) as u8, (n * 3) as u8, (n * 17) as u8]);
        for color in many_colors().into_iter().chain(between_palette_colors) {
            assert_eq!(lookup.of(color), palette.nearest(color), "{color:?}");
        }
    }

    #[test]
    fn a_color_asked_again_gets_the_same_answer() {
        let palette = Palette::from_frames(&[&many_colors()], 64).unwrap();
        let mut lookup = palette.lookup();

        let first: Vec<u8> = many_colors().into_iter().map(|color| lookup.of(color)).collect();
        let again: Vec<u8> = many_colors().into_iter().map(|color| lookup.of(color)).collect();

        assert_eq!(again, first);
        assert_eq!(first, palette.to_indices(&many_colors()));
    }

    #[test]
    fn every_entry_of_a_full_palette_can_be_the_answer() {
        let palette = Palette::from_frames(&[&many_colors()], Palette::MAX_COLORS).unwrap();
        assert_eq!(palette.colors().len(), Palette::MAX_COLORS);
        let mut lookup = palette.lookup();

        for _ in 0..2 {
            for (index, &color) in palette.colors().iter().enumerate() {
                assert_eq!(usize::from(lookup.of(color)), usize::from(palette.nearest(color)), "entry {index}");
            }
        }
        let answers: std::collections::HashSet<u8> = palette.colors().iter().map(|&color| lookup.of(color)).collect();
        assert!(answers.contains(&254), "the last entry is never mistaken for an empty slot");
    }

    #[test]
    fn colors_are_counted_in_order_with_how_often_each_appears() {
        let first = [[0, 0, 255], [255, 0, 0], [0, 0, 255]];
        let second = [[0, 1, 0], [0, 0, 255]];
        let mut counts = ColorCounts::new();
        counts.add(&first);
        counts.add(&second);
        assert_eq!(counts.in_color_order(), [([0, 0, 255], 3), ([0, 1, 0], 1), ([255, 0, 0], 1)]);
    }

    #[test]
    fn counting_frame_by_frame_chooses_the_same_palette_as_all_frames_at_once() {
        let pixels = many_colors();
        let (first, second) = pixels.split_at(1234);
        let mut counts = ColorCounts::new();
        counts.add(first);
        counts.add(second);

        assert_eq!(Palette::from_counts(&counts, 32), Palette::from_frames(&[first, second], 32));
        assert_eq!(Palette::from_counts(&ColorCounts::new(), 32), Err(PaletteError::NoPixels));
        assert_eq!(Palette::from_counts(&counts, 256), Err(PaletteError::ColorLimit(256)));
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
        assert_eq!(Palette::from_frames(&[], 255), Err(PaletteError::NoPixels));
        assert_eq!(Palette::from_frames(&[&[]], 255), Err(PaletteError::NoPixels));
        assert_eq!(Palette::from_frames(&[&pixel], 0), Err(PaletteError::ColorLimit(0)));
        assert_eq!(Palette::from_frames(&[&pixel], 256), Err(PaletteError::ColorLimit(256)));
    }

    #[test]
    fn rgb_frames_become_a_gif_that_decodes_to_the_same_colors() {
        let frame: Vec<Rgb> = (0..12 * 12).map(|n| [(n % 12 * 20) as u8, (n / 12 * 20) as u8, 90]).collect();
        let palette = Palette::from_frames(&[&frame], Palette::MAX_COLORS).unwrap();
        let indices = palette.to_indices(&frame);
        let bytes = write_gif(12, 12, palette.colors(), &[&indices], GifFps::Fps10).unwrap();

        let mut options = gif::DecodeOptions::new();
        options.set_color_output(gif::ColorOutput::RGBA);
        let mut decoder = options.read_info(&bytes[..]).unwrap();
        let decoded = decoder.read_next_frame().unwrap().unwrap();
        let shown: Vec<Rgb> = decoded.buffer.chunks(4).map(|rgba| [rgba[0], rgba[1], rgba[2]]).collect();
        assert_eq!(shown, frame);
    }
}
