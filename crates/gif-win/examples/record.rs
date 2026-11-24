use std::time::Instant;

use gif_core::GifFps;
use gif_core::encode::{ManualSettings, encode_gif};
use gif_core::session::{MonitorId, ScreenRect};
use gif_core::writer::Rgb;
use gif_win::{ScreenCapturer, record_screen};

const WIDTH: u32 = 480;
const HEIGHT: u32 = 270;
const SECONDS: u32 = 3;
const FPS: GifFps = GifFps::Fps15;

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "record.gif".to_string());
    let monitor = MonitorId(0);
    let region = ScreenRect { monitor, x: 0, y: 0, width: WIDTH, height: HEIGHT };

    let mut capturer = ScreenCapturer::open(monitor).expect("the primary monitor can be captured");
    let started = Instant::now();
    let mut frames = Vec::new();
    let keep = |frame, _slot| frames.push(frame);
    record_screen(&mut capturer, region, (WIDTH, HEIGHT), FPS, FPS.per_second() * SECONDS, || true, keep)
        .expect("the top-left corner is on the monitor");
    let recorded = started.elapsed();

    let borrowed: Vec<&[Rgb]> = frames.iter().map(Vec::as_slice).collect();
    let encode = |tolerance| {
        let settings = ManualSettings { colors: ManualSettings::MAX_COLORS, dither: 1.0, tolerance, loss: 0 };
        encode_gif(WIDTH as u16, HEIGHT as u16, &borrowed, FPS, settings).expect("every frame is WIDTH x HEIGHT")
    };
    let bytes = encode(0);
    std::fs::write(&path, &bytes).expect("the output path is writable");
    println!("{} frames in {recorded:?}, saved to {path}: {} bytes", frames.len(), bytes.len());
    for tolerance in [4, 8, 16] {
        println!("with tolerance {tolerance}: {} bytes", encode(tolerance).len());
    }
}
