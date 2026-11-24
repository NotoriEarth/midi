use crate::palette::{NearestColor, Palette};
use crate::writer::Rgb;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DitherError {
    WidthDoesNotFit { width: usize, pixels: usize },
}

const BAYER_SIZE: usize = 8;
const BAYER: [[u8; BAYER_SIZE]; BAYER_SIZE] = [
    [0, 32, 8, 40, 2, 34, 10, 42],
    [48, 16, 56, 24, 50, 18, 58, 26],
    [12, 44, 4, 36, 14, 46, 6, 38],
    [60, 28, 52, 20, 62, 30, 54, 22],
    [3, 35, 11, 43, 1, 33, 9, 41],
    [51, 19, 59, 27, 49, 17, 57, 25],
    [15, 47, 7, 39, 13, 45, 5, 37],
    [63, 31, 55, 23, 61, 29, 53, 21],
];

pub fn ordered_dither(
    nearest: &mut NearestColor,
    pixels: &[Rgb],
    width: usize,
    strength: f32,
) -> Result<Vec<u8>, DitherError> {
    if width == 0 || !pixels.len().is_multiple_of(width) {
        return Err(DitherError::WidthDoesNotFit { width, pixels: pixels.len() });
    }

    let partners = Partners::of(nearest.palette());
    let mut blends = RememberedBlends::new();
    let rows = pixels.chunks_exact(width).enumerate();
    let indices = rows.flat_map(|(y, row)| {
        let ranks = &BAYER[y % BAYER_SIZE];
        row.iter().enumerate().map(move |(x, &pixel)| (pixel, ranks[x % BAYER_SIZE]))
    });
    let indices = indices.map(|(pixel, rank)| {
        let blend = blends.of(pixel, || partners.best_blend(pixel, nearest.of(pixel), strength));
        if rank < blend.partner_ranks { blend.partner } else { blend.closest }
    });
    Ok(indices.collect())
}

const PARTNERS_PER_COLOR: usize = 4;
const PATTERN_CONTRAST_WEIGHT: f32 = 0.75;
const LARGEST_MISS_A_BLEND_MAY_LEAVE: f32 = 0.1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Blend {
    closest: u8,
    partner: u8,
    partner_ranks: u8,
}

#[derive(Debug, Clone, Copy)]
struct Partner {
    index: u8,
    step: [f32; 3],
    squared_step: f32,
}

struct Partners {
    colors: Vec<[f32; 3]>,
    per_color: usize,
    nearest_others: Vec<Partner>,
}

impl Partners {
    fn of(palette: &Palette) -> Self {
        let colors: Vec<[f32; 3]> = palette.colors().iter().map(|color| color.map(f32::from)).collect();
        let per_color = PARTNERS_PER_COLOR.min(colors.len().saturating_sub(1));
        let mut nearest_others = Vec::with_capacity(colors.len() * per_color);
        let mut others: Vec<u8> = Vec::with_capacity(colors.len());
        for (index, &color) in colors.iter().enumerate() {
            others.clear();
            others.extend((0..colors.len()).filter(|&other| other != index).map(|other| other as u8));
            let distance = |other: &u8| squared_length(minus(colors[usize::from(*other)], color));
            if per_color < others.len() {
                others.select_nth_unstable_by(per_color, |a, b| distance(a).total_cmp(&distance(b)));
            }
            nearest_others.extend(others[..per_color].iter().map(|&partner| {
                let step = minus(colors[usize::from(partner)], color);
                Partner { index: partner, step, squared_step: squared_length(step) }
            }));
        }
        Self { colors, per_color, nearest_others }
    }

