use std::io::Write;
use std::time::Instant;

use gif_core::GifFps;
use gif_core::session::{MonitorId, ScreenRect};
use gif_win::{ScreenCapturer, record_screen};

fn main() {
    let numbers: Vec<u32> = std::env::args().skip(1).take(4).map(|text| text.parse().expect("a number")).collect();
    let [width, height, per_second, frame_count] = numbers[..] else {
        panic!("usage: spool_speed WIDTH HEIGHT FPS FRAMES [disk|nowhere]");
    };
    let to_disk = std::env::args().nth(5).is_none_or(|place| place == "disk");
    let fps = GifFps::try_from(per_second).expect("10, 15 or 30");
    let monitor = MonitorId(0);
    let region = ScreenRect { monitor, x: 0, y: 0, width, height };

    let path = std::env::temp_dir().join(format!("midi-spool-speed-{}.frames", std::process::id()));
    let mut file = std::fs::File::create(&path).expect("the temp folder is writable");
    let mut capturer = ScreenCapturer::open(monitor).expect("the primary monitor can be captured");

    let mut kept = 0u32;
    let mut slowest_write = 0.0f64;
    let keep = |frame: Vec<[u8; 3]>| {
        let started = Instant::now();
        if to_disk {
            file.write_all(frame.as_flattened()).expect("the disk has room");
        }
        slowest_write = slowest_write.max(started.elapsed().as_secs_f64());
        kept += 1;
    };
    let started = Instant::now();
    record_screen(&mut capturer, region, (width, height), fps, frame_count, || true, keep).expect("the region is on the monitor");
    let took = started.elapsed().as_secs_f64();
    drop(file);
    std::fs::remove_file(&path).expect("the file can be removed");

    let on_time = f64::from(frame_count - 1) / f64::from(per_second);
    let megabytes_per_second = f64::from(width * height * 3) * f64::from(kept) / 1e6 / took;
    println!(
        "{width}x{height} at {per_second}fps to {}: {kept} frames in {took:.2}s (on time would be {on_time:.2}s), {megabytes_per_second:.0} MB/s, slowest write {:.0}ms",
        if to_disk { "disk" } else { "nowhere" },
        slowest_write * 1000.0,
    );
}
