use std::sync::{Once, mpsc};
use std::thread::{self, JoinHandle};

use gif_core::session::ScreenRect;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{CombineRgn, CreateRectRgn, CreateSolidBrush, DeleteObject, RGN_DIFF, SetWindowRgn};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, LWA_ALPHA, MSG, PostMessageW,
    PostQuitMessage, RegisterClassExW, SW_SHOWNOACTIVATE, SetLayeredWindowAttributes, SetWindowDisplayAffinity,
    ShowWindow, TranslateMessage, WDA_EXCLUDEFROMCAPTURE, WM_CLOSE, WM_DESTROY, WNDCLASSEXW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{PCWSTR, w};

use crate::CaptureError;
use crate::capture::{monitor_bounds, system};

pub const THICKNESS: i32 = 2;
const RECORDING_RED: COLORREF = COLORREF(0x0039_35E5);
const CLASS_NAME: PCWSTR = w!("midi-recording-border");

#[derive(Debug, Clone, Copy, PartialEq)]
struct Frame {
    outer: RECT,
    inner: RECT,
}

fn frame_around(region: RECT, monitor: RECT) -> Frame {
    let outer = RECT {
        left: (region.left - THICKNESS).max(monitor.left),
        top: (region.top - THICKNESS).max(monitor.top),
        right: (region.right + THICKNESS).min(monitor.right),
        bottom: (region.bottom + THICKNESS).min(monitor.bottom),
    };
    let inner = RECT {
        left: outer.left + THICKNESS,
        top: outer.top + THICKNESS,
        right: outer.right - THICKNESS,
        bottom: outer.bottom - THICKNESS,
    };
    Frame { outer, inner }
}

#[derive(Debug)]
pub struct RecordingBorder {
    window: isize,
    thread: Option<JoinHandle<()>>,
}

impl RecordingBorder {
    pub fn show(region: ScreenRect) -> Result<RecordingBorder, CaptureError> {
        let monitor = monitor_bounds(region.monitor)?;
        let on_screen = RECT {
            left: monitor.left + region.x,
            top: monitor.top + region.y,
            right: monitor.left + region.x + region.width as i32,
            bottom: monitor.top + region.y + region.height as i32,
        };
        let frame = frame_around(on_screen, monitor);

        let (told, shown) = mpsc::channel();
        let thread = thread::spawn(move || {
            unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
            let window = create_window(frame);
            let created = window.is_ok();
            let _ = told.send(window.map(|window| window.0 as isize));
            if created {
                pump_messages();
            }
        });
        match shown.recv() {
            Ok(Ok(window)) => Ok(RecordingBorder { window, thread: Some(thread) }),
            Ok(Err(problem)) => {
                let _ = thread.join();
                Err(problem)
            }
            Err(_) => Err(CaptureError::System("the border thread ended before showing the border".to_string())),
        }
    }
}

fn create_window(frame: Frame) -> Result<HWND, CaptureError> {
    let module = unsafe { GetModuleHandleW(None) }.map_err(system)?;
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| {
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(border_proc),
            hInstance: module.into(),
            hbrBackground: unsafe { CreateSolidBrush(RECORDING_RED) },
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        unsafe { RegisterClassExW(&class) };
    });

    let Frame { outer, inner } = frame;
    let (width, height) = (outer.right - outer.left, outer.bottom - outer.top);
    unsafe {
        let window = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            CLASS_NAME,
            w!(""),
            WS_POPUP,
            outer.left,
            outer.top,
            width,
            height,
            None,
            None,
            Some(module.into()),
            None,
        )
        .map_err(system)?;
        SetLayeredWindowAttributes(window, COLORREF(0), u8::MAX, LWA_ALPHA).map_err(system)?;

        let ring = CreateRectRgn(0, 0, width, height);
        let hole = CreateRectRgn(inner.left - outer.left, inner.top - outer.top, inner.right - outer.left, inner.bottom - outer.top);
        CombineRgn(Some(ring), Some(ring), Some(hole), RGN_DIFF);
        let _ = DeleteObject(hole.into());
        SetWindowRgn(window, Some(ring), false);

        let _ = SetWindowDisplayAffinity(window, WDA_EXCLUDEFROMCAPTURE);
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

