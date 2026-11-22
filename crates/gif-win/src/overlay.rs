use std::cell::RefCell;
use std::sync::Once;

use gif_core::selection::{Point, drag_region};
use gif_core::session::{MonitorId, ScreenRect};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BLACK_BRUSH, BeginPaint, CreateSolidBrush, DeleteObject, EndPaint, FillRect, FrameRect, GetStockObject, HBRUSH,
    InvalidateRect, PAINTSTRUCT, WHITE_BRUSH,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetThreadDpiAwarenessContext};
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, SetFocus, VK_ESCAPE};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, FindWindowW, GetMessageW, PostMessageW, WM_CLOSE, IDC_CROSS, LWA_ALPHA, LWA_COLORKEY,
    LoadCursorW, MSG, RegisterClassExW, SW_SHOW, SetCursor, SetForegroundWindow, SetLayeredWindowAttributes,
    ShowWindow, TranslateMessage, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_PAINT,
    WM_RBUTTONDOWN, WM_SETCURSOR, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{PCWSTR, w};

use crate::CaptureError;
use crate::capture::{monitor_bounds, system};

const CLASS_NAME: PCWSTR = w!("midi-selection-overlay");
const DIM_ALPHA: u8 = 120;
const SEE_THROUGH: COLORREF = COLORREF(0x00FF_00FF);

#[derive(Clone, Copy)]
enum Outcome {
    Selected(ScreenRect),
    Cancelled,
}

struct Selection {
    monitor: MonitorId,
    monitor_size: (u32, u32),
    pressed_at: Option<Point>,
    pointer: Point,
    outcome: Option<Outcome>,
}

thread_local! {
    static SELECTION: RefCell<Option<Selection>> = const { RefCell::new(None) };
}

pub fn select_region(monitor: MonitorId) -> Result<Option<ScreenRect>, CaptureError> {
    let earlier_awareness = unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let selected = select_in_physical_pixels(monitor);
    unsafe { SetThreadDpiAwarenessContext(earlier_awareness) };
    selected
}

pub fn cancel_selection() {
    if let Ok(window) = unsafe { FindWindowW(CLASS_NAME, None) } {
        let _ = unsafe { PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0)) };
    }
}

fn select_in_physical_pixels(monitor: MonitorId) -> Result<Option<ScreenRect>, CaptureError> {
    let bounds = monitor_bounds(monitor)?;
    let (width, height) = (bounds.right - bounds.left, bounds.bottom - bounds.top);
    let monitor_size = (width as u32, height as u32);

    let window = create_window(bounds.left, bounds.top, width, height)?;
    SELECTION.set(Some(Selection { monitor, monitor_size, pressed_at: None, pointer: (0, 0), outcome: None }));
    unsafe {
        let _ = ShowWindow(window, SW_SHOW);
        let _ = SetForegroundWindow(window);
        let _ = SetFocus(Some(window));
    }

    let outcome = run_until_decided();
    SELECTION.set(None);
    unsafe { DestroyWindow(window) }.map_err(system)?;

    match outcome? {
        Outcome::Selected(region) => Ok(Some(region)),
        Outcome::Cancelled => Ok(None),
    }
}

fn create_window(left: i32, top: i32, width: i32, height: i32) -> Result<HWND, CaptureError> {
    let module = unsafe { GetModuleHandleW(None) }.map_err(system)?;

    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| {
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(window_procedure),
            hInstance: module.into(),
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        unsafe { RegisterClassExW(&class) };
    });

    let window = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TOOLWINDOW,
            CLASS_NAME,
            w!(""),
            WS_POPUP,
            left,
            top,
            width,
            height,
            None,
            None,
            Some(module.into()),
            None,
        )
    }
    .map_err(system)?;
    unsafe { SetLayeredWindowAttributes(window, SEE_THROUGH, DIM_ALPHA, LWA_COLORKEY | LWA_ALPHA) }.map_err(system)?;
    Ok(window)
}

