use std::sync::Arc;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::AppHandle;

use gif_core::hotkey::Hotkey;

const TRAY_ID: &str = "main";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayState {
    Idle,
    Recording,
    Encoding { attempt: usize, percent: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    Click,
    BeginSelection,
    ShowWindow,
    OpenSaveFolder,
    Quit,
}

const MENU: [(&str, &str, TrayAction); 4] = [
    ("begin", "영역 골라 녹화", TrayAction::BeginSelection),
    ("show", "창 열기", TrayAction::ShowWindow),
    ("folder", "저장 폴더 열기", TrayAction::OpenSaveFolder),
    ("quit", "종료", TrayAction::Quit),
];

pub const START_HIDDEN: &str = "--hidden";

pub fn starts_hidden<T: AsRef<str>>(arguments: impl IntoIterator<Item = T>) -> bool {
    arguments.into_iter().any(|argument| argument.as_ref() == START_HIDDEN)
}

pub fn how_to_start(hotkey: Option<Hotkey>) -> String {
    match hotkey {
        Some(hotkey) => format!("{hotkey}나 트레이 아이콘 클릭으로 녹화를 시작합니다."),
        None => "트레이 아이콘 클릭으로 녹화를 시작합니다.".to_string(),
    }
}

pub fn hotkey_taken_notice(hotkey: Hotkey) -> String {
    format!("{hotkey}를 다른 프로그램이 쓰고 있어서 등록하지 못했습니다. 트레이 아이콘을 클릭해 녹화를 시작하세요.")
}

pub fn tooltip(state: TrayState) -> String {
    match state {
        TrayState::Idle => "midi: 클릭하면 녹화할 영역을 고릅니다".to_string(),
        TrayState::Recording => "midi: 녹화하는 중 (클릭하면 끝냅니다)".to_string(),
        TrayState::Encoding { attempt: 1, percent } => format!("midi: GIF 만드는 중 {percent}%"),
        TrayState::Encoding { attempt, percent } => {
            format!("midi: GIF 만드는 중 {percent}% (용량 목표에 맞추는 중, {attempt}번째 시도)")
        }
    }
}

fn action_of(menu_id: &str) -> Option<TrayAction> {
    MENU.iter().find(|(id, _, _)| *id == menu_id).map(|&(_, _, action)| action)
}

pub fn install(app: &AppHandle, on_action: impl Fn(&AppHandle, TrayAction) + Send + Sync + 'static) -> tauri::Result<()> {
    let on_action = Arc::new(on_action);
    let item = |id: &str, label: &str| MenuItem::with_id(app, id, label, true, None::<&str>);
    let [begin, show, folder, quit] = MENU.map(|(id, label, _)| item(id, label));
    let menu = Menu::with_items(app, &[&begin?, &show?, &folder?, &PredefinedMenuItem::separator(app)?, &quit?])?;

    let on_menu = Arc::clone(&on_action);
    let on_click = on_action;
    let mut tray = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(tooltip(TrayState::Idle))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| {
            if let Some(action) = action_of(event.id().as_ref()) {
                on_menu(app, action);
            }
        })
        .on_tray_icon_event(move |tray, event| {
            if matches!(event, TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }) {
                on_click(tray.app_handle(), TrayAction::Click);
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

pub fn show(app: &AppHandle, state: TrayState) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(tooltip(state)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOWS_TOOLTIP_LIMIT: usize = 127;

    #[test]
    fn an_idle_tray_shows_the_app_name_and_how_to_start() {
        assert_eq!(tooltip(TrayState::Idle), "midi: 클릭하면 녹화할 영역을 고릅니다");
    }

    #[test]
    fn a_recording_says_so_and_how_to_end_it() {
        assert_eq!(tooltip(TrayState::Recording), "midi: 녹화하는 중 (클릭하면 끝냅니다)");
    }

    #[test]
    fn encoding_shows_how_far_it_has_got() {
        assert_eq!(tooltip(TrayState::Encoding { attempt: 1, percent: 45 }), "midi: GIF 만드는 중 45%");
    }

    #[test]
    fn a_later_attempt_toward_a_size_limit_says_which_one() {
        let third = TrayState::Encoding { attempt: 3, percent: 7 };
        assert_eq!(tooltip(third), "midi: GIF 만드는 중 7% (용량 목표에 맞추는 중, 3번째 시도)");
    }

    #[test]
    fn every_tooltip_fits_what_windows_shows() {
        let longest = TrayState::Encoding { attempt: 6, percent: 100 };
        for state in [TrayState::Idle, TrayState::Recording, longest] {
            assert!(tooltip(state).encode_utf16().count() <= WINDOWS_TOOLTIP_LIMIT, "{state:?}");
        }
    }

    #[test]
    fn every_menu_item_leads_to_its_own_action() {
        let actions: Vec<_> = MENU.iter().map(|(id, _, _)| action_of(id)).collect();
        let expected = [TrayAction::BeginSelection, TrayAction::ShowWindow, TrayAction::OpenSaveFolder, TrayAction::Quit];
        assert_eq!(actions, expected.map(Some));
        assert_eq!(action_of("something else"), None);
    }

    #[test]
    fn the_app_starts_hidden_only_when_asked_to() {
        assert!(starts_hidden(["midi-app.exe", "--hidden"]));
        assert!(starts_hidden(["midi-app.exe", "--something", "--hidden"]));
        assert!(!starts_hidden(["midi-app.exe"]));
        assert!(!starts_hidden(["midi-app.exe", "--hiddenness"]));
        assert!(!starts_hidden(Vec::<String>::new()));
    }

    #[test]
    fn the_ways_to_start_leave_out_a_hotkey_that_could_not_be_registered() {
        let hotkey = Hotkey::DEFAULT_BEGIN_SELECTION;
        assert_eq!(how_to_start(Some(hotkey)), "Win+Shift+G나 트레이 아이콘 클릭으로 녹화를 시작합니다.");
        assert_eq!(how_to_start(None), "트레이 아이콘 클릭으로 녹화를 시작합니다.");
    }

    #[test]
    fn a_taken_hotkey_is_named_along_with_what_to_do_instead() {
        assert_eq!(
            hotkey_taken_notice(Hotkey::DEFAULT_BEGIN_SELECTION),
            "Win+Shift+G를 다른 프로그램이 쓰고 있어서 등록하지 못했습니다. 트레이 아이콘을 클릭해 녹화를 시작하세요."
        );
    }
}
