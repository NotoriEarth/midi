use std::sync::{Mutex, MutexGuard, PoisonError};

use gif_core::GifFps;
use gif_core::encode::{ManualSettings, encode_gif};
use gif_core::session::{MonitorId, ScreenRect};
use gif_core::writer::Rgb;
use gif_win::{CaptureError, ScreenCapturer, record_screen, select_region};

use crate::sample::{explain_encode, parse_fps};

const PRIMARY: MonitorId = MonitorId(0);
const MAX_SECONDS: u32 = 30;
const MAX_FRAME_BYTES: u64 = 1 << 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Phase {
    #[default]
    Idle,
    Selecting,
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
    pub fn record(
        &self,
        fps: u32,
        colors: usize,
        dither: f32,
        on_recording: impl FnOnce(ScreenRect),
    ) -> Result<Option<Vec<u8>>, String> {
        let fps = parse_fps(fps)?;
        let settings = ManualSettings { colors, dither };
        settings.check().map_err(explain_encode)?;

        let _back_to_idle = self.begin()?;
        match select_region(PRIMARY).map_err(explain_capture)? {
            Some(region) => self.record_region(region, fps, settings, on_recording),
            None => Ok(None),
        }
    }

    pub fn stop(&self) {
        self.ask(Phase::StopAsked);
    }

    pub fn cancel(&self) {
        self.ask(Phase::CancelAsked);
    }

    fn record_region(
        &self,
        region: ScreenRect,
        fps: GifFps,
        settings: ManualSettings,
        on_recording: impl FnOnce(ScreenRect),
    ) -> Result<Option<Vec<u8>>, String> {
        let mut capturer = ScreenCapturer::open(region.monitor).map_err(explain_capture)?;
        *self.phase() = Phase::Recording;
        on_recording(region);

        let keep_going = || *self.phase() == Phase::Recording;
        let frames = record_screen(&mut capturer, region, fps, max_frames(fps, region), keep_going)
            .map_err(explain_capture)?;

        if !self.encode_unless_cancelled() {
            return Ok(None);
        }
        let frames: Vec<&[Rgb]> = frames.iter().map(Vec::as_slice).collect();
        encode_gif(region.width as u16, region.height as u16, &frames, fps, settings)
            .map(Some)
            .map_err(explain_encode)
    }

    fn phase(&self) -> MutexGuard<'_, Phase> {
        self.phase.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn begin(&self) -> Result<BackToIdle<'_>, String> {
        let mut phase = self.phase();
        if *phase != Phase::Idle {
            return Err("이미 녹화하고 있습니다.".to_string());
        }
        *phase = Phase::Selecting;
        Ok(BackToIdle(self))
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

#[derive(Debug)]
struct BackToIdle<'a>(&'a Recorder);

impl Drop for BackToIdle<'_> {
    fn drop(&mut self) {
        *self.0.phase() = Phase::Idle;
    }
}

impl std::fmt::Debug for Recorder {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "Recorder({:?})", *self.phase())
    }
}

fn max_frames(fps: GifFps, region: ScreenRect) -> u32 {
    let bytes_per_frame = u64::from(region.width) * u64::from(region.height) * size_of::<Rgb>() as u64;
    let frames_in_memory = (MAX_FRAME_BYTES / bytes_per_frame.max(1)).max(1);
    (fps.per_second() * MAX_SECONDS).min(frames_in_memory as u32)
}

