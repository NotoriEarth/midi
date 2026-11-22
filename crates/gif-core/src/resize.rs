use crate::writer::Rgb;

pub const MIN_SCALE_PERCENT: u32 = 10;
pub const MAX_SCALE_PERCENT: u32 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeError {
    ScalePercent(u32),
    PixelCount { expected: usize, actual: usize },
    EmptyTarget,
    LargerThanSource,
}

pub fn scaled_size(width: u32, height: u32, percent: u32) -> Result<(u32, u32), ResizeError> {
    if !(MIN_SCALE_PERCENT..=MAX_SCALE_PERCENT).contains(&percent) {
        return Err(ResizeError::ScalePercent(percent));
    }
    let scaled = |side: u32| ((u64::from(side) * u64::from(percent) / 100) as u32).max(1);
    Ok((scaled(width), scaled(height)))
}

pub fn downscale(pixels: &[Rgb], from: (u32, u32), to: (u32, u32)) -> Result<Vec<Rgb>, ResizeError> {
    let (from_width, from_height) = (from.0 as usize, from.1 as usize);
    let (to_width, to_height) = (to.0 as usize, to.1 as usize);
    if pixels.len() != from_width * from_height {
        return Err(ResizeError::PixelCount { expected: from_width * from_height, actual: pixels.len() });
    }
    if to_width == 0 || to_height == 0 {
        return Err(ResizeError::EmptyTarget);
    }
    if to_width > from_width || to_height > from_height {
        return Err(ResizeError::LargerThanSource);
    }
    if from == to {
        return Ok(pixels.to_vec());
    }

    let columns = source_spans(from_width, to_width);
    let rows = source_spans(from_height, to_height);
    let mut scaled = Vec::with_capacity(to_width * to_height);
    for (top, bottom) in &rows {
        for (left, right) in &columns {
            let mut sums = [0u32; 3];
            for row in *top..*bottom {
                for pixel in &pixels[row * from_width + left..row * from_width + right] {
                    for (sum, &channel) in sums.iter_mut().zip(pixel) {
                        *sum += u32::from(channel);
                    }
                }
            }
            let count = ((bottom - top) * (right - left)) as u32;
            scaled.push(sums.map(|sum| ((sum + count / 2) / count) as u8));
        }
    }
    Ok(scaled)
}

fn source_spans(from: usize, to: usize) -> Vec<(usize, usize)> {
    (0..to).map(|index| (index * from / to, (index + 1) * from / to)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gray(level: u8) -> Rgb {
        [level, level, level]
    }

    #[test]
    fn full_scale_keeps_the_size() {
        assert_eq!(scaled_size(1920, 1080, 100), Ok((1920, 1080)));
    }

    #[test]
    fn half_scale_halves_both_sides() {
        assert_eq!(scaled_size(1920, 1080, 50), Ok((960, 540)));
        assert_eq!(scaled_size(480, 270, 25), Ok((120, 67)));
    }

    #[test]
    fn a_tiny_region_never_shrinks_to_nothing() {
        assert_eq!(scaled_size(16, 3, 10), Ok((1, 1)));
    }

    #[test]
    fn a_scale_outside_ten_to_a_hundred_percent_is_rejected() {
        assert_eq!(scaled_size(100, 100, 9), Err(ResizeError::ScalePercent(9)));
        assert_eq!(scaled_size(100, 100, 101), Err(ResizeError::ScalePercent(101)));
    }

    #[test]
    fn the_same_size_gives_the_same_pixels() {
        let pixels = [gray(1), gray(2), gray(3), gray(4), gray(5), gray(6)];
        assert_eq!(downscale(&pixels, (3, 2), (3, 2)), Ok(pixels.to_vec()));
    }

    #[test]
    fn halving_averages_each_two_by_two_block() {
        #[rustfmt::skip]
        let pixels = [
            gray(10), gray(20),   gray(100), gray(100),
            gray(30), gray(40),   gray(100), gray(104),
        ];
        assert_eq!(downscale(&pixels, (4, 2), (2, 1)), Ok(vec![gray(25), gray(101)]));
    }

    #[test]
    fn each_channel_is_averaged_on_its_own() {
        let pixels = [[255, 0, 10], [1, 0, 20]];
        assert_eq!(downscale(&pixels, (2, 1), (1, 1)), Ok(vec![[128, 0, 15]]));
    }

    #[test]
    fn an_average_that_ends_in_a_half_rounds_up() {
        assert_eq!(downscale(&[gray(0), gray(1)], (2, 1), (1, 1)), Ok(vec![gray(1)]));
        assert_eq!(downscale(&[gray(0), gray(0), gray(0), gray(1)], (4, 1), (1, 1)), Ok(vec![gray(0)]));
    }

    #[test]
    fn a_size_that_does_not_divide_evenly_still_uses_every_source_pixel_once() {
        let pixels = [gray(10), gray(20), gray(40)];
        assert_eq!(downscale(&pixels, (3, 1), (2, 1)), Ok(vec![gray(10), gray(30)]));
    }

    #[test]
    fn rows_shrink_the_same_way_as_columns() {
        let pixels = [gray(10), gray(20), gray(40)];
        assert_eq!(downscale(&pixels, (1, 3), (1, 2)), Ok(vec![gray(10), gray(30)]));
    }

    #[test]
    fn input_that_does_not_fit_is_rejected() {
        let pixels = [gray(0); 6];
        assert_eq!(downscale(&pixels, (4, 2), (2, 1)), Err(ResizeError::PixelCount { expected: 8, actual: 6 }));
        assert_eq!(downscale(&pixels, (3, 2), (0, 1)), Err(ResizeError::EmptyTarget));
        assert_eq!(downscale(&pixels, (3, 2), (4, 2)), Err(ResizeError::LargerThanSource));
    }
}
