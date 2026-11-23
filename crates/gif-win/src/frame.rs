use std::cell::RefCell;
use std::sync::Once;

use gif_core::selection::{Nudge, SizeEntry, SizeKey, SizeSide, frame_start, nudge};
use gif_core::session::{MonitorId, ScreenRect};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, ClientToScreen, CombineRgn, CreateRectRgn, CreateSolidBrush, DeleteObject, EndPaint, FillRect, GetDC,
    GetTextExtentPoint32W, HFONT, InvalidateRect, PAINTSTRUCT, RGN_DIFF, ReleaseDC, SelectObject, SetWindowRgn,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow, SetThreadDpiAwarenessContext};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, SetFocus, VK_CONTROL, VK_DOWN, VK_ESCAPE, VK_LEFT, VK_RETURN, VK_RIGHT,
    VK_SHIFT, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, FindWindowW, GetMessageW, GetWindowRect,
    HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTLEFT, HTNOWHERE, HTRIGHT, HTTOP, HTTOPLEFT, HTTOPRIGHT, IDC_ARROW,
    LoadCursorW, MINMAXINFO, MSG, PostMessageW, RegisterClassExW, SW_SHOW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE,
    SWP_NOSIZE, SWP_NOZORDER, SetForegroundWindow, SetWindowDisplayAffinity, SetWindowPos, ShowWindow,
    TranslateMessage, WDA_EXCLUDEFROMCAPTURE, WM_CHAR, WM_CLOSE, WM_ERASEBKGND, WM_GETMINMAXINFO, WM_KEYDOWN, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCHITTEST, WM_PAINT, WM_SIZE, WM_WINDOWPOSCHANGED, WNDCLASSEXW, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{PCWSTR, w};

use crate::CaptureError;
use crate::border::THICKNESS;
use crate::capture::{every_monitor, monitor_bounds, monitor_holding_most_of, system};
use crate::panel::{BASE_DPI, BASE_SIZE, PanelButton, bar_font, button_at, draw_bar, panel_place, stop_width};

const RING_CLASS: PCWSTR = w!("midi-frame-ring");
const BAR_CLASS: PCWSTR = w!("midi-frame-bar");
const BASE_MARGIN: i32 = 8;
const FRAME_DARK: COLORREF = COLORREF(0x0030_3030);
const WAITING_AMBER: COLORREF = COLORREF(0x0007_C1FF);
const NUDGE_STEP: i32 = 1;
const NUDGE_STEP_WITH_CTRL: i32 = 10;

fn ring_hit(x: i32, y: i32, (width, height): (i32, i32), margin: i32) -> u32 {
    let on_ring = x < margin || x >= width - margin || y < margin || y >= height - margin;
    if !on_ring {
        return HTNOWHERE;
    }
    let corner = margin * 3;
    let (near_left, near_right) = (x < corner, x >= width - corner);
    let (near_top, near_bottom) = (y < corner, y >= height - corner);
    match () {
        _ if near_left && near_top => HTTOPLEFT,
        _ if near_right && near_top => HTTOPRIGHT,
        _ if near_left && near_bottom => HTBOTTOMLEFT,
        _ if near_right && near_bottom => HTBOTTOMRIGHT,
        _ if x < margin => HTLEFT,
        _ if x >= width - margin => HTRIGHT,
        _ if y < margin => HTTOP,
        _ => HTBOTTOM,
    }
}

#[derive(Clone, Copy)]
struct Drag {
    pressed_at: POINT,
    ring_was_at: POINT,
}

struct FrameState {
    monitor_id: MonitorId,
    monitor: RECT,
    monitors: Vec<RECT>,
    margin: i32,
    bar_size: (i32, i32),
    gap: i32,
    ring: HWND,
    bar: HWND,
    font: HFONT,
    drag: Option<Drag>,
    size_entry: Option<SizeEntry>,
    outcome: Option<Option<ScreenRect>>,
}

thread_local! {
    static FRAME: RefCell<Option<FrameState>> = const { RefCell::new(None) };
}

#[derive(Clone, Copy)]
struct Layout {
    monitor_id: MonitorId,
    monitor: RECT,
    margin: i32,
    bar_size: (i32, i32),
    gap: i32,
    ring: HWND,
    bar: HWND,
}

fn layout() -> Option<Layout> {
    FRAME.with(|frame| {
        let frame = frame.try_borrow().ok()?;
        let frame = frame.as_ref()?;
        Some(Layout {
            monitor_id: frame.monitor_id,
            monitor: frame.monitor,
            margin: frame.margin,
            bar_size: frame.bar_size,
            gap: frame.gap,
            ring: frame.ring,
            bar: frame.bar,
        })
    })
}

fn with_frame<T>(change: impl FnOnce(&mut FrameState) -> T) -> Option<T> {
    FRAME.with(|frame| frame.try_borrow_mut().ok()?.as_mut().map(change))
}

