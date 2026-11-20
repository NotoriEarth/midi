use gif_core::GifFps;
use gif_core::dither::ordered_dither;
use gif_core::palette::Palette;
use gif_core::writer::{Rgb, write_gif};

const WIDTH: usize = 160;
const HEIGHT: usize = 120;
const FRAMES: usize = 15;

fn main() {
    let mut arguments = std::env::args().skip(1);
    let path = arguments.next().unwrap_or_else(|| "gradient.gif".to_string());
    let strength: f32 = arguments.next().map_or(1.0, |text| text.parse().expect("strength is a number"));
    let frames: Vec<Vec<Rgb>> = (0..FRAMES).map(frame).collect();
    let frames: Vec<&[Rgb]> = frames.iter().map(Vec::as_slice).collect();

    let palette = Palette::from_frames(&frames, Palette::MAX_COLORS).expect("frames have pixels");
    let indexed: Vec<Vec<u8>> = frames
        .iter()
        .map(|frame| ordered_dither(&palette, frame, WIDTH, strength).expect("frames are WIDTH wide"))
        .collect();
    let indexed: Vec<&[u8]> = indexed.iter().map(Vec::as_slice).collect();

    let bytes = write_gif(WIDTH as u16, HEIGHT as u16, palette.colors(), &indexed, GifFps::Fps15)
        .expect("indices come from the palette");
    std::fs::write(&path, &bytes).expect("the output path is writable");
    println!("{path}: {} bytes, {} colors, dither {strength}", bytes.len(), palette.colors().len());
}

fn frame(number: usize) -> Vec<Rgb> {
    let shift = number * 255 / FRAMES;
    (0..WIDTH * HEIGHT)
        .map(|index| {
            let (x, y) = (index % WIDTH, index / WIDTH);
            let red = x * 255 / (WIDTH - 1);
            let green = y * 255 / (HEIGHT - 1);
            let blue = (shift + x + y) % 256;
            [red as u8, green as u8, blue as u8]
        })
        .collect()
}
