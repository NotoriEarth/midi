use gif_core::session::ScreenRect;
use gif_core::writer::Rgb;

use crate::CaptureError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Rotation {
    None,
    Clockwise90,
    Half,
    Clockwise270,
}

pub(crate) struct MonitorImage<'a> {
    pub bgra: &'a [u8],
    pub row_pitch: usize,
    pub width: u32,
    pub height: u32,
    pub rotation: Rotation,
}

impl MonitorImage<'_> {
    pub fn desktop_size(&self) -> (u32, u32) {
        match self.rotation {
            Rotation::Clockwise90 | Rotation::Clockwise270 => (self.height, self.width),
            Rotation::None | Rotation::Half => (self.width, self.height),
        }
    }

    fn texture_pixel(&self, x: usize, y: usize) -> (usize, usize) {
        let (width, height) = (self.width as usize, self.height as usize);
        match self.rotation {
            Rotation::None => (x, y),
            Rotation::Clockwise90 => (y, height - 1 - x),
            Rotation::Half => (width - 1 - x, height - 1 - y),
            Rotation::Clockwise270 => (width - 1 - y, x),
        }
    }
}

const BYTES_PER_PIXEL: usize = 4;

pub(crate) fn crop_to_rgb(image: &MonitorImage, region: ScreenRect) -> Result<Vec<Rgb>, CaptureError> {
    if region.width == 0 || region.height == 0 {
        return Err(CaptureError::EmptyRegion);
    }
    let (desktop_width, desktop_height) = image.desktop_size();
    let outside = CaptureError::RegionOutsideMonitor { monitor_width: desktop_width, monitor_height: desktop_height };
    let (Ok(left), Ok(top)) = (u32::try_from(region.x), u32::try_from(region.y)) else {
        return Err(outside);
    };
    let fits = |start: u32, length: u32, limit: u32| start.checked_add(length).is_some_and(|end| end <= limit);
    if !fits(left, region.width, desktop_width) || !fits(top, region.height, desktop_height) {
        return Err(outside);
    }
    if image.rotation != Rotation::None {
        return Ok(turned_to_rgb(image, region));
    }

    let (left, width) = (left as usize, region.width as usize);
    let rows = top as usize..(top + region.height) as usize;
    let pixels = rows.flat_map(|row| {
        let start = row * image.row_pitch + left * BYTES_PER_PIXEL;
        let (row_pixels, _) = image.bgra[start..start + width * BYTES_PER_PIXEL].as_chunks::<BYTES_PER_PIXEL>();
        row_pixels.iter().map(|&[blue, green, red, _alpha]| [red, green, blue])
    });
    Ok(pixels.collect())
}

fn turned_to_rgb(image: &MonitorImage, region: ScreenRect) -> Vec<Rgb> {
    let columns = region.x as usize..(region.x as usize + region.width as usize);
    let rows = region.y as usize..(region.y as usize + region.height as usize);
    rows.flat_map(|y| columns.clone().map(move |x| (x, y)))
        .map(|(x, y)| {
            let (u, v) = image.texture_pixel(x, y);
            let start = v * image.row_pitch + u * BYTES_PER_PIXEL;
            let [blue, green, red, _alpha] = image.bgra[start..start + BYTES_PER_PIXEL] else {
                unreachable!("four bytes were sliced")
            };
            [red, green, blue]
        })
        .collect()
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
        turned(Rotation::None)
    }

    fn turned(rotation: Rotation) -> MonitorImage<'static> {
        MonitorImage { bgra: &THREE_BY_TWO, row_pitch: 16, width: 3, height: 2, rotation }
    }

    const A: Rgb = [12, 11, 10];
    const B: Rgb = [22, 21, 20];
    const C: Rgb = [32, 31, 30];
    const D: Rgb = [42, 41, 40];
    const E: Rgb = [52, 51, 50];
    const F: Rgb = [62, 61, 60];

    #[test]
    fn a_monitor_turned_a_quarter_clockwise_shows_the_left_column_bottom_up_as_its_top_row() {
        let turned = turned(Rotation::Clockwise90);
        assert_eq!(turned.desktop_size(), (2, 3));
        assert_eq!(crop_to_rgb(&turned, region(0, 0, 2, 3)), Ok(vec![D, A, E, B, F, C]));
        assert_eq!(crop_to_rgb(&turned, region(1, 1, 1, 2)), Ok(vec![B, C]));
    }

    #[test]
    fn a_monitor_turned_three_quarters_shows_the_right_column_top_down_as_its_top_row() {
        let turned = turned(Rotation::Clockwise270);
        assert_eq!(turned.desktop_size(), (2, 3));
        assert_eq!(crop_to_rgb(&turned, region(0, 0, 2, 3)), Ok(vec![C, F, B, E, A, D]));
        assert_eq!(crop_to_rgb(&turned, region(0, 2, 2, 1)), Ok(vec![A, D]));
    }

    #[test]
    fn a_monitor_upside_down_shows_everything_reversed() {
        let turned = turned(Rotation::Half);
        assert_eq!(turned.desktop_size(), (3, 2));
        assert_eq!(crop_to_rgb(&turned, region(0, 0, 3, 2)), Ok(vec![F, E, D, C, B, A]));
    }

    #[test]
    fn a_region_is_checked_against_the_turned_size() {
        let outside = Err(CaptureError::RegionOutsideMonitor { monitor_width: 2, monitor_height: 3 });
        assert_eq!(crop_to_rgb(&turned(Rotation::Clockwise90), region(0, 0, 3, 1)), outside);
        assert!(crop_to_rgb(&turned(Rotation::Clockwise90), region(0, 0, 1, 3)).is_ok());
    }

    fn region(x: i32, y: i32, width: u32, height: u32) -> ScreenRect {
        ScreenRect { monitor: MonitorId(0), x, y, width, height }
    }

    #[test]
    fn blue_and_red_swap_places_and_alpha_is_dropped() {
        assert_eq!(crop_to_rgb(&image(), region(0, 0, 1, 1)), Ok(vec![[12, 11, 10]]));
    }

    #[test]
    fn the_whole_monitor_skips_the_padding_at_the_end_of_each_row() {
        let expected = vec![[12, 11, 10], [22, 21, 20], [32, 31, 30], [42, 41, 40], [52, 51, 50], [62, 61, 60]];
        assert_eq!(crop_to_rgb(&image(), region(0, 0, 3, 2)), Ok(expected));
    }

    #[test]
    fn a_region_inside_the_monitor_is_cut_out() {
        assert_eq!(crop_to_rgb(&image(), region(1, 1, 2, 1)), Ok(vec![[52, 51, 50], [62, 61, 60]]));
    }

    #[test]
    fn a_region_that_leaves_the_monitor_is_rejected() {
        let outside = Err(CaptureError::RegionOutsideMonitor { monitor_width: 3, monitor_height: 2 });
        assert_eq!(crop_to_rgb(&image(), region(2, 0, 2, 1)), outside);
        assert_eq!(crop_to_rgb(&image(), region(0, 1, 1, 2)), outside);
        assert_eq!(crop_to_rgb(&image(), region(-1, 0, 2, 1)), outside);
        assert_eq!(crop_to_rgb(&image(), region(0, 0, u32::MAX, 1)), outside);
    }

    #[test]
    fn a_region_without_area_is_rejected() {
        assert_eq!(crop_to_rgb(&image(), region(0, 0, 0, 1)), Err(CaptureError::EmptyRegion));
        assert_eq!(crop_to_rgb(&image(), region(0, 0, 1, 0)), Err(CaptureError::EmptyRegion));
    }
}