fn run_until_decided() -> Result<Outcome, CaptureError> {
    let mut message = MSG::default();
    loop {
        if let Some(outcome) = SELECTION.with_borrow(|selection| selection.as_ref().and_then(|s| s.outcome)) {
            return Ok(outcome);
        }
        let got = unsafe { GetMessageW(&mut message, None, 0, 0) };
        match got.0 {
            -1 => return Err(system(windows::core::Error::from_thread())),
            0 => return Ok(Outcome::Cancelled),
            _ => unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            },
        }
    }
}

fn pointer_position(lparam: LPARAM) -> Point {
    let x = (lparam.0 & 0xFFFF) as u16 as i16;
    let y = ((lparam.0 >> 16) & 0xFFFF) as u16 as i16;
    (i32::from(x), i32::from(y))
}

fn dragged_so_far(selection: &Selection) -> Option<RECT> {
    let (from, to) = (selection.pressed_at?, selection.pointer);
    let clamp = |value: i32, limit: u32| value.clamp(0, limit as i32);
    let (width, height) = selection.monitor_size;
    Some(RECT {
        left: clamp(from.0.min(to.0), width),
        top: clamp(from.1.min(to.1), height),
        right: clamp(from.0.max(to.0), width),
        bottom: clamp(from.1.max(to.1), height),
    })
}

