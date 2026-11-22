#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod sample;
mod screen;

use gif_core::hotkey::Hotkey;
use gif_core::session::ScreenRect;
use gif_win::{HotkeyTaken, Hotkeys, KeyCombo, cancel_selection};
use tauri::ipc::Response;
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};

use screen::Recorder;

#[tauri::command(async)]
fn make_sample_gif(fps: u32, colors: usize, dither: f32) -> Result<Response, String> {
    sample::encode(fps, colors, dither).map(Response::new)
}

const CANCELLED: Vec<u8> = Vec::new();
const RECORDING_STARTED: &str = "recording-started";
const BEGIN_SELECTION: &str = "begin-selection";

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
    fps: u32,
    colors: usize,
    dither: f32,
) -> Result<Response, String> {
    let _keys = KeysWhileRecording(&hotkeys);
    let _ = hotkeys.register(CANCEL_KEY, KeyCombo::ESCAPE);
    let tell_the_page = |region: ScreenRect| {
        let _ = hotkeys.register(STOP_KEY, KeyCombo::ENTER);
        let _ = window.emit(RECORDING_STARTED, (region.width, region.height));
    };
    let gif = recorder.record(fps, colors, dither, tell_the_page)?;
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
        .setup(|app| {
            let handle = app.handle().clone();
            let hotkeys = Hotkeys::listen(move |id| on_hotkey(&handle, id));
            let begin = hotkeys.register(BEGIN_KEY, Hotkey::DEFAULT_BEGIN_SELECTION.into());
            app.manage(BeginHotkey(begin));
            app.manage(hotkeys);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![make_sample_gif, record_screen_gif, stop_recording, cancel_recording, begin_hotkey])
        .run(tauri::generate_context!())
        .expect("the app window can be created");
}
