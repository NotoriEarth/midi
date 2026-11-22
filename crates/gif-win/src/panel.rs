use std::cell::RefCell;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Once;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use gif_core::session::ScreenRect;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CLEARTYPE_QUALITY, CreateFontIndirectW, CreateSolidBrush, DEFAULT_CHARSET, DT_CENTER, DT_SINGLELINE,
    DT_VCENTER, DeleteObject, DrawTextW, EndPaint, FONT_CHARSET, FillRect, HDC, HFONT, InvalidateRect, LOGFONTW,
    PAINTSTRUCT, SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow, SetThreadDpiAwarenessContext};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, IDC_ARROW, KillTimer, LoadCursorW,
    MA_NOACTIVATE, MSG, PostMessageW, PostQuitMessage, RegisterClassExW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE,
    SWP_NOZORDER, SetTimer, SetWindowDisplayAffinity, SetWindowPos, ShowWindow, TranslateMessage,
    WDA_EXCLUDEFROMCAPTURE, WM_CLOSE, WM_DESTROY, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_PAINT, WM_TIMER, WNDCLASSEXW,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{PCWSTR, w};

use crate::CaptureError;
use crate::border::THICKNESS;
use crate::capture::{monitor_bounds, system};

const CLASS_NAME: PCWSTR = w!("midi-recording-panel");
pub(crate) const BASE_SIZE: (i32, i32) = (184, 32);
pub(crate) const BASE_DPI: u32 = 96;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelButton {
    Stop,
    Cancel,
}

pub(crate) fn stop_width(height: i32) -> i32 {
    height * 13 / 8
}

pub(crate) fn button_at(x: i32, (width, height): (i32, i32)) -> Option<PanelButton> {
    match x {
        x if x < 0 || x >= width => None,
        x if x >= width - height => Some(PanelButton::Cancel),
        x if x >= width - height - stop_width(height) => Some(PanelButton::Stop),
        _ => None,
    }
}

fn remaining_label(left: Duration) -> String {
    format!("● {}초 남음", left.as_millis().div_ceil(1000))
}

pub(crate) fn panel_place(region: RECT, monitor: RECT, (width, height): (i32, i32), gap: i32) -> POINT {
    let on_monitor = |x: i32| x.min(monitor.right - width).max(monitor.left);
    let x = on_monitor(region.right - width);
    let below = region.bottom + gap;
    if below + height <= monitor.bottom {
        return POINT { x, y: below };
    }
    let above = region.top - gap - height;
    if above >= monitor.top {
        return POINT { x, y: above };
    }
    POINT { x: on_monitor((region.right - width - gap).max(region.left)), y: (region.bottom - height - gap).max(region.top) }
}

pub(crate) const PANEL_GRAY: COLORREF = COLORREF(0x0020_2020);
const BUTTON_GRAY: COLORREF = COLORREF(0x0048_4848);
pub(crate) const RECORDING_RED: COLORREF = COLORREF(0x0039_35E5);
const WHITE: COLORREF = COLORREF(0x00FF_FFFF);
const TICK: usize = 1;
const TICK_MS: u32 = 200;

struct PanelState {
    started: Instant,
    limit: Duration,
    size: (i32, i32),
    font: HFONT,
    pressed: Sender<PanelButton>,
}

thread_local! {
    static PANEL: RefCell<Option<PanelState>> = const { RefCell::new(None) };
}

#[derive(Debug)]
pub struct RecordingPanel {
    window: isize,
    pressed: Receiver<PanelButton>,
    thread: Option<JoinHandle<()>>,
}

