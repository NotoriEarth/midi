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
    const REFINING_PASSES: usize = 3;

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
        boxes.push(ColorBox::new(0..colors.len(), &colors));
        while boxes.len() < max_colors {
            let Some(worst) = worst_box_that_can_split(&boxes) else {
                break;
            };
            let Range { start, end } = boxes.swap_remove(worst).colors;
            let cut = start + split_where_error_is_least(&mut colors[start..end]);
            boxes.push(ColorBox::new(start..cut, &colors));
            boxes.push(ColorBox::new(cut..end, &colors));
        }

        let mut palette = Palette(boxes.into_iter().map(|color_box| color_box.moments.mean()).collect());
        for _ in 0..Self::REFINING_PASSES {
            palette = palette.moved_to_the_middle_of(&colors);
        }
        Ok(palette)
    }

    fn moved_to_the_middle_of(&self, colors: &[CountedColor]) -> Palette {
        let near_cells = NearCells::of(self);
        let mut nearest_to = vec![Moments::default(); self.0.len()];
        for &(color, count) in colors {
            nearest_to[usize::from(near_cells.nearest(color))].add(color, count);
        }
        let moved = self.0.iter().zip(&nearest_to).map(|(&color, moments)| if moments.pixels == 0 { color } else { moments.mean() });
        Palette(moved.collect())
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
        NearestColor { palette: self, near_cells: NearCells::of(self), answers: vec![NOT_ASKED_YET; EVERY_RGB_COLOR] }
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
    near_cells: NearCells,
    answers: Vec<u8>,
}

#[derive(Debug, Clone)]
struct NearCells {
    starts: Vec<usize>,
    candidates: Vec<(Rgb, u8)>,
}

impl NearCells {
    const CELL_BITS: u32 = 4;
    const CELL_SIZE: u8 = 1 << Self::CELL_BITS;
    const CELL_COUNT: usize = 1 << (3 * (8 - Self::CELL_BITS));

    fn of(palette: &Palette) -> Self {
        let mut starts = Vec::with_capacity(Self::CELL_COUNT + 1);
        let mut candidates = Vec::new();
        for cell in 0..Self::CELL_COUNT {
            starts.push(candidates.len());
            let (low, high) = Self::corners(cell);
            let farthest_of_the_nearest = palette.0.iter().map(|&color| farthest_in(low, high, color)).min();
            let bound = farthest_of_the_nearest.expect("a palette always has at least one color");
            let can_be_nearest = palette.0.iter().enumerate().filter(|&(_, &color)| nearest_in(low, high, color) <= bound);
            candidates.extend(can_be_nearest.map(|(index, &color)| (color, index as u8)));
        }
        starts.push(candidates.len());
        Self { starts, candidates }
    }

    fn corners(cell: usize) -> (Rgb, Rgb) {
        let per_axis = 8 - Self::CELL_BITS;
        let mask = (1 << per_axis) - 1;
        let low = [cell >> (2 * per_axis), cell >> per_axis, cell].map(|part| ((part & mask) as u8) << Self::CELL_BITS);
        (low, low.map(|channel| channel + (Self::CELL_SIZE - 1)))
    }

    fn cell_of(pixel: Rgb) -> usize {
        let per_axis = 8 - Self::CELL_BITS;
        let [red, green, blue] = pixel.map(|channel| usize::from(channel >> Self::CELL_BITS));
        red << (2 * per_axis) | green << per_axis | blue
    }

    fn near(&self, pixel: Rgb) -> &[(Rgb, u8)] {
        let cell = Self::cell_of(pixel);
        &self.candidates[self.starts[cell]..self.starts[cell + 1]]
    }

    fn nearest(&self, pixel: Rgb) -> u8 {
        let best = self.near(pixel).iter().map(|&(color, index)| (squared_distance(color, pixel), index)).min();
        best.expect("every cell has at least one candidate").1
    }
}

fn nearest_in(low: Rgb, high: Rgb, color: Rgb) -> u32 {
    (0..3).map(|channel| u32::from(color[channel].saturating_sub(high[channel]).max(low[channel].saturating_sub(color[channel]))).pow(2)).sum()
}

fn farthest_in(low: Rgb, high: Rgb, color: Rgb) -> u32 {
    (0..3).map(|channel| u32::from(color[channel].abs_diff(low[channel]).max(color[channel].abs_diff(high[channel]))).pow(2)).sum()
}