fn rect_of(window: HWND) -> RECT {
    let mut rect = RECT::default();
    let _ = unsafe { GetWindowRect(window, &mut rect) };
    rect
}

fn region_on_screen(layout: Layout) -> RECT {
    let ring = rect_of(layout.ring);
    let margin = layout.margin;
    RECT { left: ring.left + margin, top: ring.top + margin, right: ring.right - margin, bottom: ring.bottom - margin }
}

fn region_now(layout: Layout) -> ScreenRect {
    let on_screen = region_on_screen(layout);
    ScreenRect {
        monitor: layout.monitor_id,
        x: on_screen.left - layout.monitor.left,
        y: on_screen.top - layout.monitor.top,
        width: (on_screen.right - on_screen.left).max(0) as u32,
        height: (on_screen.bottom - on_screen.top).max(0) as u32,
    }
}

fn monitor_size(layout: Layout) -> (u32, u32) {
    ((layout.monitor.right - layout.monitor.left) as u32, (layout.monitor.bottom - layout.monitor.top) as u32)
}

fn place_ring(layout: Layout, region: ScreenRect) {
    let margin = layout.margin;
    let left = layout.monitor.left + region.x - margin;
    let top = layout.monitor.top + region.y - margin;
    let (width, height) = (region.width as i32 + 2 * margin, region.height as i32 + 2 * margin);
    let _ = unsafe { SetWindowPos(layout.ring, None, left, top, width, height, SWP_NOZORDER | SWP_NOACTIVATE) };
}

fn rehome(now: Layout) -> Layout {
    let area = region_on_screen(now);
    let moved = with_frame(|frame| {
        let index = monitor_holding_most_of(area, &frame.monitors)?;
        let monitor_id = MonitorId(index as u32);
        (monitor_id != frame.monitor_id).then(|| {
            frame.monitor_id = monitor_id;
            frame.monitor = frame.monitors[index];
        })
    });
    match moved.flatten() {
        Some(()) => layout().unwrap_or(now),
        None => now,
    }
}

fn follow(layout: Layout) {
    let place = panel_place(region_on_screen(layout), layout.monitor, layout.bar_size, layout.gap);
    let (width, height) = layout.bar_size;
    unsafe {
        let _ = SetWindowPos(layout.bar, None, place.x, place.y, width, height, SWP_NOZORDER | SWP_NOACTIVATE);
        let _ = InvalidateRect(Some(layout.bar), None, false);
    }
}

fn decide(outcome: Option<ScreenRect>) {
    with_frame(|frame| frame.outcome.get_or_insert(outcome).to_owned());
}

pub fn cancel() {
    if let Ok(bar) = unsafe { FindWindowW(BAR_CLASS, None) } {
        let _ = unsafe { PostMessageW(Some(bar), WM_CLOSE, WPARAM(0), LPARAM(0)) };
    }
}

pub fn adjust_frame(start: ScreenRect) -> Result<Option<ScreenRect>, CaptureError> {
    let earlier_awareness = unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let chosen = adjust_in_physical_pixels(start);
    unsafe { SetThreadDpiAwarenessContext(earlier_awareness) };
    chosen
}

fn register(class: PCWSTR, procedure: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT) -> Result<(), CaptureError> {
    let module = unsafe { GetModuleHandleW(None) }.map_err(system)?;
    let window_class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(procedure),
        hInstance: module.into(),
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default(),
        lpszClassName: class,
        ..Default::default()
    };
    unsafe { RegisterClassExW(&window_class) };
    Ok(())
}

fn create(class: PCWSTR, monitor: RECT) -> Result<HWND, CaptureError> {
    let module = unsafe { GetModuleHandleW(None) }.map_err(system)?;
    let window = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class,
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
    }
    .map_err(system)?;
    let _ = unsafe { SetWindowDisplayAffinity(window, WDA_EXCLUDEFROMCAPTURE) };
    Ok(window)
}

