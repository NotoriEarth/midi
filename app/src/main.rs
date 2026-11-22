#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod choice;
mod sample;
mod save;
mod screen;
mod settings;
mod tray;

use gif_core::fit::LEVELS;
use gif_core::hotkey::Hotkey;
use std::cell::Cell;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use gif_win::{HotkeyTaken, Hotkeys, KeyCombo, cancel_selection, local_time, monitor_at_cursor, monitor_size};
use tauri::ipc::Response;
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use tauri_plugin_notification::NotificationExt;

use choice::QualityChoice;
use screen::{ALREADY_RECORDING, Pick, Recorded, Recorder, RecordingEvent};
use settings::{RecordControl, RecordSettings, Selection};
use tray::{TrayAction, TrayState};

#[tauri::command]
fn quality_levels() -> Vec<(usize, u8)> {
    LEVELS.iter().map(|level| (level.colors, level.tolerance)).collect()
}

#[tauri::command(async)]
fn make_sample_gif(fps: u32, quality: QualityChoice) -> Result<Response, String> {
    let size_is_fixed = false;
    sample::encode(fps, quality.settings(size_is_fixed)?).map(Response::new)
}

const RECORDING_STARTED: &str = "recording-started";
const SELECTION_STARTED: &str = "selection-started";
const RECORDING_FINISHED: &str = "recording-finished";
const GIF_SAVED: &str = "gif-saved";
const GIF_ENCODED: &str = "gif-encoded";
const GIF_PROGRESS: &str = "gif-progress";
const SAVE_FOLDER: &str = "midi";
const BLACK_RECORDING: &str = "녹화된 화면이 전부 검은색입니다. 보호된 영상일 수 있습니다.";

#[derive(Default)]
struct LastSaved(Mutex<Option<PathBuf>>);

fn save_folder(app: &AppHandle) -> Result<PathBuf, String> {
    let pictures = app.path().picture_dir().map_err(|error| format!("사진 폴더를 찾지 못했습니다: {error}"))?;
    Ok(pictures.join(SAVE_FOLDER))
}

fn save_to_pictures(app: &AppHandle, gif: &[u8]) -> Result<PathBuf, String> {
    let folder = save_folder(app)?;
    save::save_gif(&folder, local_time(), gif)
        .map_err(|error| format!("{}에 저장하지 못했습니다: {error}", folder.display()))
}

fn open_in_explorer(argument: std::ffi::OsString) -> Result<(), String> {
    Command::new("explorer").arg(argument).spawn().map(drop).map_err(|error| format!("탐색기를 열지 못했습니다: {error}"))
}

#[tauri::command]
fn show_saved_gif(last: State<LastSaved>) -> Result<(), String> {
    let last = last.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let path = last.as_ref().ok_or("아직 저장한 GIF가 없습니다.")?;
    let mut select = std::ffi::OsString::from("/select,");
    select.push(path);
    open_in_explorer(select)
}

fn open_save_folder(app: &AppHandle) -> Result<(), String> {
    let folder = save_folder(app)?;
    std::fs::create_dir_all(&folder).map_err(|error| format!("{}을 만들지 못했습니다: {error}", folder.display()))?;
    open_in_explorer(folder.into_os_string())
}

const MAIN_WINDOW: &str = "main";

fn main_window_is_visible(app: &AppHandle) -> bool {
    app.get_webview_window(MAIN_WINDOW).and_then(|window| window.is_visible().ok()).unwrap_or(false)
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn notify(app: &AppHandle, title: &str, body: &str) {
    let _ = app.notification().builder().title(title).body(body).show();
}

fn notify_if_hidden(app: &AppHandle, title: &str, body: &str) {
    if !main_window_is_visible(app) {
        notify(app, title, body);
    }
}

#[derive(Default)]
struct ToldAboutTheTray(AtomicBool);

fn hide_to_tray(window: &tauri::Window) {
    let _ = window.hide();
    let app = window.app_handle();
    if !app.state::<ToldAboutTheTray>().0.swap(true, Ordering::Relaxed) {
        let start = Hotkey::DEFAULT_BEGIN_SELECTION;
        let body = format!("{start}나 트레이 아이콘 클릭으로 녹화를 시작합니다. 완전히 끄려면 트레이 메뉴의 [종료]를 누르세요.");
        notify(app, "midi는 트레이에서 계속 실행됩니다", &body);
    }
}

fn on_tray(app: &AppHandle, action: TrayAction) {
    match action {
        TrayAction::Click => {
            if !app.state::<Recorder>().stop() {
                start_recording(app, None);
            }
        }
        TrayAction::BeginSelection => start_recording(app, None),
        TrayAction::ShowWindow => show_main_window(app),
        TrayAction::OpenSaveFolder => {
            if let Err(problem) = open_save_folder(app) {
                notify(app, "저장 폴더를 열지 못했습니다", &problem);
            }
        }
        TrayAction::Quit => app.exit(0),
    }
}

struct TrayBackToIdle<'a>(&'a AppHandle);

impl Drop for TrayBackToIdle<'_> {
    fn drop(&mut self) {
        tray::show(self.0, TrayState::Idle);
    }
}

const SETTINGS_FILE: &str = "settings.json";
const REGION_FILE: &str = "region.json";

fn region_file(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|folder| folder.join(REGION_FILE))
}

