use crate::session::{MonitorId, ScreenRect};

pub const MIN_SIDE: u32 = 16;

pub type Point = (i32, i32);

pub fn drag_region(monitor: MonitorId, monitor_size: (u32, u32), from: Point, to: Point) -> Option<ScreenRect> {
    let (width, height) = monitor_size;
    let (left, right) = span_inside(from.0, to.0, width);
    let (top, bottom) = span_inside(from.1, to.1, height);

    let region = ScreenRect { monitor, x: left as i32, y: top as i32, width: right - left, height: bottom - top };
    (region.width >= MIN_SIDE && region.height >= MIN_SIDE).then_some(region)
}

fn span_inside(a: i32, b: i32, limit: u32) -> (u32, u32) {
    let inside = |value: i32| (value.max(0) as u32).min(limit);
    (inside(a.min(b)), inside(a.max(b)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MONITOR: MonitorId = MonitorId(0);
    const SIZE: (u32, u32) = (1920, 1080);

    fn rect(x: i32, y: i32, width: u32, height: u32) -> Option<ScreenRect> {
        Some(ScreenRect { monitor: MONITOR, x, y, width, height })
    }

    #[test]
    fn a_drag_down_and_right_starts_at_the_press_point() {
        assert_eq!(drag_region(MONITOR, SIZE, (100, 200), (400, 500)), rect(100, 200, 300, 300));
    }

    #[test]
    fn a_drag_in_any_other_direction_gives_the_same_rectangle() {
        let expected = rect(100, 200, 300, 300);
        assert_eq!(drag_region(MONITOR, SIZE, (400, 500), (100, 200)), expected);
        assert_eq!(drag_region(MONITOR, SIZE, (400, 200), (100, 500)), expected);
        assert_eq!(drag_region(MONITOR, SIZE, (100, 500), (400, 200)), expected);
    }

    #[test]
    fn a_drag_that_leaves_the_monitor_is_cut_at_its_edge() {
        assert_eq!(drag_region(MONITOR, SIZE, (1800, 1000), (2500, 1300)), rect(1800, 1000, 120, 80));
        assert_eq!(drag_region(MONITOR, SIZE, (50, 60), (-30, -40)), rect(0, 0, 50, 60));
    }

    #[test]
    fn the_whole_monitor_can_be_selected() {
        assert_eq!(drag_region(MONITOR, SIZE, (0, 0), (1920, 1080)), rect(0, 0, 1920, 1080));
    }

    #[test]
    fn a_click_or_a_tiny_drag_selects_nothing() {
        assert_eq!(drag_region(MONITOR, SIZE, (300, 300), (300, 300)), None);
        assert_eq!(drag_region(MONITOR, SIZE, (300, 300), (315, 600)), None);
        assert_eq!(drag_region(MONITOR, SIZE, (300, 300), (600, 315)), None);
        assert_eq!(drag_region(MONITOR, SIZE, (300, 300), (316, 316)), rect(300, 300, 16, 16));
    }

    #[test]
    fn a_drag_that_is_only_big_enough_outside_the_monitor_selects_nothing() {
        assert_eq!(drag_region(MONITOR, SIZE, (1910, 500), (2400, 900)), None);
    }

    #[test]
    fn the_rectangle_keeps_the_monitor_it_was_dragged_on() {
        let region = drag_region(MonitorId(1), SIZE, (0, 0), (100, 100)).unwrap();
        assert_eq!(region.monitor, MonitorId(1));
    }
}
