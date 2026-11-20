use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Letter(char),
    Digit(u8),
    Function(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hotkey {
    pub win: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: Key,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyError {
    MissingKey,
    MissingModifier,
    MoreThanOneKey,
    UnknownKey(String),
}

impl Hotkey {
    pub const DEFAULT_BEGIN_SELECTION: Hotkey = Hotkey {
        win: true,
        ctrl: false,
        alt: false,
        shift: true,
        key: Key::Letter('G'),
    };
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Key::Letter(letter) => write!(f, "{letter}"),
            Key::Digit(digit) => write!(f, "{digit}"),
            Key::Function(number) => write!(f, "F{number}"),
        }
    }
}

impl fmt::Display for Hotkey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let modifiers = [
            (self.win, "Win"),
            (self.ctrl, "Ctrl"),
            (self.alt, "Alt"),
            (self.shift, "Shift"),
        ];
        for (held, name) in modifiers {
            if held {
                write!(f, "{name}+")?;
            }
        }
        write!(f, "{}", self.key)
    }
}

impl FromStr for Key {
    type Err = HotkeyError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let unknown = || HotkeyError::UnknownKey(text.to_string());
        let upper = text.to_ascii_uppercase();
        let mut chars = upper.chars();

        match (chars.next(), chars.as_str()) {
            (Some(letter @ 'A'..='Z'), "") => Ok(Key::Letter(letter)),
            (Some(digit @ '0'..='9'), "") => Ok(Key::Digit(digit as u8 - b'0')),
            (Some('F'), number) => match number.parse() {
                Ok(number @ 1..=24) => Ok(Key::Function(number)),
                _ => Err(unknown()),
            },
            _ => Err(unknown()),
        }
    }
}

impl FromStr for Hotkey {
    type Err = HotkeyError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let (mut win, mut ctrl, mut alt, mut shift) = (false, false, false, false);
        let mut key = None;

        for part in text.split('+').map(str::trim).filter(|part| !part.is_empty()) {
            match part.to_ascii_lowercase().as_str() {
                "win" => win = true,
                "ctrl" => ctrl = true,
                "alt" => alt = true,
                "shift" => shift = true,
                _ if key.is_some() => return Err(HotkeyError::MoreThanOneKey),
                _ => key = Some(part.parse()?),
            }
        }

        let key = key.ok_or(HotkeyError::MissingKey)?;
        if !(win || ctrl || alt || shift) {
            return Err(HotkeyError::MissingModifier);
        }
        Ok(Hotkey { win, ctrl, alt, shift, key })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Hotkey, HotkeyError> {
        text.parse()
    }

    #[test]
    fn the_default_is_win_shift_g() {
        assert_eq!(Hotkey::DEFAULT_BEGIN_SELECTION.to_string(), "Win+Shift+G");
    }

    #[test]
    fn what_is_saved_reads_back_as_the_same_hotkey() {
        for text in ["Win+Shift+G", "Ctrl+Alt+F9", "Win+Ctrl+Alt+Shift+7"] {
            assert_eq!(parse(text).map(|hotkey| hotkey.to_string()), Ok(text.to_string()));
        }
    }

    #[test]
    fn case_spacing_and_modifier_order_do_not_matter() {
        assert_eq!(parse(" shift + WIN + g "), Ok(Hotkey::DEFAULT_BEGIN_SELECTION));
    }

    #[test]
    fn a_key_without_a_modifier_is_rejected() {
        assert_eq!(parse("G"), Err(HotkeyError::MissingModifier));
    }

    #[test]
    fn modifiers_without_a_key_are_rejected() {
        assert_eq!(parse("Ctrl+Shift"), Err(HotkeyError::MissingKey));
        assert_eq!(parse(""), Err(HotkeyError::MissingKey));
    }

    #[test]
    fn two_keys_are_rejected() {
        assert_eq!(parse("Ctrl+A+B"), Err(HotkeyError::MoreThanOneKey));
    }

    #[test]
    fn enter_and_escape_cannot_be_chosen() {
        assert_eq!(parse("Ctrl+Enter"), Err(HotkeyError::UnknownKey("Enter".into())));
        assert_eq!(parse("Ctrl+Esc"), Err(HotkeyError::UnknownKey("Esc".into())));
    }

    #[test]
    fn function_keys_stop_at_f24() {
        assert_eq!(parse("Ctrl+F24").map(|hotkey| hotkey.key), Ok(Key::Function(24)));
        assert_eq!(parse("Ctrl+F25"), Err(HotkeyError::UnknownKey("F25".into())));
        assert_eq!(parse("Ctrl+F0"), Err(HotkeyError::UnknownKey("F0".into())));
    }
}
