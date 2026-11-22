use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::{self, JoinHandle};

use gif_core::hotkey::{Hotkey, Key};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, RegisterHotKey, UnregisterHotKey,
    VK_ESCAPE, VK_F1, VK_RETURN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetMessageW, MSG, PM_NOREMOVE, PeekMessageW, PostThreadMessageW, WM_APP, WM_HOTKEY, WM_QUIT,
};

const REQUEST_WAITING: u32 = WM_APP;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyCombo {
    modifiers: u32,
    virtual_key: u32,
}

impl KeyCombo {
    pub const ENTER: KeyCombo = KeyCombo { modifiers: 0, virtual_key: VK_RETURN.0 as u32 };
    pub const ESCAPE: KeyCombo = KeyCombo { modifiers: 0, virtual_key: VK_ESCAPE.0 as u32 };
}

impl From<Hotkey> for KeyCombo {
    fn from(hotkey: Hotkey) -> KeyCombo {
        let held = [
            (hotkey.win, MOD_WIN),
            (hotkey.ctrl, MOD_CONTROL),
            (hotkey.alt, MOD_ALT),
            (hotkey.shift, MOD_SHIFT),
        ];
        let modifiers = held.iter().filter(|(held, _)| *held).fold(0, |bits, (_, modifier)| bits | modifier.0);
        let virtual_key = match hotkey.key {
            Key::Letter(letter) => u32::from(letter.to_ascii_uppercase()),
            Key::Digit(digit) => u32::from(b'0' + digit),
            Key::Function(number) => u32::from(VK_F1.0) + u32::from(number) - 1,
        };
        KeyCombo { modifiers, virtual_key }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HotkeyTaken;

enum Request {
    Register { id: u32, combo: KeyCombo },
    Unregister { id: u32 },
}

struct Line {
    requests: Sender<Request>,
    registered: Receiver<Result<(), HotkeyTaken>>,
}

pub struct Hotkeys {
    thread_id: u32,
    thread: Option<JoinHandle<()>>,
    line: Mutex<Line>,
}

impl Hotkeys {
    pub fn listen(on_press: impl Fn(u32) + Send + 'static) -> Hotkeys {
        let (requests, incoming) = channel();
        let (answers, registered) = channel();
        let (ready, started) = channel();

        let thread = thread::spawn(move || {
            let mut message = MSG::default();
            unsafe {
                let _ = PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);
                let _ = ready.send(GetCurrentThreadId());
            }
            let mut held: Vec<u32> = Vec::new();

            while unsafe { GetMessageW(&mut message, None, 0, 0) }.0 > 0 {
                match message.message {
                    WM_HOTKEY => on_press(message.wParam.0 as u32),
                    REQUEST_WAITING => {
                        for request in incoming.try_iter() {
                            match request {
                                Request::Register { id, combo } => {
                                    let modifiers = HOT_KEY_MODIFIERS(combo.modifiers) | MOD_NOREPEAT;
                                    let done = unsafe { RegisterHotKey(None, id as i32, modifiers, combo.virtual_key) };
                                    if done.is_ok() {
                                        held.push(id);
                                    }
                                    let _ = answers.send(done.map_err(|_| HotkeyTaken));
                                }
                                Request::Unregister { id } => {
                                    held.retain(|&other| other != id);
                                    let _ = unsafe { UnregisterHotKey(None, id as i32) };
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            for id in held {
                let _ = unsafe { UnregisterHotKey(None, id as i32) };
            }
        });

        let thread_id = started.recv().expect("the hotkey thread starts");
        Hotkeys { thread_id, thread: Some(thread), line: Mutex::new(Line { requests, registered }) }
    }

    pub fn register(&self, id: u32, combo: KeyCombo) -> Result<(), HotkeyTaken> {
        let line = self.line.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        self.ask(&line, Request::Register { id, combo });
        line.registered.recv().unwrap_or(Err(HotkeyTaken))
    }

    pub fn unregister(&self, id: u32) {
        let line = self.line.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        self.ask(&line, Request::Unregister { id });
    }

    fn ask(&self, line: &Line, request: Request) {
        let _ = line.requests.send(request);
        self.post(REQUEST_WAITING, 0);
    }

    fn post(&self, message: u32, wparam: usize) {
        let _ = unsafe { PostThreadMessageW(self.thread_id, message, WPARAM(wparam), LPARAM(0)) };
    }
}

impl Drop for Hotkeys {
    fn drop(&mut self) {
        self.post(WM_QUIT, 0);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn combo(text: &str) -> KeyCombo {
        text.parse::<Hotkey>().unwrap().into()
    }

    #[test]
    fn the_default_hotkey_is_win_shift_and_the_letter_g() {
        let expected = KeyCombo { modifiers: MOD_WIN.0 | MOD_SHIFT.0, virtual_key: 0x47 };
        assert_eq!(KeyCombo::from(Hotkey::DEFAULT_BEGIN_SELECTION), expected);
    }

    #[test]
    fn every_modifier_has_its_own_bit() {
        assert_eq!(combo("Ctrl+A").modifiers, MOD_CONTROL.0);
        assert_eq!(combo("Alt+A").modifiers, MOD_ALT.0);
        assert_eq!(combo("Ctrl+Alt+Shift+A").modifiers, MOD_CONTROL.0 | MOD_ALT.0 | MOD_SHIFT.0);
    }

    #[test]
    fn letters_digits_and_function_keys_map_to_their_virtual_keys() {
        assert_eq!(combo("Ctrl+Z").virtual_key, 0x5A);
        assert_eq!(combo("Ctrl+7").virtual_key, 0x37);
        assert_eq!(combo("Ctrl+F1").virtual_key, 0x70);
        assert_eq!(combo("Ctrl+F12").virtual_key, 0x7B);
    }

    #[test]
    fn a_press_reaches_the_listener_with_the_id_it_was_registered_under() {
        let (pressed, presses) = channel();
        let hotkeys = Hotkeys::listen(move |id| pressed.send(id).unwrap());

        hotkeys.post(WM_HOTKEY, 7);

        assert_eq!(presses.recv_timeout(Duration::from_secs(2)), Ok(7));
    }

    #[test]
    #[ignore = "needs an unlocked desktop; run with --ignored"]
    fn a_combination_can_be_held_by_only_one_listener_until_it_lets_go() {
        let rare = combo("Ctrl+Alt+Shift+F11");
        let first = Hotkeys::listen(|_| {});
        let second = Hotkeys::listen(|_| {});

        assert_eq!(first.register(1, rare), Ok(()));
        assert_eq!(second.register(1, rare), Err(HotkeyTaken));

        first.unregister(1);
        assert_eq!(second.register(1, rare), Ok(()));
    }

    #[test]
    #[ignore = "needs an unlocked desktop; run with --ignored"]
    fn dropping_the_listener_lets_go_of_its_combinations() {
        let rare = combo("Ctrl+Alt+Shift+F10");
        let first = Hotkeys::listen(|_| {});
        assert_eq!(first.register(1, rare), Ok(()));
        drop(first);

        let second = Hotkeys::listen(|_| {});
        assert_eq!(second.register(1, rare), Ok(()));
    }
}
