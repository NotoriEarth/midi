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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeKey {
    Digit(u8),
    Next,
    Erase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeSide {
    Width,
    Height,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SizeEntry {
    width: Option<u32>,
    height: Option<u32>,
    on_height: bool,
}

const LONGEST_SIDE_TYPED: u32 = 9999;

impl SizeEntry {
    pub fn edit(self, side: SizeSide) -> SizeEntry {
        match side {
            SizeSide::Width => SizeEntry { width: None, on_height: false, ..self },
            SizeSide::Height => SizeEntry { height: None, on_height: true, ..self },
        }
    }

    pub fn press(self, key: SizeKey) -> SizeEntry {
        let typing = if self.on_height { self.height } else { self.width };
        let typed = match key {
            SizeKey::Digit(digit) => {
                let longer = typing.unwrap_or(0) * 10 + u32::from(digit);
                if longer > LONGEST_SIDE_TYPED { typing } else { Some(longer) }
            }
            SizeKey::Next => return SizeEntry { on_height: true, ..self },
            SizeKey::Erase if typing.is_none() => return SizeEntry { on_height: false, ..self },
            SizeKey::Erase => typing.and_then(|value| (value >= 10).then_some(value / 10)),
        };
        if self.on_height { SizeEntry { height: typed, ..self } } else { SizeEntry { width: typed, ..self } }
    }

    pub fn label(&self, region: ScreenRect) -> String {
        let side = |typed: Option<u32>, now: u32, typing: bool| match (typing, typed) {
            (true, Some(value)) => format!("[{value}]"),
            (true, None) => "[ ]".to_string(),
            (false, typed) => typed.unwrap_or(now).to_string(),
        };
        let width = side(self.width, region.width, !self.on_height);
        let height = side(self.height, region.height, self.on_height);
        format!("{width}×{height}")
    }

    pub fn apply(&self, region: ScreenRect, monitor_size: (u32, u32)) -> ScreenRect {
        let side = |typed: Option<u32>, now: u32, limit: u32| typed.unwrap_or(now).clamp(MIN_SIDE, limit);
        let (width, height) = (side(self.width, region.width, monitor_size.0), side(self.height, region.height, monitor_size.1));
        let start = |at: i32, side: u32, limit: u32| at.min((limit - side) as i32);
        ScreenRect { x: start(region.x, width, monitor_size.0), y: start(region.y, height, monitor_size.1), width, height, ..region }
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

    fn typed(keys: &str) -> SizeEntry {
        keys.chars().fold(SizeEntry::default(), |entry, key| {
            entry.press(match key {
                'x' => SizeKey::Next,
                '<' => SizeKey::Erase,
                digit => SizeKey::Digit(digit.to_digit(10).unwrap() as u8),
            })
        })
    }

    #[test]
    fn a_typed_width_and_height_become_the_frame_size_where_it_is() {
        let start = frame(100, 200, 500, 300);
        assert_eq!(typed("1280x720").apply(start, SIZE), frame(100, 200, 1280, 720));
    }

    #[test]
    fn the_field_being_typed_is_shown_in_brackets() {
        let start = frame(100, 200, 500, 300);
        assert_eq!(typed("12").label(start), "[12]×300");
        assert_eq!(typed("1280x").label(start), "1280×[ ]");
        assert_eq!(typed("1280x7").label(start), "1280×[7]");
    }

    #[test]
    fn erasing_takes_back_the_last_digit_and_then_returns_to_the_width() {
        let start = frame(100, 200, 500, 300);
        assert_eq!(typed("128<").label(start), "[12]×300");
        assert_eq!(typed("12x<").label(start), "[12]×300");
        assert_eq!(typed("12x7<<9").label(start), "[129]×300");
    }

    #[test]
    fn a_field_left_empty_keeps_that_side() {
        let start = frame(100, 200, 500, 300);
        assert_eq!(typed("800").apply(start, SIZE), frame(100, 200, 800, 300));
        assert_eq!(typed("x400").apply(start, SIZE), frame(100, 200, 500, 400));
    }

    #[test]
    fn a_size_that_does_not_fit_from_here_moves_the_frame_and_stops_at_the_monitor() {
        let start = frame(1500, 900, 300, 100);
        assert_eq!(typed("640x360").apply(start, SIZE), frame(1920 - 640, 1080 - 360, 640, 360));
        assert_eq!(typed("5000x5000").apply(start, SIZE), frame(0, 0, 1920, 1080));
        assert_eq!(typed("3x3").apply(start, SIZE), frame(1500, 900, MIN_SIDE, MIN_SIDE));
    }

    #[test]
    fn choosing_a_side_empties_it_for_typing_and_keeps_the_other() {
        let start = frame(100, 200, 640, 360);
        let height = SizeEntry::default().edit(SizeSide::Height);
        assert_eq!(height.label(start), "640×[ ]");
        let typed_height = "300".chars().fold(height, |entry, digit| entry.press(SizeKey::Digit(digit.to_digit(10).unwrap() as u8)));
        assert_eq!(typed_height.label(start), "640×[300]");
        assert_eq!(typed_height.apply(start, SIZE), frame(100, 200, 640, 300));

        let width_again = typed_height.edit(SizeSide::Width);
        assert_eq!(width_again.label(start), "[ ]×300");
        assert_eq!(width_again.press(SizeKey::Digit(8)).press(SizeKey::Digit(0)).press(SizeKey::Digit(0)).apply(start, SIZE), frame(100, 200, 800, 300));
        assert_eq!(typed("1280").edit(SizeSide::Width).label(start), "[ ]×360", "choosing the side being typed starts it over");
    }

    #[test]
    fn no_side_takes_more_than_four_digits() {
        let start = frame(0, 0, 500, 300);
        assert_eq!(typed("123456").label(start), "[1234]×300");
    }
}
