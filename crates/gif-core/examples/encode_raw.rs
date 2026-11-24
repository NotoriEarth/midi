use gif_core::GifFps;
use gif_core::fit::{EncodeSettings, Quality, encode};
use gif_core::writer::Rgb;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, input, width, height, fps, level, output, rest @ ..] = &args[..] else {
        panic!("usage: encode_raw RAW_RGB24 WIDTH HEIGHT FPS LEVEL OUT.gif [DITHER]");
    };
    let (width, height): (u16, u16) = (width.parse().unwrap(), height.parse().unwrap());
    let fps = GifFps::try_from(fps.parse::<u32>().unwrap()).unwrap();
    let level: usize = level.parse().unwrap();
    let dither: f32 = rest.first().map_or(1.0, |dither| dither.parse().unwrap());

    let bytes = std::fs::read(input).unwrap();
    let pixels: Vec<Rgb> = bytes.as_chunks::<3>().0.to_vec();
    let frames: Vec<&[Rgb]> = pixels.chunks_exact(usize::from(width) * usize::from(height)).collect();

    let started = Instant::now();
    let settings = EncodeSettings { quality: Quality::Level(level), dither };
    let encoded = encode(width, height, &mut &frames[..], fps, settings, &mut |_| {}).unwrap();
    let seconds = started.elapsed().as_secs_f64();
    std::fs::write(output, &encoded.gif).unwrap();
    println!("frames={} encode_secs={seconds:.3} bytes={}", frames.len(), encoded.gif.len());
}
