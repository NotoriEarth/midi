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

pub const DEFAULT_FRAME: (u32, u32) = (640, 360);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nudge {
    Move,
    Grow,
}

pub fn frame_start(last: Option<ScreenRect>, monitor: MonitorId, monitor_size: (u32, u32)) -> ScreenRect {
    let (width, height) = monitor_size;
    let side = |wanted: u32, limit: u32| wanted.max(MIN_SIDE).min(limit);
    let start = |wanted: i32, side: u32, limit: u32| wanted.clamp(0, (limit - side) as i32);
    match last.filter(|last| last.monitor == monitor) {
        Some(last) => {
            let (w, h) = (side(last.width, width), side(last.height, height));
            ScreenRect { monitor, x: start(last.x, w, width), y: start(last.y, h, height), width: w, height: h }
        }
        None => {
            let (w, h) = (side(DEFAULT_FRAME.0, width), side(DEFAULT_FRAME.1, height));
            ScreenRect { monitor, x: ((width - w) / 2) as i32, y: ((height - h) / 2) as i32, width: w, height: h }
        }
    }
}

pub fn nudge(region: ScreenRect, monitor_size: (u32, u32), (dx, dy): Point, how: Nudge) -> ScreenRect {
    let (width, height) = (monitor_size.0 as i32, monitor_size.1 as i32);
    let (w, h) = (region.width as i32, region.height as i32);
    match how {
        Nudge::Move => ScreenRect { x: (region.x + dx).clamp(0, width - w), y: (region.y + dy).clamp(0, height - h), ..region },
        Nudge::Grow => {
            let grown = |side: i32, by: i32, room: i32| (side + by).min(room).max(MIN_SIDE as i32) as u32;
            ScreenRect { width: grown(w, dx, width - region.x), height: grown(h, dy, height - region.y), ..region }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(x: i32, y: i32, width: u32, height: u32) -> ScreenRect {
        ScreenRect { monitor: MONITOR, x, y, width, height }
    }

    #[test]
    fn the_frame_starts_where_the_last_recording_was() {
        let last = frame(100, 200, 500, 300);
        assert_eq!(frame_start(Some(last), MONITOR, SIZE), last);
    }

    #[test]
    fn without_a_last_recording_the_frame_starts_at_a_default_size_in_the_middle() {
        assert_eq!(frame_start(None, MONITOR, SIZE), frame((1920 - 640) / 2, (1080 - 360) / 2, 640, 360));
    }

    #[test]
    fn a_last_recording_on_another_monitor_is_not_reused() {
        let elsewhere = ScreenRect { monitor: MonitorId(1), ..frame(100, 200, 500, 300) };
        assert_eq!(frame_start(Some(elsewhere), MONITOR, SIZE), frame_start(None, MONITOR, SIZE));
    }

    #[test]
    fn a_last_recording_that_no_longer_fits_is_pulled_back_onto_the_monitor() {
        assert_eq!(frame_start(Some(frame(1700, 900, 500, 300)), MONITOR, SIZE), frame(1420, 780, 500, 300));
        assert_eq!(frame_start(Some(frame(-50, -10, 3000, 2000)), MONITOR, SIZE), frame(0, 0, 1920, 1080));
        assert_eq!(frame_start(Some(frame(10, 10, 4, 4)), MONITOR, SIZE), frame(10, 10, MIN_SIDE, MIN_SIDE));
    }

    #[test]
    fn a_monitor_smaller_than_the_default_gets_a_frame_that_fits() {
        assert_eq!(frame_start(None, MONITOR, (600, 300)), frame(0, 0, 600, 300));
    }

    #[test]
    fn moving_shifts_the_frame_and_stops_at_the_monitor_edges() {
        let start = frame(100, 200, 500, 300);
        assert_eq!(nudge(start, SIZE, (1, 0), Nudge::Move), frame(101, 200, 500, 300));
        assert_eq!(nudge(start, SIZE, (0, -10), Nudge::Move), frame(100, 190, 500, 300));
        assert_eq!(nudge(start, SIZE, (-500, 0), Nudge::Move), frame(0, 200, 500, 300));
        assert_eq!(nudge(start, SIZE, (5000, 5000), Nudge::Move), frame(1420, 780, 500, 300));
    }

    #[test]
    fn growing_moves_the_right_and_bottom_edges_and_keeps_the_size_sensible() {
        let start = frame(100, 200, 500, 300);
        assert_eq!(nudge(start, SIZE, (10, 0), Nudge::Grow), frame(100, 200, 510, 300));
        assert_eq!(nudge(start, SIZE, (0, -1), Nudge::Grow), frame(100, 200, 500, 299));
        assert_eq!(nudge(start, SIZE, (-1000, -1000), Nudge::Grow), frame(100, 200, MIN_SIDE, MIN_SIDE));
        assert_eq!(nudge(start, SIZE, (5000, 5000), Nudge::Grow), frame(100, 200, 1820, 880));
    }

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
