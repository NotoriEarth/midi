#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod choice;
mod sample;
mod save;
mod screen;
mod settings;

use gif_core::fit::LEVELS;
use gif_core::hotkey::Hotkey;
use std::cell::Cell;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;

use gif_win::{HotkeyTaken, Hotkeys, KeyCombo, cancel_selection, local_time};
use tauri::ipc::Response;
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};

use choice::QualityChoice;
use screen::{Recorded, Recorder, Started};
use settings::RecordSettings;

#[tauri::command]
fn quality_levels() -> Vec<(usize, u8)> {
    LEVELS.iter().map(|level| (level.colors, level.tolerance)).collect()
}

#[tauri::command(async)]
fn make_sample_gif(fps: u32, quality: QualityChoice) -> Result<Response, String> {
    let size_is_fixed = false;
    sample::encode(fps, quality.settings(size_is_fixed)?).map(Response::new)
}

const CANCELLED: Vec<u8> = Vec::new();
const RECORDING_STARTED: &str = "recording-started";
const BEGIN_SELECTION: &str = "begin-selection";
const GIF_SAVED: &str = "gif-saved";
const GIF_ENCODED: &str = "gif-encoded";
const SAVE_FOLDER: &str = "midi";

#[derive(Default)]
struct LastSaved(Mutex<Option<PathBuf>>);

fn save_to_pictures(app: &AppHandle, gif: &[u8]) -> Result<PathBuf, String> {
    let pictures = app.path().picture_dir().map_err(|error| format!("사진 폴더를 찾지 못했습니다: {error}"))?;
    let folder = pictures.join(SAVE_FOLDER);
    save::save_gif(&folder, local_time(), gif)
        .map_err(|error| format!("{}에 저장하지 못했습니다: {error}", folder.display()))
}

#[tauri::command]
fn show_saved_gif(last: State<LastSaved>) -> Result<(), String> {
    let last = last.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let path = last.as_ref().ok_or("아직 저장한 GIF가 없습니다.")?;
    let mut select = std::ffi::OsString::from("/select,");
    select.push(path);
    Command::new("explorer").arg(select).spawn().map(drop).map_err(|error| format!("탐색기를 열지 못했습니다: {error}"))
}

const SETTINGS_FILE: &str = "settings.json";

fn settings_file(app: &AppHandle) -> Result<PathBuf, String> {
    let folder = app.path().app_config_dir().map_err(|error| format!("설정 폴더를 찾지 못했습니다: {error}"))?;
    Ok(folder.join(SETTINGS_FILE))
}

#[tauri::command]
fn saved_settings(app: AppHandle) -> RecordSettings {
    settings_file(&app).map(|file| settings::load(&file)).unwrap_or_default()
}

#[tauri::command]
fn remember_settings(app: AppHandle, settings: RecordSettings) -> Result<(), String> {
    if !settings.can_record() {
        return Ok(());
    }
    let file = settings_file(&app)?;
    settings::store(&file, settings).map_err(|error| format!("설정을 {}에 저장하지 못했습니다: {error}", file.display()))
}

const BEGIN_KEY: u32 = 1;
const STOP_KEY: u32 = 2;
const CANCEL_KEY: u32 = 3;

struct BeginHotkey(Result<(), HotkeyTaken>);

struct KeysWhileRecording<'a>(&'a Hotkeys);

impl Drop for KeysWhileRecording<'_> {
    fn drop(&mut self) {
        self.0.unregister(STOP_KEY);
        self.0.unregister(CANCEL_KEY);
    }
}

fn on_hotkey(app: &AppHandle, id: u32) {
    match id {
        BEGIN_KEY => {
            let _ = app.emit(BEGIN_SELECTION, ());
        }
        STOP_KEY => app.state::<Recorder>().stop(),
        CANCEL_KEY => {
            app.state::<Recorder>().cancel();
            cancel_selection();
        }
        _ => {}
    }
}

#[tauri::command]
fn begin_hotkey(begin: State<BeginHotkey>) -> Result<String, String> {
    let hotkey = Hotkey::DEFAULT_BEGIN_SELECTION;
    begin.0.map(|()| hotkey.to_string()).map_err(|HotkeyTaken| {
        format!("{hotkey} 단축키를 다른 프로그램이 쓰고 있어서 등록하지 못했습니다. [녹화 시작] 버튼을 쓰세요.")
    })
}

#[tauri::command(async)]
fn record_screen_gif(
    window: WebviewWindow,
    recorder: State<Recorder>,
    hotkeys: State<Hotkeys>,
    last: State<LastSaved>,
    settings: RecordSettings,
) -> Result<Response, String> {
    let _keys = KeysWhileRecording(&hotkeys);
    let _ = hotkeys.register(CANCEL_KEY, KeyCombo::ESCAPE);
    let recorded_size = Cell::new((0, 0));
    let tell_the_page = |started: Started| {
        recorded_size.set(started.output_size);
        let _ = hotkeys.register(STOP_KEY, KeyCombo::ENTER);
        let region = (started.region.width, started.region.height);
        let _ = window.emit(RECORDING_STARTED, (region, started.output_size, started.limit_ms, started.limited_by));
    };
    let recorded = recorder.record(settings.fps, settings.encode_settings()?, settings.output_size(), tell_the_page)?;
    if let Some(Recorded { encoded, ended_at_limit }) = &recorded {
        let final_size = (encoded.width, encoded.height);
        let made = (encoded.level, encoded.within_limit, recorded_size.get(), final_size, ended_at_limit);
        let _ = window.emit(GIF_ENCODED, made);
    }
    let gif = recorded.map(|recorded| recorded.encoded.gif);
    if let Some(gif) = &gif {
        let saved = save_to_pictures(window.app_handle(), gif);
        let _ = window.emit(GIF_SAVED, saved.as_ref().map(|path| path.display().to_string()));
        if let Ok(path) = saved {
            *last.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(path);
        }
    }
    Ok(Response::new(gif.unwrap_or(CANCELLED)))
}

#[tauri::command]
fn stop_recording(recorder: State<Recorder>) {
    recorder.stop();
}

#[tauri::command]
fn cancel_recording(recorder: State<Recorder>) {
    recorder.cancel();
}

fn main() {
    tauri::Builder::default()
        .manage(Recorder::default())
        .manage(LastSaved::default())
        .setup(|app| {
            let handle = app.handle().clone();
            let hotkeys = Hotkeys::listen(move |id| on_hotkey(&handle, id));
            let begin = hotkeys.register(BEGIN_KEY, Hotkey::DEFAULT_BEGIN_SELECTION.into());
            app.manage(BeginHotkey(begin));
            app.manage(hotkeys);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![make_sample_gif, record_screen_gif, stop_recording, cancel_recording, begin_hotkey, show_saved_gif, quality_levels, saved_settings, remember_settings])
        .run(tauri::generate_context!())
        .expect("the app window can be created");
}