impl NearestColor<'_> {
    pub fn palette(&self) -> &Palette {
        self.palette
    }

    pub fn of(&mut self, pixel: Rgb) -> u8 {
        let slot = slot_of(pixel);
        if self.answers[slot] == NOT_ASKED_YET {
            self.answers[slot] = self.search(pixel);
        }
        self.answers[slot]
    }

    #[inline(never)]
    fn search(&self, pixel: Rgb) -> u8 {
        self.near_cells.nearest(pixel)
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

#[derive(Debug, Clone, Copy, Default)]
struct Moments {
    pixels: u64,
    sums: [u64; 3],
    squares: u64,
}

impl Moments {
    fn of(colors: &[CountedColor]) -> Self {
        let mut moments = Moments::default();
        for &(color, count) in colors {
            moments.add(color, count);
        }
        moments
    }

    fn add(&mut self, color: Rgb, count: u32) {
        let count = u64::from(count);
        self.pixels += count;
        for channel in 0..3 {
            let value = u64::from(color[channel]);
            self.sums[channel] += value * count;
            self.squares += value * value * count;
        }
    }

    fn plus(self, other: Moments) -> Moments {
        Moments {
            pixels: self.pixels + other.pixels,
            sums: std::array::from_fn(|channel| self.sums[channel] + other.sums[channel]),
            squares: self.squares + other.squares,
        }
    }

    fn minus(self, other: Moments) -> Moments {
        Moments {
            pixels: self.pixels - other.pixels,
            sums: std::array::from_fn(|channel| self.sums[channel] - other.sums[channel]),
            squares: self.squares - other.squares,
        }
    }

    fn squared_error(&self) -> f64 {
        let spread_of_the_mean: f64 = self.sums.iter().map(|&sum| (sum as f64).powi(2)).sum();
        self.squares as f64 - spread_of_the_mean / self.pixels as f64
    }

    fn mean(&self) -> Rgb {
        self.sums.map(|sum| ((sum + self.pixels / 2) / self.pixels) as u8)
    }
}

struct ColorBox {
    colors: Range<usize>,
    moments: Moments,
}

impl ColorBox {
    fn new(colors: Range<usize>, every_color: &[CountedColor]) -> Self {
        ColorBox { moments: Moments::of(&every_color[colors.clone()]), colors }
    }
}

fn worst_box_that_can_split(boxes: &[ColorBox]) -> Option<usize> {
    (0..boxes.len())
        .rev()
        .filter(|&index| boxes[index].colors.len() > 1)
        .max_by(|&a, &b| boxes[a].moments.squared_error().total_cmp(&boxes[b].moments.squared_error()))
}

fn split_where_error_is_least(colors: &mut [CountedColor]) -> usize {
    let mut per_value = [[Moments::default(); 256]; 3];
    for &(color, count) in colors.iter() {
        for channel in 0..3 {
            per_value[channel][usize::from(color[channel])].add(color, count);
        }
    }
    let whole = per_value[0].iter().fold(Moments::default(), |sum, &moments| sum.plus(moments));

    let mut best: Option<(f64, usize, u8)> = None;
    for (channel, values) in per_value.iter().enumerate() {
        let mut below = Moments::default();
        for value in 0..u8::MAX {
            below = below.plus(values[usize::from(value)]);
            if below.pixels == 0 || below.pixels == whole.pixels {
                continue;
            }
            let error = below.squared_error() + whole.minus(below).squared_error();
            if best.is_none_or(|(least, ..)| error < least) {
                best = Some((error, channel, value));
            }
        }
    }
    let (_, channel, highest_below) = best.expect("a box of two or more colors differs in some channel");

    let mut lower_end = 0;
    for next in 0..colors.len() {
        if colors[next].0[channel] <= highest_below {
            colors.swap(lower_end, next);
            lower_end += 1;
        }
    }
    lower_end
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

    fn plain_median_cut(pixels: &[Rgb], max_colors: usize) -> Palette {
        let widest = |colors: &[Rgb]| {
            let range = |channel: usize| {
                let values = colors.iter().map(|color| color[channel]);
                values.clone().max().unwrap() - values.min().unwrap()
            };
            (0..3).map(|channel| (range(channel), channel)).max().unwrap()
        };
        let mut boxes = vec![pixels.to_vec()];
        while boxes.len() < max_colors {
            let Some(index) = (0..boxes.len()).filter(|&index| widest(&boxes[index]).0 > 0).max_by_key(|&index| widest(&boxes[index]).0)
            else {
                break;
            };
            let mut colors = boxes.swap_remove(index);
            let channel = widest(&colors).1;
            colors.sort_by_key(|&color| (color[channel], color));
            let upper = colors.split_off(colors.len() / 2);
            boxes.push(colors);
            boxes.push(upper);
        }
        let average = |colors: &[Rgb]| -> Rgb {
            std::array::from_fn(|channel| {
                let sum: usize = colors.iter().map(|color| usize::from(color[channel])).sum();
                ((sum + colors.len() / 2) / colors.len()) as u8
            })
        };
        Palette(boxes.iter().map(|colors| average(colors)).collect())
    }

    fn squared_error_shown_by(palette: &Palette, pixels: &[Rgb]) -> u64 {
        let shown = palette.to_indices(pixels).into_iter().map(|index| palette.colors()[usize::from(index)]);
        pixels.iter().zip(shown).map(|(&pixel, shown)| u64::from(squared_distance(pixel, shown))).sum()
    }

    fn dense_cluster_and_rare_spread() -> Vec<Rgb> {
        (0..30_000u32)
            .map(|n| if n % 10 == 0 { [(n * 7) as u8, (n * 13) as u8, (n * 29) as u8] } else { [120 + (n % 9) as u8, 60 + (n % 5) as u8, 30] })
            .collect()
    }

    fn error_against_plain_median_cut(pixels: &[Rgb], max_colors: usize) -> (u64, u64) {
        let chosen = squared_error_shown_by(&Palette::from_frames(&[pixels], max_colors).unwrap(), pixels);
        (chosen, squared_error_shown_by(&plain_median_cut(pixels, max_colors), pixels))
    }

    #[test]
    fn unevenly_spread_colors_are_shown_with_less_error_than_plain_median_cut() {
        let mostly_one_value: Vec<Rgb> =
            (0..20_000u32).map(|n| if n % 5 == 0 { [(n % 256) as u8, (n / 256) as u8, 3] } else { [90, (n % 7) as u8, (n % 11) as u8] }).collect();
        let cases = [("many", many_colors()), ("mostly one value", mostly_one_value), ("dense cluster and rare spread", dense_cluster_and_rare_spread())];
        for (name, pixels) in &cases {
            for max_colors in [4, 16, 64, Palette::MAX_COLORS] {
                let (chosen, median_cut) = error_against_plain_median_cut(pixels, max_colors);
                assert!(chosen < median_cut, "{name}, {max_colors} colors: {chosen} against {median_cut}");
            }
        }
    }

    #[test]
    fn an_even_gradient_is_shown_about_as_well_as_plain_median_cut() {
        let gradient: Vec<Rgb> = (0..=255u8).flat_map(|red| (0..=255u8).step_by(3).map(move |green| [red, green, 7])).collect();
        for max_colors in [4, 16, 64, Palette::MAX_COLORS] {
            let (chosen, median_cut) = error_against_plain_median_cut(&gradient, max_colors);
            assert!(chosen * 100 <= median_cut * 101, "{max_colors} colors: {chosen} against {median_cut}");
        }
    }

    #[test]
    fn rare_colors_far_from_a_dense_cluster_are_shown_with_far_less_error_than_plain_median_cut() {
        for max_colors in [16, 64] {
            let (chosen, median_cut) = error_against_plain_median_cut(&dense_cluster_and_rare_spread(), max_colors);
            assert!(chosen * 2 < median_cut, "{max_colors} colors: {chosen} against {median_cut}");
        }
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
    fn a_color_as_near_to_two_palette_colors_gets_the_earlier_one() {
        let palette = Palette(vec![[0, 40, 0], [0, 20, 0], [0, 0, 0], [0, 20, 0], [10, 30, 0]]);
        let mut lookup = palette.lookup();

        assert_eq!(palette.nearest([0, 10, 0]), 1);
        for color in [[0, 10, 0], [0, 30, 0], [5, 25, 0], [0, 20, 0], [0, 50, 0], [5, 35, 0], [10, 10, 0]] {
            assert_eq!(lookup.of(color), palette.nearest(color), "{color:?}");
        }
    }

    #[test]
    fn colors_on_the_edges_of_the_lookup_cells_get_the_same_answer_as_searching_every_color() {
        let on_a_grid: Vec<Rgb> = (0..255u32).map(|n| [(n % 8 * 32) as u8, (n / 8 % 8 * 32) as u8, (n / 64 * 32) as u8]).collect();
        let cell_edges: Vec<u8> = (0..=255u8).filter(|value| value % 16 == 0 || value % 16 == 15).collect();
        for pixels in [on_a_grid, many_colors()] {
            let palette = Palette::from_frames(&[&pixels], Palette::MAX_COLORS).unwrap();
            let mut lookup = palette.lookup();
            for &red in &cell_edges {
                for &green in &cell_edges {
                    for &blue in &cell_edges {
                        let color = [red, green, blue];
                        assert_eq!(lookup.of(color), palette.nearest(color), "{color:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_lookup_answers_like_searching_every_palette_color_across_all_colors() {
        let palette = Palette::from_frames(&[&many_colors()], Palette::MAX_COLORS).unwrap();
        let mut lookup = palette.lookup();

        let steps = (0..=255u8).step_by(7);
        for red in steps.clone() {
            for green in steps.clone() {
                for blue in steps.clone() {
                    let color = [red, green, blue];
                    assert_eq!(lookup.of(color), palette.nearest(color), "{color:?}");
                }
            }
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