unsafe extern "system" fn window_procedure(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let handled = SELECTION.with(|selection| {
        let mut selection = selection.try_borrow_mut().ok()?;
        let selection = selection.as_mut()?;
        match message {
            WM_LBUTTONDOWN => {
                selection.pressed_at = Some(pointer_position(lparam));
                selection.pointer = pointer_position(lparam);
                unsafe { SetCapture(window) };
            }
            WM_MOUSEMOVE if selection.pressed_at.is_some() => selection.pointer = pointer_position(lparam),
            WM_LBUTTONUP => {
                let Some(from) = selection.pressed_at.take() else { return Some(()) };
                unsafe { ReleaseCapture() }.ok();
                let region = drag_region(selection.monitor, selection.monitor_size, from, pointer_position(lparam));
                selection.outcome = region.map(Outcome::Selected);
            }
            WM_RBUTTONDOWN | WM_CLOSE => selection.outcome = Some(Outcome::Cancelled),
            WM_KEYDOWN if wparam.0 == usize::from(VK_ESCAPE.0) => selection.outcome = Some(Outcome::Cancelled),
            WM_PAINT => {
                paint(window, selection);
                return Some(());
            }
            WM_ERASEBKGND => return Some(()),
            WM_SETCURSOR => {
                unsafe { SetCursor(LoadCursorW(None, IDC_CROSS).ok()) };
                return Some(());
            }
            _ => return None,
        }
        unsafe { InvalidateRect(Some(window), None, false) }.ok().ok();
        Some(())
    });

    match handled {
        Some(()) if message == WM_ERASEBKGND || message == WM_SETCURSOR => LRESULT(1),
        Some(()) => LRESULT(0),
        None => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

fn paint(window: HWND, selection: &Selection) {
    let (width, height) = (selection.monitor_size.0 as i32, selection.monitor_size.1 as i32);
    let hole = dragged_so_far(selection).unwrap_or_default();
    let dim = [
        RECT { left: 0, top: 0, right: width, bottom: hole.top },
        RECT { left: 0, top: hole.bottom, right: width, bottom: height },
        RECT { left: 0, top: hole.top, right: hole.left, bottom: hole.bottom },
        RECT { left: hole.right, top: hole.top, right: width, bottom: hole.bottom },
    ];

    let mut paint = PAINTSTRUCT::default();
    unsafe {
        let canvas = BeginPaint(window, &mut paint);
        let black = HBRUSH(GetStockObject(BLACK_BRUSH).0);
        for band in &dim {
            FillRect(canvas, band, black);
        }
        let see_through = CreateSolidBrush(SEE_THROUGH);
        FillRect(canvas, &hole, see_through);
        let _ = DeleteObject(see_through.into());
        FrameRect(canvas, &hole, HBRUSH(GetStockObject(WHITE_BRUSH).0));
        let _ = EndPaint(window, &paint);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    use crate::desktop::{ONE_AT_A_TIME, PRIMARY};

    fn at(x: u16, y: u16) -> LPARAM {
        LPARAM((y as isize) << 16 | x as isize)
    }

    fn post_when_the_overlay_is_up(messages: Vec<(u32, WPARAM, LPARAM)>) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            let overlay = (0..100)
                .find_map(|_| {
                    thread::sleep(Duration::from_millis(20));
                    unsafe { FindWindowW(CLASS_NAME, None) }.ok()
                })
                .expect("the overlay window appears");
            let overlay = overlay.0 as isize;
            for (message, wparam, lparam) in messages {
                thread::sleep(Duration::from_millis(30));
                unsafe { PostMessageW(Some(HWND(overlay as _)), message, wparam, lparam) }.unwrap();
            }
        })
    }

    #[test]
    #[ignore = "needs an unlocked desktop; run with --ignored"]
    fn a_drag_on_the_overlay_selects_that_rectangle() {
        let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mouse = post_when_the_overlay_is_up(vec![
            (WM_LBUTTONDOWN, WPARAM(1), at(400, 300)),
            (WM_MOUSEMOVE, WPARAM(1), at(250, 200)),
            (WM_MOUSEMOVE, WPARAM(1), at(100, 120)),
            (WM_LBUTTONUP, WPARAM(0), at(100, 120)),
        ]);

        let region = select_region(PRIMARY).unwrap();
        mouse.join().unwrap();

        assert_eq!(region, Some(ScreenRect { monitor: PRIMARY, x: 100, y: 120, width: 300, height: 180 }));
    }

    #[test]
    #[ignore = "needs an unlocked desktop; run with --ignored"]
    fn a_click_keeps_the_overlay_open_and_escape_cancels_it() {
        let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let keys = post_when_the_overlay_is_up(vec![
            (WM_LBUTTONDOWN, WPARAM(1), at(400, 300)),
            (WM_LBUTTONUP, WPARAM(0), at(400, 300)),
            (WM_KEYDOWN, WPARAM(usize::from(VK_ESCAPE.0)), LPARAM(0)),
        ]);

        let region = select_region(PRIMARY).unwrap();
        keys.join().unwrap();

        assert_eq!(region, None);
    }

    #[test]
    #[ignore = "needs an unlocked desktop; run with --ignored"]
    fn the_dimming_is_gone_from_the_first_capture_after_selecting() {
        let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mouse = post_when_the_overlay_is_up(vec![
            (WM_LBUTTONDOWN, WPARAM(1), at(100, 100)),
            (WM_MOUSEMOVE, WPARAM(1), at(200, 200)),
            (WM_LBUTTONUP, WPARAM(0), at(200, 200)),
        ]);
        select_region(PRIMARY).unwrap();
        mouse.join().unwrap();

        let was_dimmed = ScreenRect { monitor: PRIMARY, x: 300, y: 300, width: 400, height: 300 };
        let brightness = |pixels: Vec<[u8; 3]>| pixels.iter().flatten().map(|&channel| u64::from(channel)).sum::<u64>();
        let mut capturer = crate::ScreenCapturer::open(PRIMARY).unwrap();
        let at_once = brightness(capturer.capture(was_dimmed).unwrap());
        thread::sleep(Duration::from_millis(500));
        let later = brightness(capturer.capture(was_dimmed).unwrap());

        assert!(at_once * 10 >= later * 9, "at once {at_once}, later {later}");
    }

    #[test]
    #[ignore = "needs an unlocked desktop; run with --ignored"]
    fn the_selection_can_be_cancelled_from_another_thread() {
        let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let canceller = thread::spawn(|| {
            thread::sleep(Duration::from_millis(400));
            cancel_selection();
        });

        let region = select_region(PRIMARY).unwrap();
        canceller.join().unwrap();

        assert_eq!(region, None);
    }

    #[test]
    fn a_monitor_that_is_not_there_is_reported() {
        assert_eq!(select_region(MonitorId(99)), Err(CaptureError::NoSuchMonitor(MonitorId(99))));
    }
}
