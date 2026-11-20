use gif_core::GifFps;
use gif_core::encode::{ManualSettings, encode_gif};
use gif_core::session::{MonitorId, ScreenRect};
use gif_core::writer::Rgb;
use gif_win::{CaptureError, ScreenCapturer, record_screen};

use crate::sample::{explain_encode, parse_fps};

const PRIMARY: MonitorId = MonitorId(0);
const WIDTH: u32 = 480;
const HEIGHT: u32 = 270;
const MAX_SECONDS: u32 = 10;

pub fn record(fps: u32, colors: usize, dither: f32, seconds: u32) -> Result<Vec<u8>, String> {
    let fps = parse_fps(fps)?;
    let frame_count = frame_count(fps, seconds)?;

    let region = ScreenRect { monitor: PRIMARY, x: 0, y: 0, width: WIDTH, height: HEIGHT };
    let mut capturer = ScreenCapturer::open(PRIMARY).map_err(explain_capture)?;
    let frames = record_screen(&mut capturer, region, fps, frame_count).map_err(explain_capture)?;
    let frames: Vec<&[Rgb]> = frames.iter().map(Vec::as_slice).collect();

    encode_gif(WIDTH as u16, HEIGHT as u16, &frames, fps, ManualSettings { colors, dither }).map_err(explain_encode)
}

fn frame_count(fps: GifFps, seconds: u32) -> Result<u32, String> {
    if (1..=MAX_SECONDS).contains(&seconds) {
        Ok(fps.per_second() * seconds)
    } else {
        Err(format!("녹화 길이는 1~{MAX_SECONDS}초 사이여야 합니다: {seconds}"))
    }
}

fn explain_capture(error: CaptureError) -> String {
    match error {
        CaptureError::NoSuchMonitor(_) => "캡처할 모니터를 찾지 못했습니다. 화면이 잠겨 있지 않은지 확인하세요.".to_string(),
        CaptureError::RegionOutsideMonitor { monitor_width, monitor_height } => {
            format!("모니터({monitor_width}×{monitor_height})가 녹화 영역 {WIDTH}×{HEIGHT}보다 작습니다.")
        }
        CaptureError::AccessLost => "녹화 중에 화면 모드가 바뀌어 캡처가 끊겼습니다. 다시 시도하세요.".to_string(),
        CaptureError::NoFrameYet => "화면 그림을 받지 못했습니다. 다시 시도하세요.".to_string(),
        other => format!("화면을 캡처하지 못했습니다: {other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_frame_count_is_the_frame_rate_times_the_seconds() {
        assert_eq!(frame_count(GifFps::Fps10, 3), Ok(30));
        assert_eq!(frame_count(GifFps::Fps30, 10), Ok(300));
    }

    #[test]
    fn a_length_out_of_range_is_explained() {
        assert_eq!(frame_count(GifFps::Fps10, 0), Err("녹화 길이는 1~10초 사이여야 합니다: 0".to_string()));
        assert_eq!(record(10, 64, 0.5, 11), Err("녹화 길이는 1~10초 사이여야 합니다: 11".to_string()));
    }

    #[test]
    fn an_unsupported_frame_rate_is_explained_before_anything_is_captured() {
        assert_eq!(record(60, 64, 0.5, 1), Err("지원하지 않는 fps입니다: 60 (10, 15, 30 중에서 고르세요)".to_string()));
    }

    #[test]
    #[ignore = "needs an unlocked desktop; run with --ignored"]
    fn one_second_of_screen_becomes_a_gif() {
        let bytes = record(10, 64, 0.5, 1).unwrap();
        assert!(bytes.starts_with(b"GIF89a"), "{} bytes", bytes.len());
    }
}
