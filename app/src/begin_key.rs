use gif_core::hotkey::{Hotkey, HotkeyError};
use gif_win::HotkeyTaken;

pub fn change(
    from: Hotkey,
    to: Hotkey,
    mut register: impl FnMut(Hotkey) -> Result<(), HotkeyTaken>,
    mut unregister: impl FnMut(),
) -> Result<(), String> {
    if from == to {
        return Ok(());
    }
    unregister();
    register(to).map_err(|HotkeyTaken| {
        let _ = register(from);
        taken(to)
    })
}

pub fn explain(error: HotkeyError) -> String {
    match error {
        HotkeyError::MissingModifier => "Ctrl, Alt, Shift, Win 중 하나 이상과 함께 눌러야 합니다.".to_string(),
        HotkeyError::MissingKey => "글자, 숫자, F1~F24 중 하나를 함께 눌러야 합니다.".to_string(),
        HotkeyError::MoreThanOneKey => "Ctrl, Alt, Shift, Win 말고는 키를 하나만 쓸 수 있습니다.".to_string(),
        HotkeyError::UnknownKey(key) => format!("{key} 키는 단축키로 쓸 수 없습니다. 글자, 숫자, F1~F24 중에서 고르세요."),
    }
}

pub fn taken(hotkey: Hotkey) -> String {
    format!("{hotkey} 단축키를 다른 프로그램이 쓰고 있어서 등록하지 못했습니다.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use gif_core::hotkey::Key;

    const CTRL_ALT_G: Hotkey = Hotkey { win: false, ctrl: true, alt: true, shift: false, key: Key::Letter('G') };
    const WIN_SHIFT_G: Hotkey = Hotkey::DEFAULT_BEGIN_SELECTION;

    #[derive(Debug, PartialEq)]
    enum Call {
        Register(Hotkey),
        Unregister,
    }

    fn calls_for(from: Hotkey, to: Hotkey, taken: Option<Hotkey>) -> (Result<(), String>, Vec<Call>) {
        let calls = std::cell::RefCell::new(Vec::new());
        let register = |hotkey: Hotkey| {
            calls.borrow_mut().push(Call::Register(hotkey));
            if Some(hotkey) == taken { Err(HotkeyTaken) } else { Ok(()) }
        };
        let unregister = || calls.borrow_mut().push(Call::Unregister);
        let changed = change(from, to, register, unregister);
        (changed, calls.into_inner())
    }

    #[test]
    fn changing_lets_go_of_the_old_hotkey_and_takes_the_new_one() {
        let (changed, calls) = calls_for(WIN_SHIFT_G, CTRL_ALT_G, None);
        assert_eq!(changed, Ok(()));
        assert_eq!(calls, [Call::Unregister, Call::Register(CTRL_ALT_G)]);
    }

    #[test]
    fn a_new_hotkey_another_program_holds_leaves_the_old_one_working() {
        let (changed, calls) = calls_for(WIN_SHIFT_G, CTRL_ALT_G, Some(CTRL_ALT_G));
        assert_eq!(changed, Err(taken(CTRL_ALT_G)));
        assert_eq!(calls, [Call::Unregister, Call::Register(CTRL_ALT_G), Call::Register(WIN_SHIFT_G)]);
    }

    #[test]
    fn choosing_the_same_hotkey_again_touches_nothing() {
        let (changed, calls) = calls_for(WIN_SHIFT_G, WIN_SHIFT_G, None);
        assert_eq!(changed, Ok(()));
        assert_eq!(calls, []);
    }

    #[test]
    fn a_hotkey_that_cannot_be_read_says_what_is_missing() {
        assert_eq!(explain(HotkeyError::MissingModifier), "Ctrl, Alt, Shift, Win 중 하나 이상과 함께 눌러야 합니다.");
        assert_eq!(explain(HotkeyError::MissingKey), "글자, 숫자, F1~F24 중 하나를 함께 눌러야 합니다.");
        assert_eq!(explain(HotkeyError::MoreThanOneKey), "Ctrl, Alt, Shift, Win 말고는 키를 하나만 쓸 수 있습니다.");
        assert_eq!(explain(HotkeyError::UnknownKey("Tab".to_string())), "Tab 키는 단축키로 쓸 수 없습니다. 글자, 숫자, F1~F24 중에서 고르세요.");
    }
}