fn adjust_in_physical_pixels(start: ScreenRect) -> Result<Option<ScreenRect>, CaptureError> {
    let monitor = monitor_bounds(start.monitor)?;
    static REGISTER: Once = Once::new();
    let mut registered = Ok(());
    REGISTER.call_once(|| registered = register(RING_CLASS, ring_proc).and_then(|()| register(BAR_CLASS, bar_proc)));
    registered?;

    let ring = create(RING_CLASS, monitor)?;
    let bar = match create(BAR_CLASS, monitor) {
        Ok(bar) => bar,
        Err(problem) => {
            let _ = unsafe { DestroyWindow(ring) };
            return Err(problem);
        }
    };
    let dpi = unsafe { GetDpiForWindow(ring) }.max(1) as i32;
    let scale = |value: i32| value * dpi / BASE_DPI as i32;
    let margin = scale(BASE_MARGIN);
    FRAME.set(Some(FrameState {
        monitor_id: start.monitor,
        monitor,
        monitors: every_monitor().unwrap_or_else(|_| vec![monitor]),
        margin,
        bar_size: (scale(BASE_SIZE.0), scale(BASE_SIZE.1)),
        gap: margin + scale(4),
        ring,
        bar,
        font: bar_font(scale(14)),
        drag: None,
        size_entry: None,
        outcome: None,
    }));

    if let Some(layout) = layout() {
        place_ring(layout, start);
        follow(layout);
    }
    unsafe {
        let _ = ShowWindow(ring, SW_SHOWNOACTIVATE);
        let _ = ShowWindow(bar, SW_SHOW);
        let _ = SetForegroundWindow(bar);
        let _ = SetFocus(Some(bar));
    }

    let mut message = MSG::default();
    let outcome = loop {
        if let Some(outcome) = with_frame(|frame| frame.outcome).flatten() {
            break outcome;
        }
        if !unsafe { GetMessageW(&mut message, None, 0, 0) }.as_bool() {
            break None;
        }
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    };

    let state = FRAME.take();
    unsafe {
        let _ = DestroyWindow(bar);
        let _ = DestroyWindow(ring);
    }
    let mut last_monitor = monitor;
    if let Some(state) = state {
        let _ = unsafe { DeleteObject(state.font.into()) };
        last_monitor = state.monitor;
    }
    let monitor_size = ((last_monitor.right - last_monitor.left) as u32, (last_monitor.bottom - last_monitor.top) as u32);
    Ok(outcome.map(|region| frame_start(Some(region), region.monitor, monitor_size)))
}

fn signed_point(lparam: LPARAM) -> POINT {
    POINT { x: (lparam.0 & 0xFFFF) as i16 as i32, y: ((lparam.0 >> 16) & 0xFFFF) as i16 as i32 }
}

unsafe extern "system" fn ring_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let default = || unsafe { DefWindowProcW(window, message, wparam, lparam) };
    let Some(layout) = layout() else { return default() };
    match message {
        WM_NCHITTEST => {
            let point = signed_point(lparam);
            let ring = rect_of(window);
            let size = (ring.right - ring.left, ring.bottom - ring.top);
            LRESULT(ring_hit(point.x - ring.left, point.y - ring.top, size, layout.margin) as isize)
        }
        WM_SIZE => {
            let (width, height) = ((lparam.0 & 0xFFFF) as i32, ((lparam.0 >> 16) & 0xFFFF) as i32);
            let margin = layout.margin;
            unsafe {
                let ring = CreateRectRgn(0, 0, width, height);
                let hole = CreateRectRgn(margin, margin, width - margin, height - margin);
                CombineRgn(Some(ring), Some(ring), Some(hole), RGN_DIFF);
                let _ = DeleteObject(hole.into());
                SetWindowRgn(window, Some(ring), true);
            }
            LRESULT(0)
        }
        WM_WINDOWPOSCHANGED => {
            let handled = default();
            follow(rehome(layout));
            handled
        }
        WM_GETMINMAXINFO => {
            let limits = unsafe { &mut *(lparam.0 as *mut MINMAXINFO) };
            let smallest = gif_core::selection::MIN_SIDE as i32 + 2 * layout.margin;
            limits.ptMinTrackSize = POINT { x: smallest, y: smallest };
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            paint_ring(window, layout.margin);
            LRESULT(0)
        }
        WM_CLOSE => {
            decide(None);
            LRESULT(0)
        }
        _ => default(),
    }
}

fn paint_ring(window: HWND, margin: i32) {
    let ring = rect_of(window);
    let (width, height) = (ring.right - ring.left, ring.bottom - ring.top);
    let line = THICKNESS;
    let inner = RECT { left: margin, top: margin, right: width - margin, bottom: height - margin };
    let amber_bands = [
        RECT { left: inner.left - line, top: inner.top - line, right: inner.right + line, bottom: inner.top },
        RECT { left: inner.left - line, top: inner.bottom, right: inner.right + line, bottom: inner.bottom + line },
        RECT { left: inner.left - line, top: inner.top, right: inner.left, bottom: inner.bottom },
        RECT { left: inner.right, top: inner.top, right: inner.right + line, bottom: inner.bottom },
    ];
    let mut paint = PAINTSTRUCT::default();
    unsafe {
        let canvas = BeginPaint(window, &mut paint);
        let dark = CreateSolidBrush(FRAME_DARK);
        FillRect(canvas, &RECT { left: 0, top: 0, right: width, bottom: height }, dark);
        let _ = DeleteObject(dark.into());
        let amber = CreateSolidBrush(WAITING_AMBER);
        for band in &amber_bands {
            FillRect(canvas, band, amber);
        }
        let _ = DeleteObject(amber.into());
        let _ = EndPaint(window, &paint);
    }
}