fn pick_for(app: &AppHandle, settings: RecordSettings) -> Pick {
    let monitor = monitor_at_cursor();
    match settings.selection {
        Selection::Drag if settings.record_control == RecordControl::Button => Pick::DragThenFrame(monitor),
        Selection::Drag => Pick::Drag(monitor),
        Selection::Frame => {
            let last = region_file(app).and_then(|file| settings::load_region(&file));
            match monitor_size(monitor) {
                Ok(size) => Pick::Frame(gif_core::selection::frame_start(last, monitor, size)),
                Err(_) => Pick::Drag(monitor),
            }
        }
    }
}

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
        BEGIN_KEY => start_recording(app, None),
        STOP_KEY => {
            let _ = app.state::<Recorder>().stop();
        }
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

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
enum Finished {
    Saved { bytes: usize },
    Cancelled,
    Failed { problem: String },
}

#[derive(Default)]
struct LastGif(Mutex<Vec<u8>>);

#[tauri::command]
fn last_gif(last: State<LastGif>) -> Response {
    Response::new(last.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone())
}

#[tauri::command]
fn begin_recording(app: AppHandle, settings: RecordSettings) {
    start_recording(&app, Some(settings));
}

fn start_recording(app: &AppHandle, settings: Option<RecordSettings>) {
    let settings = settings.unwrap_or_else(|| saved_settings(app.clone()));
    let app = app.clone();
    std::thread::spawn(move || {
        let outcome = record_and_save(&app, settings);
        let finished = match outcome {
            Err(problem) if problem == ALREADY_RECORDING => return,
            Err(problem) => {
                notify_if_hidden(&app, "GIF를 만들지 못했습니다", &problem);
                Finished::Failed { problem }
            }
            Ok(None) => Finished::Cancelled,
            Ok(Some(gif)) => {
                let bytes = gif.len();
                *app.state::<LastGif>().0.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = gif;
                Finished::Saved { bytes }
            }
        };
        let _ = app.emit(RECORDING_FINISHED, finished);
    });
}

fn record_and_save(app: &AppHandle, settings: RecordSettings) -> Result<Option<Vec<u8>>, String> {
    let recorder = app.state::<Recorder>();
    let hotkeys = app.state::<Hotkeys>();
    let encode_settings = settings.encode_settings()?;
    let _tray = TrayBackToIdle(app);
    let _keys = KeysWhileRecording(&hotkeys);
    let _ = hotkeys.register(CANCEL_KEY, KeyCombo::ESCAPE);
    let recorded_size = Cell::new((0, 0));
    let percent_shown = Cell::new(None);
    let tell_the_page = |event| match event {
        RecordingEvent::Started(started) => {
            if let Some(file) = region_file(app) {
                let _ = settings::store_region(&file, started.region);
            }
            recorded_size.set(started.output_size);
            let _ = hotkeys.register(STOP_KEY, KeyCombo::ENTER);
            let region = (started.region.width, started.region.height);
            let limit = (started.limit_ms, started.limited_by);
            let _ = app.emit(RECORDING_STARTED, (region, started.output_size, limit.0, limit.1));
            tray::show(app, TrayState::Recording);
        }
        RecordingEvent::Encoding(step) => {
            let percent = (step.attempt, step.progress.done * 100 / step.progress.total.max(1));
            if percent_shown.replace(Some(percent)) != Some(percent) {
                let _ = app.emit(GIF_PROGRESS, percent);
                tray::show(app, TrayState::Encoding { attempt: percent.0, percent: percent.1 });
            }
        }
    };
    let pick = pick_for(app, settings);
    let how = match pick {
        Pick::Drag(_) => "drag",
        Pick::DragThenFrame(_) => "dragThenFrame",
        Pick::Frame(_) => "frame",
    };
    let _ = app.emit(SELECTION_STARTED, how);
    let recorded = recorder.record(settings.fps, encode_settings, settings.output_size(), pick, tell_the_page)?;
    let Some(Recorded { encoded, ended_at_limit, every_frame_black }) = recorded else {
        return Ok(None);
    };
    let final_size = (encoded.width, encoded.height);
    let made = (encoded.level, encoded.within_limit, recorded_size.get(), final_size, ended_at_limit, every_frame_black);
    let _ = app.emit(GIF_ENCODED, made);

    let gif = encoded.gif;
    let saved = save_to_pictures(app, &gif);
    let _ = app.emit(GIF_SAVED, saved.as_ref().map(|path| path.display().to_string()));
    match saved {
        Ok(path) => {
            let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            let mut body = format!("{name} ({}바이트). 트레이 메뉴의 [저장 폴더 열기]로 볼 수 있습니다.", gif.len());
            if every_frame_black {
                body = format!("{BLACK_RECORDING} {body}");
            }
            notify_if_hidden(app, "GIF를 저장했습니다", &body);
            *app.state::<LastSaved>().0.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(path);
        }
        Err(problem) => notify_if_hidden(app, "GIF를 저장하지 못했습니다", &problem),
    }
    Ok(Some(gif))
}

#[tauri::command]
fn stop_recording(recorder: State<Recorder>) {
    let _ = recorder.stop();
}

#[tauri::command]
fn cancel_recording(recorder: State<Recorder>) {
    recorder.cancel();
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .manage(Recorder::default())
        .manage(LastSaved::default())
        .manage(LastGif::default())
        .manage(ToldAboutTheTray::default())
        .setup(|app| {
            let handle = app.handle().clone();
            let hotkeys = Hotkeys::listen(move |id| on_hotkey(&handle, id));
            let begin = hotkeys.register(BEGIN_KEY, Hotkey::DEFAULT_BEGIN_SELECTION.into());
            app.manage(BeginHotkey(begin));
            app.manage(hotkeys);
            tray::install(app.handle(), on_tray)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                hide_to_tray(window);
            }
        })
        .invoke_handler(tauri::generate_handler![make_sample_gif, begin_recording, last_gif, stop_recording, cancel_recording, begin_hotkey, show_saved_gif, quality_levels, saved_settings, remember_settings])
        .run(tauri::generate_context!())
        .expect("the app window can be created");
}
