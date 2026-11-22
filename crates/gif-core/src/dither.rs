use crate::palette::Palette;
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
    palette: &Palette,
    pixels: &[Rgb],
    width: usize,
    strength: f32,
) -> Result<Vec<u8>, DitherError> {
    if width == 0 || !pixels.len().is_multiple_of(width) {
        return Err(DitherError::WidthDoesNotFit {
            width,
            pixels: pixels.len(),
        });
    }

    let reach = gap_between_palette_colors(palette) * strength;
    let indices = pixels.iter().enumerate().map(|(index, &pixel)| {
        let nudge = position_threshold(index % width, index / width) * reach;
        palette.nearest(pixel.map(|channel| (f32::from(channel) + nudge).clamp(0.0, 255.0) as u8))
    });
    Ok(indices.collect())
}

fn position_threshold(x: usize, y: usize) -> f32 {
    let rank = f32::from(BAYER[y % BAYER_SIZE][x % BAYER_SIZE]);
    let cells = (BAYER_SIZE * BAYER_SIZE) as f32;
    (rank + 0.5) / cells - 0.5
}

fn gap_between_palette_colors(palette: &Palette) -> f32 {
    let colors = palette.colors();
    let distance = |a: Rgb, b: Rgb| {
        let squared: f32 = (0..3)
            .map(|channel| f32::from(a[channel].abs_diff(b[channel])).powi(2))
            .sum();
        squared.sqrt()
    };
    let gap_to_closest_neighbor = |index: usize| {
        (0..colors.len())
            .filter(|&other| other != index)
            .map(|other| distance(colors[index], colors[other]))
            .fold(f32::INFINITY, f32::min)
    };

    if colors.len() < 2 {
        return 0.0;
    }
    let average_gap: f32 =
        (0..colors.len()).map(gap_to_closest_neighbor).sum::<f32>() / colors.len() as f32;
    let same_nudge_on_three_channels = 3f32.sqrt();
    average_gap / same_nudge_on_three_channels
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
        assert_eq!(
            ordered_dither(&palette, &pixels, 16, 0.0),
            Ok(palette.to_indices(&pixels))
        );
    }

    #[test]
    fn mid_gray_becomes_half_black_and_half_white() {
        let palette = black_and_white();
        let white = palette.nearest(gray(255));
        let indices = ordered_dither(&palette, &[gray(128); 64], 8, 1.0).unwrap();
        let whites = indices.iter().filter(|&&index| index == white).count();
        assert!(
            (30..=34).contains(&whites),
            "{whites} of 64 pixels are white"
        );
    }

    #[test]
    fn a_darker_gray_gets_proportionally_fewer_white_pixels() {
        let palette = black_and_white();
        let white = palette.nearest(gray(255));
        let indices = ordered_dither(&palette, &[gray(64); 64], 8, 1.0).unwrap();
        let whites = indices.iter().filter(|&&index| index == white).count();
        assert!(
            (6..=18).contains(&whites),
            "{whites} of 64 pixels are white"
        );
    }

    #[test]
    fn a_ramp_seen_from_a_distance_is_much_closer_to_the_original() {
        let palette = four_grays();
        let width = 256;
        let pixels: Vec<Rgb> = (0..width * 8)
            .map(|index| gray((index % width) as u8))
            .collect();

        let plain = palette.to_indices(&pixels);
        let dithered = ordered_dither(&palette, &pixels, width, 1.0).unwrap();

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
        let pixels: Vec<Rgb> = (0..64)
            .map(|index| gray(if index % 3 == 0 { 255 } else { 0 }))
            .collect();
        assert_eq!(
            ordered_dither(&palette, &pixels, 8, 1.0),
            Ok(palette.to_indices(&pixels))
        );
    }

    #[test]
    fn changing_one_pixel_changes_only_that_pixel() {
        let palette = four_grays();
        let before: Vec<Rgb> = (0..32 * 32)
            .map(|index| gray((index % 251) as u8))
            .collect();
        let mut after = before.clone();
        after[500] = gray(255);

        let before = ordered_dither(&palette, &before, 32, 1.0).unwrap();
        let after = ordered_dither(&palette, &after, 32, 1.0).unwrap();
        let changed: Vec<usize> = (0..before.len())
            .filter(|&index| before[index] != after[index])
            .collect();
        assert!(changed.is_empty() || changed == [500], "{changed:?}");
    }

    #[test]
    fn a_width_that_does_not_divide_the_pixels_is_rejected() {
        let palette = black_and_white();
        let pixels = [gray(0); 10];
        for width in [0, 3, 11] {
            assert_eq!(
                ordered_dither(&palette, &pixels, width, 1.0),
                Err(DitherError::WidthDoesNotFit { width, pixels: 10 })
            );
        }
    }
}