fn start_now(layout: Layout) {
    apply_typed_size(layout);
    decide(Some(region_now(layout)));
}

fn size_key_of(character: u32) -> Option<SizeKey> {
    match char::from_u32(character)? {
        digit @ '0'..='9' => Some(SizeKey::Digit(digit as u8 - b'0')),
        'x' | 'X' | '*' | ' ' | '\t' => Some(SizeKey::Next),
        '\u{8}' => Some(SizeKey::Erase),
        _ => None,
    }
}

fn type_size(layout: Layout, key: SizeKey) {
    with_frame(|frame| {
        frame.size_entry = match (frame.size_entry, key) {
            (Some(entry), key) => Some(entry.press(key)),
            (None, SizeKey::Digit(_)) => Some(SizeEntry::default().press(key)),
            (None, _) => None,
        }
    });
    let _ = unsafe { InvalidateRect(Some(layout.bar), None, false) };
}

fn apply_typed_size(layout: Layout) {
    if let Some(entry) = with_frame(|frame| frame.size_entry.take()).flatten() {
        place_ring(layout, entry.apply(region_now(layout), monitor_size(layout)));
        let _ = unsafe { InvalidateRect(Some(layout.bar), None, false) };
    }
}

fn bar_label(layout: Layout) -> String {
    let region = region_now(layout);
    let typing = with_frame(|frame| frame.size_entry).flatten();
    typing.map_or_else(|| format!("{}×{}", region.width, region.height), |entry| entry.label(region))
}

fn side_at(bar: HWND, layout: Layout, x: i32) -> SizeSide {
    let label = bar_label(layout);
    let Some((width_part, _)) = label.split_once('×') else { return SizeSide::Width };
    let Some(font) = with_frame(|frame| frame.font) else { return SizeSide::Width };
    let wide = |canvas, text: &str| {
        let text: Vec<u16> = text.encode_utf16().collect();
        let mut size = SIZE::default();
        let _ = unsafe { GetTextExtentPoint32W(canvas, &text, &mut size) };
        size.cx
    };
    let (label_right, split) = unsafe {
        let canvas = GetDC(Some(bar));
        let earlier = SelectObject(canvas, font.into());
        let (all, before, times) = (wide(canvas, &label), wide(canvas, width_part), wide(canvas, "×"));
        SelectObject(canvas, earlier);
        ReleaseDC(Some(bar), canvas);
        let (width, height) = layout.bar_size;
        let label_right = width - height - stop_width(height);
        (label_right, (label_right - all) / 2 + before + times / 2)
    };
    if x < split.min(label_right) { SizeSide::Width } else { SizeSide::Height }
}

fn choose_side(layout: Layout, side: SizeSide) {
    with_frame(|frame| frame.size_entry = Some(frame.size_entry.unwrap_or_default().edit(side)));
    let _ = unsafe { InvalidateRect(Some(layout.bar), None, false) };
}

fn stop_typing(layout: Layout) {
    with_frame(|frame| frame.size_entry = None);
    let _ = unsafe { InvalidateRect(Some(layout.bar), None, false) };
}

fn nudge_by_key(layout: Layout, key: u16) {
    let (dx, dy) = match key {
        key if key == VK_LEFT.0 => (-1, 0),
        key if key == VK_RIGHT.0 => (1, 0),
        key if key == VK_UP.0 => (0, -1),
        key if key == VK_DOWN.0 => (0, 1),
        _ => return,
    };
    let held = |key: u16| unsafe { GetKeyState(i32::from(key)) } < 0;
    let step = if held(VK_CONTROL.0) { NUDGE_STEP_WITH_CTRL } else { NUDGE_STEP };
    let how = if held(VK_SHIFT.0) { Nudge::Grow } else { Nudge::Move };
    let moved = nudge(region_now(layout), monitor_size(layout), (dx * step, dy * step), how);
    place_ring(layout, moved);
}