impl RecordingPanel {
    pub fn show(region: ScreenRect, limit: Duration) -> Result<RecordingPanel, CaptureError> {
        let monitor = monitor_bounds(region.monitor)?;
        let on_screen = RECT {
            left: monitor.left + region.x,
            top: monitor.top + region.y,
            right: monitor.left + region.x + region.width as i32,
            bottom: monitor.top + region.y + region.height as i32,
        };
        let (pressed_here, pressed) = mpsc::channel();
        let (told, shown) = mpsc::channel();
        let thread = thread::spawn(move || {
            unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
            let window = create_window(on_screen, monitor, limit, pressed_here);
            let created = window.is_ok();
            let _ = told.send(window.map(|window| window.0 as isize));
            if created {
                pump_messages();
            }
        });
        match shown.recv() {
            Ok(Ok(window)) => Ok(RecordingPanel { window, pressed, thread: Some(thread) }),
            Ok(Err(problem)) => {
                let _ = thread.join();
                Err(problem)
            }
            Err(_) => Err(CaptureError::System("the panel thread ended before showing the panel".to_string())),
        }
    }

    pub fn pressed(&self) -> Option<PanelButton> {
        self.pressed.try_recv().ok()
    }
}

impl Drop for RecordingPanel {
    fn drop(&mut self) {
        let _ = unsafe { PostMessageW(Some(HWND(self.window as _)), WM_CLOSE, WPARAM(0), LPARAM(0)) };
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn create_window(region: RECT, monitor: RECT, limit: Duration, pressed: Sender<PanelButton>) -> Result<HWND, CaptureError> {
    let module = unsafe { GetModuleHandleW(None) }.map_err(system)?;
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| {
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(panel_proc),
            hInstance: module.into(),
            hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default(),
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        unsafe { RegisterClassExW(&class) };
    });

    unsafe {
        let window = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            CLASS_NAME,
            w!("midi"),
            WS_POPUP,
            monitor.left,
            monitor.top,
            1,
            1,
            None,
            None,
            Some(module.into()),
            None,
        )
        .map_err(system)?;

        let dpi = GetDpiForWindow(window).max(1);
        let scale = |value: i32| value * dpi as i32 / BASE_DPI as i32;
        let size = (scale(BASE_SIZE.0), scale(BASE_SIZE.1));
        let place = panel_place(region, monitor, size, THICKNESS + scale(4));
        SetWindowPos(window, None, place.x, place.y, size.0, size.1, SWP_NOZORDER | SWP_NOACTIVATE).map_err(system)?;

        let font = bar_font(scale(14));
        PANEL.with_borrow_mut(|panel| *panel = Some(PanelState { started: Instant::now(), limit, size, font, pressed }));

        let _ = SetWindowDisplayAffinity(window, WDA_EXCLUDEFROMCAPTURE);
        SetTimer(Some(window), TICK, TICK_MS, None);
        let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
        Ok(window)
    }
}

