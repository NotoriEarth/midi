use gif_core::session::ScreenRect;
use gif_core::writer::Rgb;

use crate::CaptureError;

pub(crate) struct MonitorImage<'a> {
    pub bgra: &'a [u8],
    pub row_pitch: usize,
    pub width: u32,
    pub height: u32,
}

const BYTES_PER_PIXEL: usize = 4;

pub(crate) fn crop_to_rgb(
    image: &MonitorImage,
    region: ScreenRect,
) -> Result<Vec<Rgb>, CaptureError> {
    if region.width == 0 || region.height == 0 {
        return Err(CaptureError::EmptyRegion);
    }
    let outside = CaptureError::RegionOutsideMonitor {
        monitor_width: image.width,
        monitor_height: image.height,
    };
    let (Ok(left), Ok(top)) = (u32::try_from(region.x), u32::try_from(region.y)) else {
        return Err(outside);
    };
    let fits = |start: u32, length: u32, limit: u32| {
        start.checked_add(length).is_some_and(|end| end <= limit)
    };
    if !fits(left, region.width, image.width) || !fits(top, region.height, image.height) {
        return Err(outside);
    }

    let (left, width) = (left as usize, region.width as usize);
    let rows = top as usize..(top + region.height) as usize;
    let pixels = rows.flat_map(|row| {
        let start = row * image.row_pitch + left * BYTES_PER_PIXEL;
        let (row_pixels, _) =
            image.bgra[start..start + width * BYTES_PER_PIXEL].as_chunks::<BYTES_PER_PIXEL>();
        row_pixels
            .iter()
            .map(|&[blue, green, red, _alpha]| [red, green, blue])
    });
    Ok(pixels.collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gif_core::session::MonitorId;

    const PAD: u8 = 0xEE;

    #[rustfmt::skip]
    const THREE_BY_TWO: [u8; 32] = [
        10, 11, 12, 255,   20, 21, 22, 255,   30, 31, 32, 255,   PAD, PAD, PAD, PAD,
        40, 41, 42, 255,   50, 51, 52, 255,   60, 61, 62, 255,   PAD, PAD, PAD, PAD,
    ];

    fn image() -> MonitorImage<'static> {
        MonitorImage {
            bgra: &THREE_BY_TWO,
            row_pitch: 16,
            width: 3,
            height: 2,
        }
    }

    fn region(x: i32, y: i32, width: u32, height: u32) -> ScreenRect {
        ScreenRect {
            monitor: MonitorId(0),
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn blue_and_red_swap_places_and_alpha_is_dropped() {
        assert_eq!(
            crop_to_rgb(&image(), region(0, 0, 1, 1)),
            Ok(vec![[12, 11, 10]])
        );
    }

    #[test]
    fn the_whole_monitor_skips_the_padding_at_the_end_of_each_row() {
        let expected = vec![
            [12, 11, 10],
            [22, 21, 20],
            [32, 31, 30],
            [42, 41, 40],
            [52, 51, 50],
            [62, 61, 60],
        ];
        assert_eq!(crop_to_rgb(&image(), region(0, 0, 3, 2)), Ok(expected));
    }

    #[test]
    fn a_region_inside_the_monitor_is_cut_out() {
        assert_eq!(
            crop_to_rgb(&image(), region(1, 1, 2, 1)),
            Ok(vec![[52, 51, 50], [62, 61, 60]])
        );
    }

    #[test]
    fn a_region_that_leaves_the_monitor_is_rejected() {
        let outside = Err(CaptureError::RegionOutsideMonitor {
            monitor_width: 3,
            monitor_height: 2,
        });
        assert_eq!(crop_to_rgb(&image(), region(2, 0, 2, 1)), outside);
        assert_eq!(crop_to_rgb(&image(), region(0, 1, 1, 2)), outside);
        assert_eq!(crop_to_rgb(&image(), region(-1, 0, 2, 1)), outside);
        assert_eq!(crop_to_rgb(&image(), region(0, 0, u32::MAX, 1)), outside);
    }

    #[test]
    fn a_region_without_area_is_rejected() {
        assert_eq!(
            crop_to_rgb(&image(), region(0, 0, 0, 1)),
            Err(CaptureError::EmptyRegion)
        );
        assert_eq!(
            crop_to_rgb(&image(), region(0, 0, 1, 0)),
            Err(CaptureError::EmptyRegion)
        );
    }
}
