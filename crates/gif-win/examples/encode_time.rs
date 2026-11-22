use std::time::{Duration, Instant};

use gif_core::GifFps;
use gif_core::dither::ordered_dither;
use gif_core::fit::LEVELS;
use gif_core::palette::Palette;
use gif_core::session::{MonitorId, ScreenRect};
use gif_core::stabilize::stabilize;
use gif_core::writer::{Rgb, write_gif};
use gif_win::ScreenCapturer;

const SCROLL_ROWS_PER_FRAME: usize = 8;

fn timed<T>(work: impl FnOnce() -> T) -> (T, Duration) {
    let started = Instant::now();
    let output = work();
    (output, started.elapsed())
}

fn scrolled(first: &[Rgb], width: usize, frame: usize) -> Vec<Rgb> {
    let mut pixels = first.to_vec();
    pixels.rotate_left((frame * SCROLL_ROWS_PER_FRAME * width) % first.len());
    pixels
}

fn main() {
    let numbers: Vec<u32> = std::env::args().skip(1).take(4).map(|text| text.parse().expect("a number")).collect();
    let [width, height, per_second, frame_count] = numbers[..] else {
        panic!("usage: encode_time WIDTH HEIGHT FPS FRAMES [still|scroll]");
    };
    let motion = std::env::args().nth(5).unwrap_or_else(|| "scroll".to_string());
    let fps = GifFps::try_from(per_second).expect("10, 15 or 30");

    let monitor = MonitorId(0);
    let mut capturer = ScreenCapturer::open(monitor).expect("the primary monitor can be captured");
    let first = capturer.capture(ScreenRect { monitor, x: 0, y: 0, width, height }).expect("the region is on the monitor");
    let frames: Vec<Vec<Rgb>> = (0..frame_count as usize)
        .map(|frame| if motion == "still" { first.clone() } else { scrolled(&first, width as usize, frame) })
        .collect();
    let frames: Vec<&[Rgb]> = frames.iter().map(Vec::as_slice).collect();

    let megapixels = f64::from(width * height * frame_count) / 1e6;
    println!("{width}x{height}, {frame_count} frames at {per_second}fps, {motion}: {megapixels:.0} megapixels in all");

    for (number, level) in [(1, LEVELS[0]), (6, LEVELS[5])] {
        let (steady, stabilizing) = timed(|| stabilize(&frames, level.tolerance));
        let steady: Vec<&[Rgb]> = steady.iter().map(Vec::as_slice).collect();
        let (palette, choosing) = timed(|| Palette::from_frames(&steady, level.colors).expect("frames with pixels"));
        let (indexed, dithering) = timed(|| -> Vec<Vec<u8>> {
            steady.iter().map(|frame| ordered_dither(&palette, frame, width as usize, 1.0).expect("whole rows")).collect()
        });
        let indexed: Vec<&[u8]> = indexed.iter().map(Vec::as_slice).collect();
        let (gif, writing) =
            timed(|| write_gif(width as u16, height as u16, palette.colors(), &indexed, fps).expect("a GIF"));

        let whole = stabilizing + choosing + dithering + writing;
        println!(
            "  level {number}: {:.2}s  (stabilize {:.2}s, palette {:.2}s, dither {:.2}s, diff and compress {:.2}s)  {:.1} megapixels/s, {} bytes",
            whole.as_secs_f64(),
            stabilizing.as_secs_f64(),
            choosing.as_secs_f64(),
            dithering.as_secs_f64(),
            writing.as_secs_f64(),
            megapixels / whole.as_secs_f64(),
            gif.len(),
        );
    }
}
