use gif_core::GifFps;
use gif_core::encode::{ManualSettings, encode_gif};
use gif_core::session::{MonitorId, ScreenRect};
use gif_win::ScreenCapturer;

const WIDTH: u32 = 480;
const HEIGHT: u32 = 270;

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "screenshot.gif".to_string());
    let monitor = MonitorId(0);

    let mut capturer = ScreenCapturer::open(monitor).expect("the primary monitor can be captured");
    let (monitor_width, monitor_height) = capturer.monitor_size();
    let region = ScreenRect { monitor, x: 0, y: 0, width: WIDTH, height: HEIGHT };
    let pixels = capturer.capture(region).expect("the top-left corner is on the monitor");

    let settings = ManualSettings { colors: ManualSettings::MAX_COLORS, dither: 1.0, tolerance: 0 };
    let bytes = encode_gif(WIDTH as u16, HEIGHT as u16, &[&pixels], GifFps::Fps10, settings)
        .expect("the capture is WIDTH x HEIGHT");
    std::fs::write(&path, &bytes).expect("the output path is writable");
    println!("monitor {monitor_width}x{monitor_height}, saved {WIDTH}x{HEIGHT} to {path}: {} bytes", bytes.len());
}