fn pump_messages() {
    let mut message = MSG::default();
    while unsafe { GetMessageW(&mut message, None, 0, 0) }.as_bool() {
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

unsafe extern "system" fn panel_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_TIMER => {
            let _ = unsafe { InvalidateRect(Some(window), None, false) };
            LRESULT(0)
        }
        WM_PAINT => {
            paint(window);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            PANEL.with_borrow(|panel| {
                if let Some(panel) = panel
                    && let Some(button) = button_at(x, panel.size)
                {
                    let _ = panel.pressed.send(button);
                }
            });
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = unsafe { DestroyWindow(window) };
            LRESULT(0)
        }
        WM_DESTROY => {
            let _ = unsafe { KillTimer(Some(window), TICK) };
            if let Some(panel) = PANEL.with_borrow_mut(Option::take) {
                let _ = unsafe { DeleteObject(panel.font.into()) };
            }
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

pub(crate) fn bar_font(pixel_height: i32) -> HFONT {
    let mut face = [0u16; 32];
    for (slot, unit) in face.iter_mut().zip("Malgun Gothic".encode_utf16()) {
        *slot = unit;
    }
    unsafe {
        CreateFontIndirectW(&LOGFONTW {
            lfHeight: -pixel_height,
            lfWeight: 400,
            lfCharSet: FONT_CHARSET(DEFAULT_CHARSET.0),
            lfQuality: CLEARTYPE_QUALITY,
            lfFaceName: face,
            ..Default::default()
        })
    }
}

pub(crate) fn draw_bar(canvas: HDC, (width, height): (i32, i32), font: HFONT, label: &str, action: &str) {
    let action_left = width - height - stop_width(height);
    let parts = [
        (RECT { left: 0, top: 0, right: action_left, bottom: height }, PANEL_GRAY, label),
        (RECT { left: action_left, top: 0, right: width - height, bottom: height }, RECORDING_RED, action),
        (RECT { left: width - height, top: 0, right: width, bottom: height }, BUTTON_GRAY, "✕"),
    ];
    unsafe {
        let earlier_font = SelectObject(canvas, font.into());
        SetBkMode(canvas, TRANSPARENT);
        SetTextColor(canvas, WHITE);
        for (mut area, color, label) in parts {
            let brush = CreateSolidBrush(color);
            FillRect(canvas, &area, brush);
            let _ = DeleteObject(brush.into());
            let mut text: Vec<u16> = label.encode_utf16().collect();
            DrawTextW(canvas, &mut text, &mut area, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        }
        SelectObject(canvas, earlier_font);
    }
}

fn paint(window: HWND) {
    let mut paint = PAINTSTRUCT::default();
    let canvas = unsafe { BeginPaint(window, &mut paint) };
    PANEL.with_borrow(|panel| {
        if let Some(panel) = panel {
            let left = remaining_label(panel.limit.saturating_sub(panel.started.elapsed()));
            draw_bar(canvas, panel.size, panel.font, &left, "끝");
        }
    });
    let _ = unsafe { EndPaint(window, &paint) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GWL_EXSTYLE, GetWindowDisplayAffinity, GetWindowLongPtrW, GetWindowRect, IsWindowVisible,
        SendMessageW,
    };

    use crate::desktop::{ONE_AT_A_TIME, PRIMARY};

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT { left, top, right, bottom }
    }

    const MONITOR: RECT = RECT { left: 0, top: 0, right: 2560, bottom: 1440 };
    const SIZE: (i32, i32) = (184, 32);
    const GAP: i32 = 6;

    fn at(place: POINT) -> (i32, i32) {
        (place.x, place.y)
    }

    #[test]
    fn the_panel_sits_below_the_region_lined_up_with_its_right_edge() {
        assert_eq!(at(panel_place(rect(400, 300, 900, 600), MONITOR, SIZE, GAP)), (900 - 184, 606));
    }

    #[test]
    fn without_room_below_the_panel_goes_above() {
        assert_eq!(at(panel_place(rect(400, 300, 900, 1420), MONITOR, SIZE, GAP)), (716, 300 - 6 - 32));
    }

    #[test]
    fn without_room_above_or_below_the_panel_goes_inside_the_bottom_right_corner() {
        assert_eq!(at(panel_place(MONITOR, MONITOR, SIZE, GAP)), (2560 - 184 - 6, 1440 - 32 - 6));
    }

    #[test]
    fn a_region_narrower_than_the_panel_near_the_left_edge_keeps_the_panel_on_the_monitor() {
        assert_eq!(at(panel_place(rect(0, 100, 50, 150), MONITOR, SIZE, GAP)), (0, 156));
    }

    #[test]
    fn a_monitor_that_does_not_start_at_zero_is_respected() {
        let left_monitor = rect(-1920, 0, 0, 1080);
        assert_eq!(at(panel_place(rect(-300, 200, -100, 400), left_monitor, SIZE, GAP)), (-284, 406));
    }

    #[test]
    fn the_right_end_is_cancel_then_stop_and_the_rest_is_the_time() {
        let (width, height) = SIZE;
        assert_eq!(button_at(width - 1, SIZE), Some(PanelButton::Cancel));
        assert_eq!(button_at(width - height, SIZE), Some(PanelButton::Cancel));
        assert_eq!(button_at(width - height - 1, SIZE), Some(PanelButton::Stop));
        assert_eq!(button_at(width - height - stop_width(height), SIZE), Some(PanelButton::Stop));
        assert_eq!(button_at(width - height - stop_width(height) - 1, SIZE), None);
        assert_eq!(button_at(0, SIZE), None);
        assert_eq!(button_at(width, SIZE), None, "past the right edge");
        assert_eq!(button_at(-1, SIZE), None);
    }

    #[test]
    fn the_time_left_is_rounded_up_to_whole_seconds() {
        assert_eq!(remaining_label(Duration::from_millis(27_200)), "● 28초 남음");
        assert_eq!(remaining_label(Duration::from_secs(30)), "● 30초 남음");
        assert_eq!(remaining_label(Duration::from_millis(1)), "● 1초 남음");
        assert_eq!(remaining_label(Duration::ZERO), "● 0초 남음");
    }

    fn the_panel_window() -> Option<HWND> {
        unsafe { FindWindowW(CLASS_NAME, None) }.ok()
    }

    fn click(window: HWND, x: i32, y: i32) {
        let point = LPARAM(((y as isize) << 16) | (x as isize & 0xFFFF));
        unsafe { PostMessageW(Some(window), WM_LBUTTONUP, WPARAM(0), point) }.unwrap();
    }

    fn pressed_within(panel: &RecordingPanel, wait: Duration) -> Option<PanelButton> {
        let until = Instant::now() + wait;
        while Instant::now() < until {
            if let Some(button) = panel.pressed() {
                return Some(button);
            }
            thread::sleep(Duration::from_millis(10));
        }
        None
    }

    #[test]
    #[ignore = "needs an unlocked desktop; run with --ignored"]
    fn the_panel_shows_below_the_region_never_takes_the_focus_and_is_left_out_of_captures() {
        let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let region = ScreenRect { monitor: PRIMARY, x: 300, y: 250, width: 400, height: 200 };

        let panel = RecordingPanel::show(region, Duration::from_secs(30)).unwrap();

        let window = the_panel_window().expect("the panel window exists");
        assert!(unsafe { IsWindowVisible(window) }.as_bool());
        let mut shown = RECT::default();
        unsafe { GetWindowRect(window, &mut shown) }.unwrap();
        let bounds = monitor_bounds(PRIMARY).unwrap();
        assert_eq!(shown.right, bounds.left + 700, "lined up with the region's right edge");
        assert!(shown.top > bounds.top + 450, "below the region and its border: {shown:?}");
        let style = unsafe { GetWindowLongPtrW(window, GWL_EXSTYLE) } as u32;
        assert_ne!(style & WS_EX_NOACTIVATE.0, 0);
        let activation = unsafe { SendMessageW(window, WM_MOUSEACTIVATE, None, None) };
        assert_eq!(activation.0 as u32, MA_NOACTIVATE, "a click does not activate it");
        let mut affinity = 0;
        unsafe { GetWindowDisplayAffinity(window, &mut affinity) }.unwrap();
        assert_eq!(affinity, WDA_EXCLUDEFROMCAPTURE.0);

        drop(panel);

        assert_eq!(the_panel_window(), None);
    }

    #[test]
    #[ignore = "needs an unlocked desktop; run with --ignored"]
    fn clicking_a_button_tells_which_one_and_clicking_the_time_tells_nothing() {
        let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let region = ScreenRect { monitor: PRIMARY, x: 300, y: 250, width: 400, height: 200 };
        let panel = RecordingPanel::show(region, Duration::from_secs(30)).unwrap();
        let window = the_panel_window().unwrap();
        let mut shown = RECT::default();
        unsafe { GetWindowRect(window, &mut shown) }.unwrap();
        let (width, height) = (shown.right - shown.left, shown.bottom - shown.top);

        click(window, 4, height / 2);
        assert_eq!(pressed_within(&panel, Duration::from_millis(300)), None);
        click(window, width - height - stop_width(height) / 2, height / 2);
        assert_eq!(pressed_within(&panel, Duration::from_secs(1)), Some(PanelButton::Stop));
        click(window, width - height / 2, height / 2);
        assert_eq!(pressed_within(&panel, Duration::from_secs(1)), Some(PanelButton::Cancel));
    }
}
