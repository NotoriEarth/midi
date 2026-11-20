#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod sample;
mod screen;

use tauri::ipc::Response;

#[tauri::command(async)]
fn make_sample_gif(fps: u32, colors: usize, dither: f32) -> Result<Response, String> {
    sample::encode(fps, colors, dither).map(Response::new)
}

#[tauri::command(async)]
fn record_screen_gif(fps: u32, colors: usize, dither: f32, seconds: u32) -> Result<Response, String> {
    screen::record(fps, colors, dither, seconds).map(Response::new)
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![make_sample_gif, record_screen_gif])
        .run(tauri::generate_context!())
        .expect("the app window can be created");
}
