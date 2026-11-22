use crate::writer::Rgb;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeError {
    PixelCount { expected: usize, actual: usize },
    EmptyTarget,
    LargerThanSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputSize {
    Original,
    FitWithin { width: u32, height: u32 },
}

impl OutputSize {
    pub fn check(self) -> Result<(), ResizeError> {
        match self {
            OutputSize::FitWithin { width, height } if width == 0 || height == 0 => Err(ResizeError::EmptyTarget),
            _ => Ok(()),
        }
    }

    pub fn for_source(self, width: u32, height: u32) -> Result<(u32, u32), ResizeError> {
        self.check()?;
        let OutputSize::FitWithin { width: box_width, height: box_height } = self else {
            return Ok((width, height));
        };
        if width <= box_width && height <= box_height {
            return Ok((width, height));
        }

        let (wide, tall) = (u64::from(width), u64::from(height));
        let (box_wide, box_tall) = (u64::from(box_width), u64::from(box_height));
        let rounded = |side: u64, times: u64, over: u64| (((side * times + over / 2) / over) as u32).max(1);
        if wide * box_tall >= tall * box_wide {
            Ok((box_width, rounded(tall, box_wide, wide)))
        } else {
            Ok((rounded(wide, box_tall, tall), box_height))
        }
    }
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

    fn within(width: u32, height: u32) -> OutputSize {
        OutputSize::FitWithin { width, height }
    }

    #[test]
    fn the_original_size_is_the_size_of_the_region() {
        assert_eq!(OutputSize::Original.for_source(1920, 1080), Ok((1920, 1080)));
    }

    #[test]
    fn a_box_with_the_same_shape_is_filled_exactly() {
        assert_eq!(within(480, 270).for_source(1920, 1080), Ok((480, 270)));
    }

    #[test]
    fn a_region_wider_than_the_box_touches_its_left_and_right() {
        assert_eq!(within(480, 270).for_source(1000, 250), Ok((480, 120)));
    }

    #[test]
    fn a_region_taller_than_the_box_touches_its_top_and_bottom() {
        assert_eq!(within(480, 270).for_source(800, 500), Ok((432, 270)));
        assert_eq!(within(480, 270).for_source(500, 800), Ok((169, 270)));
    }

    #[test]
    fn a_region_that_already_fits_is_not_enlarged() {
        assert_eq!(within(480, 270).for_source(300, 200), Ok((300, 200)));
        assert_eq!(within(480, 270).for_source(480, 100), Ok((480, 100)));
    }

    #[test]
    fn a_thin_region_never_shrinks_to_nothing() {
        assert_eq!(within(100, 100).for_source(1000, 2), Ok((100, 1)));
        assert_eq!(within(100, 100).for_source(2, 1000), Ok((1, 100)));
    }

    #[test]
    fn a_box_without_area_is_rejected() {
        assert_eq!(within(0, 270).check(), Err(ResizeError::EmptyTarget));
        assert_eq!(within(480, 0).for_source(800, 500), Err(ResizeError::EmptyTarget));
        assert_eq!(OutputSize::Original.check(), Ok(()));
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