    fn best_blend(&self, pixel: Rgb, closest: u8, strength: f32) -> Blend {
        let solid = Blend { closest, partner: closest, partner_ranks: 0 };
        let base = self.colors[usize::from(closest)];
        let off = minus(pixel.map(f32::from), base);
        let error = squared_length(off);
        if error == 0.0 {
            return solid;
        }

        let first = usize::from(closest) * self.per_color;
        let mut best = (f32::INFINITY, f32::INFINITY, 0.0, closest);
        for &Partner { index, step, squared_step } in &self.nearest_others[first..first + self.per_color] {
            if squared_step == 0.0 {
                continue;
            }
            let share = (dot(off, step) / squared_step).min(1.0);
            if share <= 0.0 {
                continue;
            }
            let miss = squared_length(minus(off, step.map(|channel| channel * share)));
            let cost = miss + PATTERN_CONTRAST_WEIGHT * share * (1.0 - share) * squared_step;
            if cost < best.0 {
                best = (cost, miss, share, index);
            }
        }

        let (_, miss, share, partner) = best;
        if miss > LARGEST_MISS_A_BLEND_MAY_LEAVE * error {
            return solid;
        }
        let ranks = (BAYER_SIZE * BAYER_SIZE) as f32;
        let partner_ranks = (share * strength * ranks - 0.5).ceil().clamp(0.0, ranks) as u8;
        Blend { closest, partner, partner_ranks }
    }
}

