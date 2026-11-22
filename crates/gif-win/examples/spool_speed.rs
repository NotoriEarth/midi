use std::time::Instant;

use gif_core::GifFps;
use gif_core::session::{MonitorId, ScreenRect};
use gif_win::{ScreenCapturer, Spool, record_screen};

fn main() {
    let numbers: Vec<u32> = std::env::args().skip(1).take(4).map(|text| text.parse().expect("a number")).collect();
    let [width, height, per_second, frame_count] = numbers[..] else {
        panic!("usage: spool_speed WIDTH HEIGHT FPS FRAMES [screen|busy]");
    };
    let every_row_changes = std::env::args().nth(5).is_some_and(|what| what == "busy");
    let fps = GifFps::try_from(per_second).expect("10, 15 or 30");
    let monitor = MonitorId(0);
    let region = ScreenRect { monitor, x: 0, y: 0, width, height };

    let mut spool = Spool::create_in(&std::env::temp_dir(), (width, height)).expect("the temp folder is writable");
    let mut capturer = ScreenCapturer::open(monitor).expect("the primary monitor can be captured");

    let mut kept = 0u32;
    let mut slowest_write = 0.0f64;
    let keep = |mut frame: Vec<[u8; 3]>| {
        if every_row_changes {
            for row in frame.chunks_mut(width as usize) {
                row[0] = [kept as u8; 3];
            }
        }
        let started = Instant::now();
        spool.push(&frame).expect("the disk has room");
        slowest_write = slowest_write.max(started.elapsed().as_secs_f64());
        kept += 1;
    };
    let started = Instant::now();
    record_screen(&mut capturer, region, (width, height), fps, frame_count, || true, keep).expect("the region is on the monitor");
    let took = started.elapsed().as_secs_f64();

    let on_time = f64::from(frame_count - 1) / f64::from(per_second);
    let written = spool.bytes_written() as f64 / 1e6;
    let uncompressed = Spool::frame_bytes((width, height)) as f64 * f64::from(kept) / 1e6;
    println!(
        "{width}x{height} at {per_second}fps, {}: {kept} frames in {took:.2}s (on time would be {on_time:.2}s), wrote {written:.0}MB of {uncompressed:.0}MB ({:.0}MB/s), slowest push {:.0}ms",
        if every_row_changes { "every row changing" } else { "the screen as it is" },
        written / took,
        slowest_write * 1000.0,
    );
}
