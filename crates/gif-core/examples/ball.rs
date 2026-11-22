use gif_core::GifFps;
use gif_core::encode::{ManualSettings, encode_gif};
use gif_core::writer::Rgb;

const WIDTH: usize = 240;
const HEIGHT: usize = 135;
const FRAMES: usize = 30;
const RADIUS: i64 = 12;
const BALL: Rgb = [255, 240, 80];

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "ball.gif".to_string());
    let frames: Vec<Vec<Rgb>> = (0..FRAMES).map(frame).collect();
    let frames: Vec<&[Rgb]> = frames.iter().map(Vec::as_slice).collect();

    let settings = ManualSettings { colors: ManualSettings::MAX_COLORS, dither: 1.0 };
    let bytes = encode_gif(WIDTH as u16, HEIGHT as u16, &frames, GifFps::Fps30, settings)
        .expect("frames are WIDTH x HEIGHT and the settings are in range");
    std::fs::write(&path, &bytes).expect("the output path is writable");
    println!("{path}: {} bytes, {} frames", bytes.len(), frames.len());
}

fn ball_center(number: usize) -> (i64, i64) {
    let x = 20 + number * 200 / (FRAMES - 1);
    let bounce = (number % 10) as i64 - 5;
    (x as i64, 100 - (25 - bounce * bounce) * 2)
}

fn frame(number: usize) -> Vec<Rgb> {
    let (center_x, center_y) = ball_center(number);
    (0..WIDTH * HEIGHT)
        .map(|index| {
            let (x, y) = ((index % WIDTH) as i64, (index / WIDTH) as i64);
            let inside_ball = (x - center_x).pow(2) + (y - center_y).pow(2) <= RADIUS * RADIUS;
            if inside_ball { BALL } else { background(x, y) }
        })
        .collect()
}

fn background(x: i64, y: i64) -> Rgb {
    let red = 30 + x * 60 / WIDTH as i64;
    let green = 60 + y * 120 / HEIGHT as i64;
    let blue = 200 - y * 100 / HEIGHT as i64;
    [red as u8, green as u8, blue as u8]
}