unsafe extern "system" fn bar_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let default = || unsafe { DefWindowProcW(window, message, wparam, lparam) };
    let Some(layout) = layout() else { return default() };
    match message {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let label = bar_label(layout);
            let mut paint = PAINTSTRUCT::default();
            let canvas = unsafe { BeginPaint(window, &mut paint) };
            if let Some(font) = with_frame(|frame| frame.font) {
                draw_bar(canvas, layout.bar_size, font, &label, "● 시작");
            }
            let _ = unsafe { EndPaint(window, &paint) };
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let mut point = signed_point(lparam);
            if button_at(point.x, layout.bar_size).is_none() {
                let _ = unsafe { ClientToScreen(window, &mut point) };
                let ring = rect_of(layout.ring);
                let drag = Drag { pressed_at: point, ring_was_at: POINT { x: ring.left, y: ring.top } };
                with_frame(|frame| frame.drag = Some(drag));
                unsafe { SetCapture(window) };
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            if let Some(drag) = with_frame(|frame| frame.drag).flatten() {
                let mut point = signed_point(lparam);
                let _ = unsafe { ClientToScreen(window, &mut point) };
                let x = drag.ring_was_at.x + point.x - drag.pressed_at.x;
                let y = drag.ring_was_at.y + point.y - drag.pressed_at.y;
                let flags = SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE;
                let _ = unsafe { SetWindowPos(layout.ring, None, x, y, 0, 0, flags) };
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            if let Some(drag) = with_frame(|frame| frame.drag.take()).flatten() {
                let _ = unsafe { ReleaseCapture() };
                let mut point = signed_point(lparam);
                let at = point.x;
                let _ = unsafe { ClientToScreen(window, &mut point) };
                if (point.x, point.y) == (drag.pressed_at.x, drag.pressed_at.y) {
                    choose_side(layout, side_at(window, layout, at));
                }
            } else {
                match button_at(signed_point(lparam).x, layout.bar_size) {
                    Some(PanelButton::Stop) => start_now(layout),
                    Some(PanelButton::Cancel) => decide(None),
                    None => {}
                }
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let key = wparam.0 as u16;
            let typing = with_frame(|frame| frame.size_entry.is_some()).unwrap_or(false);
            match key {
                key if key == VK_RETURN.0 && typing => apply_typed_size(layout),
                key if key == VK_RETURN.0 => start_now(layout),
                key if key == VK_ESCAPE.0 && typing => stop_typing(layout),
                key if key == VK_ESCAPE.0 => decide(None),
                _ if typing => {}
                key => nudge_by_key(layout, key),
            }
            LRESULT(0)
        }
        WM_CHAR => {
            if let Some(key) = size_key_of(wparam.0 as u32) {
                type_size(layout, key);
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            decide(None);
            LRESULT(0)
        }
        _ => default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;
    use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_DOWN, VK_ESCAPE, VK_RETURN, VK_RIGHT};
    use windows::Win32::UI::WindowsAndMessaging::WM_CHAR;
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GetWindowRect, HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTLEFT, HTRIGHT, HTTOP, HTTOPLEFT,
        HTTOPRIGHT, PostMessageW, SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos, WM_KEYDOWN, WM_LBUTTONDOWN,
        WM_LBUTTONUP, WM_MOUSEMOVE,
    };

    use crate::desktop::{ONE_AT_A_TIME, PRIMARY};
    use crate::panel::stop_width;

    const SIZE: (i32, i32) = (516, 316);
    const MARGIN: i32 = 8;

    #[test]
    fn the_edges_of_the_ring_resize_that_side() {
        assert_eq!(ring_hit(3, 150, SIZE, MARGIN), HTLEFT);
        assert_eq!(ring_hit(512, 150, SIZE, MARGIN), HTRIGHT);
        assert_eq!(ring_hit(250, 2, SIZE, MARGIN), HTTOP);
        assert_eq!(ring_hit(250, 313, SIZE, MARGIN), HTBOTTOM);
    }

    #[test]
    fn near_a_corner_the_ring_resizes_both_sides() {
        assert_eq!(ring_hit(2, 2, SIZE, MARGIN), HTTOPLEFT);
        assert_eq!(ring_hit(20, 3, SIZE, MARGIN), HTTOPLEFT, "along the top, close to the corner");
        assert_eq!(ring_hit(3, 20, SIZE, MARGIN), HTTOPLEFT, "along the left, close to the corner");
        assert_eq!(ring_hit(513, 2, SIZE, MARGIN), HTTOPRIGHT);
        assert_eq!(ring_hit(2, 313, SIZE, MARGIN), HTBOTTOMLEFT);
        assert_eq!(ring_hit(513, 313, SIZE, MARGIN), HTBOTTOMRIGHT);
        assert_eq!(ring_hit(30, 3, SIZE, MARGIN), HTTOP, "far enough from the corner");
    }

    #[test]
    fn digits_x_and_backspace_are_the_keys_for_typing_a_size() {
        let keys: Vec<_> = "7xX* \t\u{8}a".chars().map(|character| size_key_of(character as u32)).collect();
        let next = Some(SizeKey::Next);
        assert_eq!(keys, [Some(SizeKey::Digit(7)), next, next, next, next, next, Some(SizeKey::Erase), None]);
    }

    #[test]
    fn the_hole_in_the_middle_is_not_part_of_the_frame() {
        assert_eq!(ring_hit(250, 150, SIZE, MARGIN), HTNOWHERE);
    }

    fn window_of(class: PCWSTR) -> HWND {
        (0..200)
            .find_map(|_| {
                thread::sleep(Duration::from_millis(10));
                unsafe { FindWindowW(class, None) }.ok()
            })
            .expect("the frame window appears")
    }

    fn rect_of(window: HWND) -> RECT {
        let mut rect = RECT::default();
        unsafe { GetWindowRect(window, &mut rect) }.unwrap();
        rect
    }

    fn post(window: HWND, message: u32, wparam: usize, x: i32, y: i32) {
        let point = LPARAM(((y as isize) << 16) | (x as isize & 0xFFFF));
        unsafe { PostMessageW(Some(window), message, WPARAM(wparam), point) }.unwrap();
    }

    fn with_the_frame(start: ScreenRect, act: impl FnOnce(HWND, HWND) + Send + 'static) -> Option<ScreenRect> {
        let hands = thread::spawn(move || {
            let _ = unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
            let (ring, bar) = (window_of(RING_CLASS), window_of(BAR_CLASS));
            act(ring, bar);
        });
        let chosen = adjust_frame(start).unwrap();
        hands.join().unwrap();
        chosen
    }

    fn start_button(bar: HWND) -> (i32, i32) {
        let shown = rect_of(bar);
        let (width, height) = (shown.right - shown.left, shown.bottom - shown.top);
        (width - height - stop_width(height) / 2, height / 2)
    }

    const START: ScreenRect = ScreenRect { monitor: PRIMARY, x: 400, y: 300, width: 500, height: 300 };

    #[test]
    #[ignore = "needs an unlocked desktop; run with --ignored"]
    fn the_frame_shows_around_the_region_and_start_gives_that_region() {
        let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let chosen = with_the_frame(START, |ring, bar| {
            let around = rect_of(ring);
            let monitor = crate::capture::monitor_bounds(PRIMARY).unwrap();
            let margin = monitor.left + 400 - around.left;
            assert!(margin > 0, "the ring lies outside the region: {around:?}");
            assert_eq!(around.right, monitor.left + 900 + margin, "{around:?}");
            assert_eq!((around.top, around.bottom), (monitor.top + 300 - margin, monitor.top + 600 + margin));
            assert!(rect_of(bar).top >= around.bottom, "the bar sits below the frame");
            let (x, y) = start_button(bar);
            post(bar, WM_LBUTTONUP, 0, x, y);
        });
        assert_eq!(chosen, Some(START));
    }

    #[test]
    #[ignore = "needs an unlocked desktop; run with --ignored"]
    fn cancel_escape_and_closing_from_elsewhere_give_nothing() {
        let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let by_button = with_the_frame(START, |_, bar| {
            let shown = rect_of(bar);
            let (width, height) = (shown.right - shown.left, shown.bottom - shown.top);
            post(bar, WM_LBUTTONUP, 0, width - height / 2, height / 2);
        });
        let by_escape = with_the_frame(START, |_, bar| post(bar, WM_KEYDOWN, usize::from(VK_ESCAPE.0), 0, 0));
        let from_elsewhere = with_the_frame(START, |_, _| crate::cancel_selection());
        assert_eq!((by_button, by_escape, from_elsewhere), (None, None, None));
    }

    #[test]
    #[ignore = "needs an unlocked desktop; run with --ignored"]
    fn arrow_keys_move_the_frame_and_enter_starts() {
        let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let chosen = with_the_frame(START, |_, bar| {
            for key in [VK_RIGHT, VK_RIGHT, VK_RIGHT, VK_DOWN] {
                post(bar, WM_KEYDOWN, usize::from(key.0), 0, 0);
            }
            post(bar, WM_KEYDOWN, usize::from(VK_RETURN.0), 0, 0);
        });
        assert_eq!(chosen, Some(ScreenRect { x: 403, y: 301, ..START }));
    }

    #[test]
    #[ignore = "needs an unlocked desktop; run with --ignored"]
    fn resizing_the_ring_changes_the_region_and_the_bar_follows() {
        let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let chosen = with_the_frame(START, |ring, bar| {
            let before = (rect_of(ring), rect_of(bar));
            let wider = RECT { right: before.0.right + 40, bottom: before.0.bottom + 20, ..before.0 };
            let (width, height) = (wider.right - wider.left, wider.bottom - wider.top);
            unsafe { SetWindowPos(ring, None, wider.left, wider.top, width, height, SWP_NOZORDER | SWP_NOACTIVATE) }.unwrap();
            thread::sleep(Duration::from_millis(100));
            let after = rect_of(bar);
            assert_eq!((after.left - before.1.left, after.top - before.1.top), (40, 20), "the bar follows the frame");
            post(bar, WM_KEYDOWN, usize::from(VK_RETURN.0), 0, 0);
        });
        assert_eq!(chosen, Some(ScreenRect { width: 540, height: 320, ..START }));
    }

    #[test]
    #[ignore = "needs an unlocked desktop; run with --ignored"]
    fn dragging_the_bar_moves_the_frame() {
        let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let chosen = with_the_frame(START, |_, bar| {
            post(bar, WM_LBUTTONDOWN, 1, 10, 10);
            post(bar, WM_MOUSEMOVE, 1, 60, 30);
            post(bar, WM_LBUTTONUP, 0, 60, 30);
            thread::sleep(Duration::from_millis(100));
            post(bar, WM_KEYDOWN, usize::from(VK_RETURN.0), 0, 0);
        });
        assert_eq!(chosen, Some(ScreenRect { x: 450, y: 320, ..START }));
    }

    #[test]
    #[ignore = "needs an unlocked desktop with a second monitor; run with --ignored"]
    fn a_frame_moved_onto_another_monitor_records_there_and_the_bar_goes_with_it() {
        let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let monitors = crate::capture::every_monitor().unwrap();
        let primary = monitors[PRIMARY.0 as usize];
        let Some(other) = monitors.iter().position(|&monitor| monitor != primary) else {
            return;
        };
        let target = monitors[other];
        let chosen = with_the_frame(START, move |ring, bar| {
            let margin = START.x - (rect_of(ring).left - primary.left);
            let (width, height) = (START.width as i32 + 2 * margin, START.height as i32 + 2 * margin);
            let (left, top) = (target.left + 100 - margin, target.top + 200 - margin);
            unsafe { SetWindowPos(ring, None, left, top, width, height, SWP_NOZORDER | SWP_NOACTIVATE) }.unwrap();
            thread::sleep(Duration::from_millis(100));
            let shown = rect_of(bar);
            let on_target = shown.left >= target.left && shown.right <= target.right && shown.top >= target.top && shown.bottom <= target.bottom;
            post(bar, WM_KEYDOWN, usize::from(VK_RETURN.0), 0, 0);
            assert!(on_target, "the bar {shown:?} stays with the frame on {target:?}");
        });
        assert_eq!(chosen, Some(ScreenRect { monitor: MonitorId(other as u32), x: 100, y: 200, ..START }));
    }

    fn wrongly_painted_ring_pixels(ring: HWND, margin: i32) -> Vec<(i32, i32, u32)> {
        use windows::Win32::UI::WindowsAndMessaging::WDA_NONE;
        let _ = unsafe { SetWindowDisplayAffinity(ring, WDA_NONE) };
        thread::sleep(Duration::from_millis(150));
        let at = rect_of(ring);
        let (width, height) = (at.right - at.left, at.bottom - at.top);
        let shown = screen_pixels(at);
        let amber = |x: i32, y: i32| {
            let (left, right, top, bottom) = (margin - THICKNESS, width - margin + THICKNESS, margin - THICKNESS, height - margin + THICKNESS);
            (left..right).contains(&x) && (top..bottom).contains(&y)
        };
        let mut wrong = Vec::new();
        for y in 0..height {
            for x in 0..width {
                if ring_hit(x, y, (width, height), margin) == HTNOWHERE {
                    continue;
                }
                let expected = if amber(x, y) { WAITING_AMBER } else { FRAME_DARK };
                let seen = shown[(y * width + x) as usize];
                if seen != expected.0 {
                    wrong.push((x, y, seen));
                }
            }
        }
        let _ = unsafe { SetWindowDisplayAffinity(ring, WDA_EXCLUDEFROMCAPTURE) };
        wrong
    }

    fn screen_pixels(area: RECT) -> Vec<u32> {
        use windows::Win32::Graphics::Gdi::{
            BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DIB_RGB_COLORS,
            DeleteDC, GetDC, GetDIBits, ReleaseDC, SRCCOPY, SelectObject,
        };
        let (width, height) = (area.right - area.left, area.bottom - area.top);
        let mut pixels = vec![0u32; (width * height) as usize];
        unsafe {
            let screen = GetDC(None);
            let copy = CreateCompatibleDC(Some(screen));
            let bitmap = CreateCompatibleBitmap(screen, width, height);
            let earlier = SelectObject(copy, bitmap.into());
            BitBlt(copy, 0, 0, width, height, Some(screen), area.left, area.top, SRCCOPY).unwrap();
            SelectObject(copy, earlier);
            let header = BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            };
            let mut info = BITMAPINFO { bmiHeader: header, ..Default::default() };
            GetDIBits(copy, bitmap, 0, height as u32, Some(pixels.as_mut_ptr().cast()), &mut info, DIB_RGB_COLORS);
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(copy);
            ReleaseDC(None, screen);
        }
        pixels.iter().map(|&bgra| ((bgra & 0xFF) << 16) | (bgra & 0xFF00) | ((bgra >> 16) & 0xFF)).collect()
    }

    #[test]
    #[ignore = "needs an unlocked desktop; run with --ignored"]
    fn stretching_the_ring_up_and_down_keeps_a_thin_amber_line() {
        let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let wrong = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = wrong.clone();
        with_the_frame(START, move |ring, bar| {
            let monitor = crate::capture::monitor_bounds(PRIMARY).unwrap();
            let margin = monitor.left + START.x - rect_of(ring).left;
            for step in [30, 30, 30, -20, -20, -20] {
                let was = rect_of(ring);
                let (width, height) = (was.right - was.left, was.bottom - was.top + step);
                unsafe { SetWindowPos(ring, None, was.left, was.top, width, height, SWP_NOZORDER | SWP_NOACTIVATE) }.unwrap();
                thread::sleep(Duration::from_millis(30));
            }
            for step in [30, 30, 30, -20, -20, -20] {
                let was = rect_of(ring);
                let (width, height) = (was.right - was.left, was.bottom - was.top + step);
                unsafe { SetWindowPos(ring, None, was.left, was.top - step, width, height, SWP_NOZORDER | SWP_NOACTIVATE) }.unwrap();
                thread::sleep(Duration::from_millis(30));
            }
            *seen.lock().unwrap() = wrongly_painted_ring_pixels(ring, margin);
            post(bar, WM_KEYDOWN, usize::from(VK_ESCAPE.0), 0, 0);
        });
        let wrong = wrong.lock().unwrap();
        assert!(wrong.is_empty(), "{} pixels painted wrong, first ones {:?}", wrong.len(), &wrong[..wrong.len().min(8)]);
    }

    fn type_text(bar: HWND, text: &str) {
        for character in text.chars() {
            post(bar, WM_CHAR, character as usize, 0, 0);
        }
    }

    fn click(window: HWND, (x, y): (i32, i32)) {
        post(window, WM_LBUTTONDOWN, 1, x, y);
        post(window, WM_LBUTTONUP, 0, x, y);
    }

    #[test]
    #[ignore = "needs an unlocked desktop; run with --ignored"]
    fn clicking_the_width_or_the_height_on_the_bar_lets_that_side_be_typed() {
        let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let chosen = with_the_frame(START, |_, bar| {
            let shown = rect_of(bar);
            let (width, height) = (shown.right - shown.left, shown.bottom - shown.top);
            let label_right = width - height - stop_width(height);
            let a_little = height * 3 / 8;
            click(bar, (label_right / 2 + a_little, height / 2));
            type_text(bar, "200");
            click(bar, (a_little / 2, height / 2));
            type_text(bar, "640");
            post(bar, WM_KEYDOWN, usize::from(VK_RETURN.0), 0, 0);
            post(bar, WM_KEYDOWN, usize::from(VK_RETURN.0), 0, 0);
        });
        assert_eq!(chosen, Some(ScreenRect { width: 640, height: 200, ..START }));
    }

    #[test]
    #[ignore = "needs an unlocked desktop; run with --ignored"]
    fn a_typed_size_resizes_the_frame_and_the_next_enter_starts() {
        let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let chosen = with_the_frame(START, |ring, bar| {
            let before = rect_of(ring);
            type_text(bar, "640x360");
            post(bar, WM_KEYDOWN, usize::from(VK_RETURN.0), 0, 0);
            thread::sleep(Duration::from_millis(100));
            let after = rect_of(ring);
            let grew = (after.right - after.left - (before.right - before.left), after.bottom - after.top - (before.bottom - before.top));
            assert_eq!(grew, (640 - 500, 360 - 300), "the frame takes the typed size before starting");
            assert_eq!((after.left, after.top), (before.left, before.top), "and stays where it was");
            post(bar, WM_KEYDOWN, usize::from(VK_RETURN.0), 0, 0);
        });
        assert_eq!(chosen, Some(ScreenRect { width: 640, height: 360, ..START }));
    }

    #[test]
    #[ignore = "needs an unlocked desktop; run with --ignored"]
    fn escape_while_typing_drops_the_typed_size_and_keeps_the_frame() {
        let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let chosen = with_the_frame(START, |_, bar| {
            type_text(bar, "640x360");
            post(bar, WM_KEYDOWN, usize::from(VK_ESCAPE.0), 0, 0);
            post(bar, WM_KEYDOWN, usize::from(VK_RIGHT.0), 0, 0);
            post(bar, WM_KEYDOWN, usize::from(VK_RETURN.0), 0, 0);
        });
        assert_eq!(chosen, Some(ScreenRect { x: 401, ..START }));
    }

    #[test]
    #[ignore = "needs an unlocked desktop; run with --ignored"]
    fn pressing_start_while_typing_uses_the_typed_size() {
        let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let chosen = with_the_frame(START, |_, bar| {
            type_text(bar, "800x");
            let (x, y) = start_button(bar);
            post(bar, WM_LBUTTONUP, 0, x, y);
        });
        assert_eq!(chosen, Some(ScreenRect { width: 800, ..START }));
    }
}