unsafe extern "system" fn border_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_CLOSE => {
            let _ = unsafe { DestroyWindow(window) };
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

impl Drop for RecordingBorder {
    fn drop(&mut self) {
        let _ = unsafe { PostMessageW(Some(HWND(self.window as _)), WM_CLOSE, WPARAM(0), LPARAM(0)) };
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GWL_EXSTYLE, GetWindowDisplayAffinity, GetWindowLongPtrW, GetWindowRect, IsWindowVisible,
    };

    use crate::desktop::{ONE_AT_A_TIME, PRIMARY};

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT { left, top, right, bottom }
    }

    const MONITOR: RECT = RECT { left: 0, top: 0, right: 2560, bottom: 1440 };

    #[test]
    fn the_border_sits_just_outside_the_region() {
        let frame = frame_around(rect(100, 200, 400, 380), MONITOR);
        assert_eq!(frame.outer, rect(98, 198, 402, 382));
        assert_eq!(frame.inner, rect(100, 200, 400, 380));
    }

    #[test]
    fn at_the_edge_of_the_monitor_the_border_moves_inside_the_region() {
        let whole_monitor = frame_around(MONITOR, MONITOR);
        assert_eq!(whole_monitor.outer, MONITOR);
        assert_eq!(whole_monitor.inner, rect(2, 2, 2558, 1438));

        let top_left_corner = frame_around(rect(0, 0, 300, 200), MONITOR);
        assert_eq!(top_left_corner.outer, rect(0, 0, 302, 202));
        assert_eq!(top_left_corner.inner, rect(2, 2, 300, 200));
    }

    #[test]
    fn a_monitor_that_does_not_start_at_zero_is_respected() {
        let left_monitor = rect(-1920, 0, 0, 1080);
        let frame = frame_around(rect(-1920, 100, -1620, 300), left_monitor);
        assert_eq!(frame.outer, rect(-1920, 98, -1618, 302));
        assert_eq!(frame.inner, rect(-1918, 100, -1620, 300));
    }

    fn the_border_window() -> Option<HWND> {
        unsafe { FindWindowW(CLASS_NAME, None) }.ok()
    }

    #[test]
    #[ignore = "needs an unlocked desktop; run with --ignored"]
    fn the_border_is_shown_around_the_region_lets_clicks_through_and_is_gone_when_dropped() {
        let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let region = ScreenRect { monitor: PRIMARY, x: 300, y: 250, width: 320, height: 180 };

        let border = RecordingBorder::show(region).unwrap();

        let window = the_border_window().expect("the border window exists");
        let mut shown = RECT::default();
        unsafe { GetWindowRect(window, &mut shown) }.unwrap();
        let bounds = monitor_bounds(PRIMARY).unwrap();
        let expected = rect(bounds.left + 298, bounds.top + 248, bounds.left + 622, bounds.top + 432);
        assert_eq!(shown, expected);
        assert!(unsafe { IsWindowVisible(window) }.as_bool());
        let style = unsafe { GetWindowLongPtrW(window, GWL_EXSTYLE) } as u32;
        assert_ne!(style & WS_EX_TRANSPARENT.0, 0, "clicks go through to what is underneath");
        assert_ne!(style & WS_EX_NOACTIVATE.0, 0, "it never takes the focus");
        let mut affinity = 0;
        unsafe { GetWindowDisplayAffinity(window, &mut affinity) }.unwrap();
        assert_eq!(affinity, WDA_EXCLUDEFROMCAPTURE.0, "it is left out of screen captures");

        drop(border);

        assert_eq!(the_border_window(), None);
    }

    #[test]
    #[ignore = "needs an unlocked desktop; run with --ignored"]
    fn a_border_inside_the_captured_area_does_not_show_up_in_the_capture() {
        let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let patch = crate::overlay::tests::white_patch(300, 300, 200, 150);
        let region = ScreenRect { monitor: PRIMARY, x: 350, y: 350, width: 100, height: 60 };
        let border = RecordingBorder::show(region).unwrap();
        thread::sleep(std::time::Duration::from_millis(200));

        let around_the_border = ScreenRect { monitor: PRIMARY, x: 340, y: 340, width: 120, height: 80 };
        let mut capturer = crate::ScreenCapturer::open(PRIMARY).unwrap();
        let captured = capturer.capture(around_the_border).unwrap();
        drop(border);
        unsafe { DestroyWindow(patch) }.unwrap();

        let darkest = captured.iter().flatten().min().unwrap();
        assert_eq!(*darkest, 255, "the red border showed up in the capture");
    }
}
