#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod sample;
mod screen;

use tauri::State;
use tauri::ipc::Response;

use screen::Recorder;

#[tauri::command(async)]
fn make_sample_gif(fps: u32, colors: usize, dither: f32) -> Result<Response, String> {
    sample::encode(fps, colors, dither).map(Response::new)
}

const CANCELLED: Vec<u8> = Vec::new();

#[tauri::command(async)]
fn record_screen_gif(recorder: State<Recorder>, fps: u32, colors: usize, dither: f32) -> Result<Response, String> {
    let gif = recorder.record(fps, colors, dither)?;
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
        .invoke_handler(tauri::generate_handler![make_sample_gif, record_screen_gif, stop_recording, cancel_recording])
        .run(tauri::generate_context!())
        .expect("the app window can be created");
}
