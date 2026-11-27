// Encodes a raw rgb24 file with midi's gif-core the way the app does for a chosen quality level.
use gif_core::GifFps;
use gif_core::fit::{EncodeSettings, Quality, encode};
use gif_core::writer::Rgb;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, input, width, height, fps, level, output] = &args[..] else {
        panic!("usage: midi-bench RAW WIDTH HEIGHT FPS LEVEL OUT.gif");
    };
    let (width, height): (u16, u16) = (width.parse().unwrap(), height.parse().unwrap());
    let fps = GifFps::try_from(fps.parse::<u32>().unwrap()).unwrap();
    let level: usize = level.parse().unwrap();

    let bytes = std::fs::read(input).unwrap();
    let frame_len = usize::from(width) * usize::from(height);
    let pixels: Vec<Rgb> = bytes.chunks_exact(3).map(|p| [p[0], p[1], p[2]]).collect();
    let frames: Vec<&[Rgb]> = pixels.chunks_exact(frame_len).collect();

    let started = Instant::now();
    let settings = EncodeSettings { quality: Quality::Level(level), dither: 1.0 };
    let encoded = encode(width, height, &mut &frames[..], fps, settings, &mut |_| {}).unwrap();
    let encode_secs = started.elapsed().as_secs_f64();
    std::fs::write(output, &encoded.gif).unwrap();
    println!("frames={} encode_secs={encode_secs:.3} bytes={}", frames.len(), encoded.gif.len());
}
