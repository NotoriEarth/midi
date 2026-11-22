use gif_core::GifFps;
use gif_core::encode::EncodeError;
use gif_core::fit::{self, EncodeSettings, LEVELS};
use gif_core::writer::Rgb;

const WIDTH: usize = 240;
const HEIGHT: usize = 135;
const SECONDS: usize = 2;
const RADIUS: i64 = 12;
const BALL: Rgb = [255, 240, 80];

pub fn encode(fps: u32, settings: EncodeSettings) -> Result<Vec<u8>, String> {
    let fps = parse_fps(fps)?;
    settings.check().map_err(explain_encode)?;

    let frame_count = fps.per_second() as usize * SECONDS;
    let frames: Vec<Vec<Rgb>> = (0..frame_count).map(|number| frame(number, frame_count)).collect();
    let frames: Vec<&[Rgb]> = frames.iter().map(Vec::as_slice).collect();

    fit::encode(WIDTH as u16, HEIGHT as u16, &frames, fps, settings).map(|encoded| encoded.gif).map_err(explain_encode)
}

pub fn parse_fps(fps: u32) -> Result<GifFps, String> {
    GifFps::try_from(fps)
        .map_err(|unsupported| format!("지원하지 않는 fps입니다: {} (10, 15, 30 중에서 고르세요)", unsupported.0))
}

pub fn explain_encode(error: EncodeError) -> String {
    match error {
        EncodeError::Level(level) => format!("화질 단계는 1~{} 사이여야 합니다: {level}", LEVELS.len()),
        EncodeError::Dither(dither) => format!("디더링 강도는 0~1 사이여야 합니다: {dither}"),
        EncodeError::SizeLimit(bytes) => format!("용량 목표는 1바이트 이상이어야 합니다: {bytes}"),
        EncodeError::NoFrames => "녹화된 프레임이 없습니다. 조금 더 길게 녹화하세요.".to_string(),
        other => format!("GIF를 만들지 못했습니다: {other:?}"),
    }
}

fn frame(number: usize, frame_count: usize) -> Vec<Rgb> {
    let (center_x, center_y) = ball_center(number, frame_count);
    (0..WIDTH * HEIGHT)
        .map(|index| {
            let (x, y) = ((index % WIDTH) as i64, (index / WIDTH) as i64);
            let inside_ball = (x - center_x).pow(2) + (y - center_y).pow(2) <= RADIUS * RADIUS;
            if inside_ball { BALL } else { background(x, y) }
        })
        .collect()
}

fn ball_center(number: usize, frame_count: usize) -> (i64, i64) {
    let progress = number as f64 / frame_count as f64;
    let bounces = 3.0;
    let height = (progress * bounces * std::f64::consts::PI).sin().abs();
    (20 + (progress * 200.0) as i64, 110 - (height * 70.0) as i64)
}

fn background(x: i64, y: i64) -> Rgb {
    let red = 30 + x * 60 / WIDTH as i64;
    let green = 60 + y * 120 / HEIGHT as i64;
    let blue = 200 - y * 100 / HEIGHT as i64;
    [red as u8, green as u8, blue as u8]
}

#[cfg(test)]
mod tests {
    use super::*;
    use gif_core::fit::Quality;

    fn level(level: usize, dither: f32) -> EncodeSettings {
        EncodeSettings { quality: Quality::Level(level), dither }
    }

    #[test]
    fn valid_settings_give_a_gif_file() {
        let bytes = encode(15, level(3, 0.5)).unwrap();
        assert!(bytes.starts_with(b"GIF89a"), "{} bytes", bytes.len());
    }

    #[test]
    fn a_higher_frame_rate_gives_more_frames_and_a_bigger_file() {
        let slow = encode(10, level(1, 1.0)).unwrap();
        let fast = encode(30, level(1, 1.0)).unwrap();
        assert!(fast.len() > slow.len(), "10 fps {} bytes, 30 fps {} bytes", slow.len(), fast.len());
    }

    #[test]
    fn an_unsupported_frame_rate_is_explained() {
        assert_eq!(encode(60, level(3, 0.5)), Err("지원하지 않는 fps입니다: 60 (10, 15, 30 중에서 고르세요)".to_string()));
    }

    #[test]
    fn settings_out_of_range_are_explained() {
        assert_eq!(encode(15, level(7, 0.5)), Err("화질 단계는 1~6 사이여야 합니다: 7".to_string()));
        assert_eq!(encode(15, level(3, 2.0)), Err("디더링 강도는 0~1 사이여야 합니다: 2".to_string()));
    }
}