fn explain_capture(error: CaptureError) -> String {
    match error {
        CaptureError::NoSuchMonitor(_) => {
            "캡처할 모니터를 찾지 못했습니다. 화면이 잠겨 있지 않은지 확인하세요.".to_string()
        }
        CaptureError::AccessLost => "녹화 중에 화면 모드가 바뀌어 캡처가 끊겼습니다. 다시 시도하세요.".to_string(),
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

    const SMALL: ScreenRect = ScreenRect { monitor: PRIMARY, x: 0, y: 0, width: 160, height: 90 };
    const SETTINGS: ManualSettings = ManualSettings { colors: 64, dither: 0.5 };

    fn region(width: u32, height: u32) -> ScreenRect {
        ScreenRect { width, height, ..SMALL }
    }

    fn after(delay: Duration, action: impl FnOnce() + Send) -> impl FnOnce() + Send {
        move || {
            thread::sleep(delay);
            action();
        }
    }

    #[test]
    fn bad_settings_are_explained_before_anything_is_selected() {
        let recorder = Recorder::default();
        assert_eq!(
            recorder.record(60, 64, 0.5, |_| {}),
            Err("지원하지 않는 fps입니다: 60 (10, 15, 30 중에서 고르세요)".to_string())
        );
        assert_eq!(recorder.record(15, 1, 0.5, |_| {}), Err("색 수는 2~255 사이여야 합니다: 1".to_string()));
        assert_eq!(*recorder.phase(), Phase::Idle);
    }

    #[test]
    fn stop_and_cancel_do_nothing_unless_recording() {
        let recorder = Recorder::default();
        recorder.stop();
        recorder.cancel();
        assert_eq!(*recorder.phase(), Phase::Idle);

        let _session = recorder.begin().unwrap();
        recorder.stop();
        recorder.cancel();
        assert_eq!(*recorder.phase(), Phase::Selecting);
    }

    #[test]
    fn a_second_recording_is_refused_while_one_is_running() {
        let recorder = Recorder::default();
        let _session = recorder.begin().unwrap();
        assert_eq!(recorder.record(15, 64, 0.5, |_| {}), Err("이미 녹화하고 있습니다.".to_string()));
        assert_eq!(*recorder.phase(), Phase::Selecting);
    }

    #[test]
    fn the_recorder_is_idle_again_when_the_session_ends() {
        let recorder = Recorder::default();
        drop(recorder.begin().unwrap());
        assert_eq!(*recorder.phase(), Phase::Idle);
    }

    #[test]
    fn a_cancel_is_not_overwritten_by_a_later_stop() {
        let recorder = Recorder::default();
        *recorder.phase() = Phase::Recording;
        recorder.cancel();
        recorder.stop();
        assert!(!recorder.encode_unless_cancelled());
    }

    #[test]
    fn a_small_region_may_record_for_the_full_thirty_seconds() {
        assert_eq!(max_frames(GifFps::Fps30, region(480, 270)), 900);
        assert_eq!(max_frames(GifFps::Fps10, region(480, 270)), 300);
    }

    #[test]
    fn a_large_region_stops_earlier_so_its_frames_fit_in_a_gigabyte() {
        let frames = max_frames(GifFps::Fps30, region(2560, 1440));
        assert_eq!(frames, 97);
        assert!(u64::from(frames) * 2560 * 1440 * 3 <= MAX_FRAME_BYTES);
    }

    #[test]
    #[ignore = "needs an unlocked desktop; run with --ignored"]
    fn recording_runs_until_stop_and_becomes_a_gif_of_the_region() {
        let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner);
        let recorder = Recorder::default();
        let session = recorder.begin().unwrap();
        let mut told = None;
        let started = Instant::now();
        let gif = thread::scope(|scope| {
            scope.spawn(after(Duration::from_millis(600), || recorder.stop()));
            recorder.record_region(SMALL, GifFps::Fps10, SETTINGS, |region| told = Some(region))
        });
        let took = started.elapsed();
        drop(session);

        let gif = gif.unwrap().expect("a stopped recording gives a GIF");
        assert!(gif.starts_with(b"GIF89a"), "{} bytes", gif.len());
        assert_eq!(gif[6..10], [160, 0, 90, 0], "the GIF is as large as the region");
        assert_eq!(told, Some(SMALL));
        assert!(took >= Duration::from_millis(600), "{took:?}");
        assert_eq!(*recorder.phase(), Phase::Idle);
    }

    #[test]
    #[ignore = "needs an unlocked desktop; run with --ignored"]
    fn a_cancelled_recording_gives_nothing() {
        let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner);
        let recorder = Recorder::default();
        let _session = recorder.begin().unwrap();
        let gif = thread::scope(|scope| {
            scope.spawn(after(Duration::from_millis(300), || recorder.cancel()));
            recorder.record_region(SMALL, GifFps::Fps10, SETTINGS, |_| {})
        });
        assert_eq!(gif, Ok(None));
    }
}
