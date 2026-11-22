use std::sync::{Mutex, MutexGuard, PoisonError};

use gif_core::encode::{ManualSettings, encode_gif};
use gif_core::session::{MonitorId, ScreenRect};
use gif_core::writer::Rgb;
use gif_win::{CaptureError, ScreenCapturer, record_screen};

use crate::sample::{explain_encode, parse_fps};

const PRIMARY: MonitorId = MonitorId(0);
const WIDTH: u32 = 480;
const HEIGHT: u32 = 270;
const MAX_SECONDS: u32 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Phase {
    #[default]
    Idle,
    Recording,
    StopAsked,
    CancelAsked,
    Encoding,
}

#[derive(Default)]
pub struct Recorder {
    phase: Mutex<Phase>,
}

impl Recorder {
    pub fn record(&self, fps: u32, colors: usize, dither: f32) -> Result<Option<Vec<u8>>, String> {
        let fps = parse_fps(fps)?;
        let settings = ManualSettings { colors, dither };
        settings.check().map_err(explain_encode)?;

        self.begin()?;
        let _back_to_idle = BackToIdle(self);

        let region = ScreenRect {
            monitor: PRIMARY,
            x: 0,
            y: 0,
            width: WIDTH,
            height: HEIGHT,
        };
        let mut capturer = ScreenCapturer::open(PRIMARY).map_err(explain_capture)?;
        let max_frames = fps.per_second() * MAX_SECONDS;
        let keep_going = || *self.phase() == Phase::Recording;
        let frames = record_screen(&mut capturer, region, fps, max_frames, keep_going)
            .map_err(explain_capture)?;

        if !self.encode_unless_cancelled() {
            return Ok(None);
        }
        let frames: Vec<&[Rgb]> = frames.iter().map(Vec::as_slice).collect();
        encode_gif(WIDTH as u16, HEIGHT as u16, &frames, fps, settings)
            .map(Some)
            .map_err(explain_encode)
    }

    pub fn stop(&self) {
        self.ask(Phase::StopAsked);
    }

    pub fn cancel(&self) {
        self.ask(Phase::CancelAsked);
    }

    fn phase(&self) -> MutexGuard<'_, Phase> {
        self.phase.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn begin(&self) -> Result<(), String> {
        let mut phase = self.phase();
        if *phase != Phase::Idle {
            return Err("이미 녹화하고 있습니다.".to_string());
        }
        *phase = Phase::Recording;
        Ok(())
    }

    fn ask(&self, asked: Phase) {
        let mut phase = self.phase();
        if *phase == Phase::Recording {
            *phase = asked;
        }
    }

    fn encode_unless_cancelled(&self) -> bool {
        let mut phase = self.phase();
        let cancelled = *phase == Phase::CancelAsked;
        if !cancelled {
            *phase = Phase::Encoding;
        }
        !cancelled
    }
}

struct BackToIdle<'a>(&'a Recorder);

impl Drop for BackToIdle<'_> {
    fn drop(&mut self) {
        *self.0.phase() = Phase::Idle;
    }
}

fn explain_capture(error: CaptureError) -> String {
    match error {
        CaptureError::NoSuchMonitor(_) => {
            "캡처할 모니터를 찾지 못했습니다. 화면이 잠겨 있지 않은지 확인하세요.".to_string()
        }
        CaptureError::RegionOutsideMonitor {
            monitor_width,
            monitor_height,
        } => {
            format!(
                "모니터({monitor_width}×{monitor_height})가 녹화 영역 {WIDTH}×{HEIGHT}보다 작습니다."
            )
        }
        CaptureError::AccessLost => {
            "녹화 중에 화면 모드가 바뀌어 캡처가 끊겼습니다. 다시 시도하세요.".to_string()
        }
        CaptureError::NoFrameYet => "화면 그림을 받지 못했습니다. 다시 시도하세요.".to_string(),
        other => format!("화면을 캡처하지 못했습니다: {other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::{Duration, Instant};

    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

    fn after(delay: Duration, action: impl FnOnce() + Send) -> impl FnOnce() + Send {
        move || {
            thread::sleep(delay);
            action();
        }
    }

    #[test]
    fn bad_settings_are_explained_before_anything_is_recorded() {
        let recorder = Recorder::default();
        assert_eq!(
            recorder.record(60, 64, 0.5),
            Err("지원하지 않는 fps입니다: 60 (10, 15, 30 중에서 고르세요)".to_string())
        );
        assert_eq!(
            recorder.record(15, 1, 0.5),
            Err("색 수는 2~255 사이여야 합니다: 1".to_string())
        );
        assert_eq!(*recorder.phase(), Phase::Idle);
    }

    #[test]
    fn stop_and_cancel_do_nothing_when_nothing_is_being_recorded() {
        let recorder = Recorder::default();
        recorder.stop();
        recorder.cancel();
        assert_eq!(*recorder.phase(), Phase::Idle);
    }

    #[test]
    fn a_second_recording_is_refused_while_one_is_running() {
        let recorder = Recorder::default();
        recorder.begin().unwrap();
        assert_eq!(
            recorder.record(15, 64, 0.5),
            Err("이미 녹화하고 있습니다.".to_string())
        );
        assert_eq!(*recorder.phase(), Phase::Recording);
    }

    #[test]
    fn a_cancel_is_not_overwritten_by_a_later_stop() {
        let recorder = Recorder::default();
        recorder.begin().unwrap();
        recorder.cancel();
        recorder.stop();
        assert!(!recorder.encode_unless_cancelled());
    }

    #[test]
    #[ignore = "needs an unlocked desktop; run with --ignored"]
    fn recording_runs_until_stop_and_becomes_a_gif() {
        let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner);
        let recorder = Recorder::default();
        let started = Instant::now();
        let gif = thread::scope(|scope| {
            scope.spawn(after(Duration::from_millis(600), || recorder.stop()));
            recorder.record(10, 64, 0.5)
        });
        let took = started.elapsed();

        let gif = gif.unwrap().expect("a stopped recording gives a GIF");
        assert!(gif.starts_with(b"GIF89a"), "{} bytes", gif.len());
        assert!(took >= Duration::from_millis(600), "{took:?}");
        assert_eq!(*recorder.phase(), Phase::Idle);
    }

    #[test]
    #[ignore = "needs an unlocked desktop; run with --ignored"]
    fn a_cancelled_recording_gives_nothing_and_another_can_start() {
        let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner);
        let recorder = Recorder::default();
        let gif = thread::scope(|scope| {
            scope.spawn(after(Duration::from_millis(300), || recorder.cancel()));
            recorder.record(10, 64, 0.5)
        });
        assert_eq!(gif, Ok(None));
        assert_eq!(*recorder.phase(), Phase::Idle);
    }
}