fn minus(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn squared_length(a: [f32; 3]) -> f32 {
    dot(a, a)
}

struct RememberedBlends {
    slots: Vec<(u32, Blend)>,
}

impl RememberedBlends {
    const SLOT_BITS: u32 = 16;
    const EMPTY: u32 = u32::MAX;

    fn new() -> Self {
        let empty = Blend { closest: 0, partner: 0, partner_ranks: 0 };
        Self { slots: vec![(Self::EMPTY, empty); 1 << Self::SLOT_BITS] }
    }

    fn of(&mut self, pixel: Rgb, find: impl FnOnce() -> Blend) -> Blend {
        let color = u32::from(pixel[0]) << 16 | u32::from(pixel[1]) << 8 | u32::from(pixel[2]);
        let slot = (color.wrapping_mul(0x9E37_79B1) >> (32 - Self::SLOT_BITS)) as usize;
        let remembered = &mut self.slots[slot];
        if remembered.0 != color {
            *remembered = (color, find());
        }
        remembered.1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gray(level: u8) -> Rgb {
        [level, level, level]
    }

    fn black_and_white() -> Palette {
        Palette::from_frames(&[&[gray(0), gray(255)]], 2).unwrap()
    }

    fn four_grays() -> Palette {
        Palette::from_frames(&[&[gray(0), gray(85), gray(170), gray(255)]], 4).unwrap()
    }

    fn shown_level(palette: &Palette, index: u8) -> f64 {
        f64::from(palette.colors()[usize::from(index)][0])
    }

    fn block_error(palette: &Palette, pixels: &[Rgb], indices: &[u8], width: usize) -> f64 {
        let blocks = width / 8;
        let error_of_block = |block: usize| {
            let columns = block * 8..block * 8 + 8;
            let in_block = |index: &usize| columns.contains(&(index % width));
            let average = |level: &dyn Fn(usize) -> f64| {
                let levels: Vec<f64> = (0..pixels.len()).filter(in_block).map(level).collect();
                levels.iter().sum::<f64>() / levels.len() as f64
            };
            let wanted = average(&|index| f64::from(pixels[index][0]));
            let shown = average(&|index| shown_level(palette, indices[index]));
            (wanted - shown).abs()
        };
        (0..blocks).map(error_of_block).sum::<f64>() / blocks as f64
    }

    #[test]
    fn zero_strength_is_plain_nearest_color() {
        let palette = four_grays();
        let pixels: Vec<Rgb> = (0..=255).map(gray).collect();
        assert_eq!(ordered_dither(&mut palette.lookup(), &pixels, 16, 0.0), Ok(palette.to_indices(&pixels)));
    }

    #[test]
    fn mid_gray_becomes_half_black_and_half_white() {
        let palette = black_and_white();
        let white = palette.nearest(gray(255));
        let indices = ordered_dither(&mut palette.lookup(), &[gray(128); 64], 8, 1.0).unwrap();
        let whites = indices.iter().filter(|&&index| index == white).count();
        assert!((30..=34).contains(&whites), "{whites} of 64 pixels are white");
    }

    #[test]
    fn a_darker_gray_gets_proportionally_fewer_white_pixels() {
        let palette = black_and_white();
        let white = palette.nearest(gray(255));
        let indices = ordered_dither(&mut palette.lookup(), &[gray(64); 64], 8, 1.0).unwrap();
        let whites = indices.iter().filter(|&&index| index == white).count();
        assert!((6..=18).contains(&whites), "{whites} of 64 pixels are white");
    }

    #[test]
    fn a_ramp_seen_from_a_distance_is_much_closer_to_the_original() {
        let palette = four_grays();
        let width = 256;
        let pixels: Vec<Rgb> = (0..width * 8).map(|index| gray((index % width) as u8)).collect();

        let plain = palette.to_indices(&pixels);
        let dithered = ordered_dither(&mut palette.lookup(), &pixels, width, 1.0).unwrap();

        let plain_error = block_error(&palette, &pixels, &plain, width);
        let dithered_error = block_error(&palette, &pixels, &dithered, width);
        assert!(
            dithered_error * 3.0 < plain_error,
            "plain {plain_error:.1}, dithered {dithered_error:.1}"
        );
    }

    #[test]
    fn pure_black_and_white_are_left_alone() {
        let palette = black_and_white();
        let pixels: Vec<Rgb> = (0..64).map(|index| gray(if index % 3 == 0 { 255 } else { 0 })).collect();
        assert_eq!(ordered_dither(&mut palette.lookup(), &pixels, 8, 1.0), Ok(palette.to_indices(&pixels)));
    }

    #[test]
    fn changing_one_pixel_changes_only_that_pixel() {
        let palette = four_grays();
        let before: Vec<Rgb> = (0..32 * 32).map(|index| gray((index % 251) as u8)).collect();
        let mut after = before.clone();
        after[500] = gray(255);

        let before = ordered_dither(&mut palette.lookup(), &before, 32, 1.0).unwrap();
        let after = ordered_dither(&mut palette.lookup(), &after, 32, 1.0).unwrap();
        let changed: Vec<usize> = (0..before.len()).filter(|&index| before[index] != after[index]).collect();
        assert!(changed.is_empty() || changed == [500], "{changed:?}");
    }

    #[test]
    fn a_color_between_two_hues_is_drawn_with_both_in_proportion() {
        let red = [255, 0, 0];
        let palette = Palette::from_frames(&[&[red, [0, 0, 255]]], 2).unwrap();
        let indices = ordered_dither(&mut palette.lookup(), &[[96, 0, 160]; 64], 8, 1.0).unwrap();
        let reds = indices.iter().filter(|&&index| index == palette.nearest(red)).count();
        assert!((22..=26).contains(&reds), "{reds} of 64 pixels are red");
    }

    #[test]
    fn a_flat_color_no_blend_comes_close_to_stays_one_solid_color() {
        let palette = black_and_white();
        let indices = ordered_dither(&mut palette.lookup(), &[[200, 0, 0]; 64], 8, 1.0).unwrap();
        assert!(indices.iter().all(|&index| index == indices[0]), "{indices:?}");
    }

    #[test]
    fn a_width_that_does_not_divide_the_pixels_is_rejected() {
        let palette = black_and_white();
        let pixels = [gray(0); 10];
        for width in [0, 3, 11] {
            assert_eq!(
                ordered_dither(&mut palette.lookup(), &pixels, width, 1.0),
                Err(DitherError::WidthDoesNotFit { width, pixels: 10 })
            );
        }
    }
}
