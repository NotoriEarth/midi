use gif_core::GifFps;
use gif_core::encode::{ManualSettings, encode_gif};
use gif_core::writer::Rgb;

const WIDTH: usize = 160;
const HEIGHT: usize = 120;
const FRAMES: usize = 15;

fn main() {
    let mut arguments = std::env::args().skip(1);
    let path = arguments.next().unwrap_or_else(|| "gradient.gif".to_string());
    let strength: f32 = arguments.next().map_or(1.0, |text| text.parse().expect("strength is a number"));
    let frames: Vec<Vec<Rgb>> = (0..FRAMES).map(frame).collect();
    let frames: Vec<&[Rgb]> = frames.iter().map(Vec::as_slice).collect();

    let settings = ManualSettings { colors: ManualSettings::MAX_COLORS, dither: strength, tolerance: 0, loss: 0 };
    let bytes = encode_gif(WIDTH as u16, HEIGHT as u16, &frames, GifFps::Fps15, settings)
        .expect("frames are WIDTH x HEIGHT and the settings are in range");
    std::fs::write(&path, &bytes).expect("the output path is writable");
    println!("{path}: {} bytes, dither {strength}", bytes.len());
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
