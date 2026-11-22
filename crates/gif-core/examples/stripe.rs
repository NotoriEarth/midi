use gif_core::GifFps;
use gif_core::writer::{Rgb, write_gif};

const SIZE: usize = 120;
const STRIPE: usize = 20;
const PALETTE: [Rgb; 4] = [[24, 24, 32], [255, 96, 64], [255, 208, 64], [64, 200, 255]];

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "stripe.gif".to_string());
    let frames: Vec<Vec<u8>> = (0..30).map(frame).collect();
    let frames: Vec<&[u8]> = frames.iter().map(Vec::as_slice).collect();

    let bytes = write_gif(SIZE as u16, SIZE as u16, &PALETTE, &frames, GifFps::Fps30)
        .expect("frames match the palette and size");
    std::fs::write(&path, &bytes).expect("the output path is writable");
    println!("{path}: {} bytes, {} frames", bytes.len(), frames.len());
}

fn frame(number: usize) -> Vec<u8> {
    let offset = number * SIZE / 30;
    (0..SIZE * SIZE)
        .map(|index| {
            let x = index % SIZE;
            let band = ((x + SIZE - offset) % SIZE) / STRIPE;
            if band < 3 { band as u8 + 1 } else { 0 }
        })
        .collect()
}
