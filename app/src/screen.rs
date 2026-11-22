use std::cell::RefCell;
use std::sync::{Mutex, MutexGuard, PoisonError};

use gif_core::GifFps;
use gif_core::fit::{EncodeSettings, Encoded, Step, encode};
use gif_core::resize::{OutputSize, ResizeError};
use gif_core::session::{MonitorId, ScreenRect};
use gif_core::writer::Rgb;
use gif_win::{CaptureError, RecordingBorder, ScreenCapturer, Spool, free_disk_bytes, record_screen, select_region};

use crate::sample::{explain_encode, parse_fps};

const PRIMARY: MonitorId = MonitorId(0);
pub const ALREADY_RECORDING: &str = "이미 녹화하고 있습니다.";
const MAX_SECONDS: u32 = 30;
const DISK_SPACE_LEFT_FREE: u64 = 1 << 30;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LimitedBy {
    Time,
    Disk,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordingLimit {
    pub frames: u32,
    pub by: LimitedBy,
}

impl RecordingLimit {
    fn milliseconds(self, fps: GifFps) -> u32 {
        self.frames * 1000 / fps.per_second()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Started {
    pub region: ScreenRect,
    pub output_size: (u32, u32),
    pub limit_ms: u32,
    pub limited_by: LimitedBy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordingEvent {
    Started(Started),
    Encoding(Step),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recorded {
    pub encoded: Encoded,
    pub ended_at_limit: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ending {
    Asked,
    AtLimit,
}

#[derive(Default)]
pub struct Recorder {
    phase: Mutex<Phase>,
}

impl Recorder {
    pub fn record(
        &self,
        fps: u32,
        settings: EncodeSettings,
        size: OutputSize,
        mut on_event: impl FnMut(RecordingEvent),
    ) -> Result<Option<Recorded>, String> {
        let fps = parse_fps(fps)?;
        settings.check().map_err(explain_encode)?;
        size.check().map_err(explain_resize)?;

        let _back_to_idle = self.begin()?;
        match select_region(PRIMARY).map_err(explain_capture)? {
            Some(region) => {
                let output_size = size.for_source(region.width, region.height).map_err(explain_resize)?;
                let limit = recording_limit(fps, output_size, free_disk_bytes(&std::env::temp_dir()))?;
                self.record_region(region, output_size, fps, limit, settings, &mut on_event)
            }
            None => Ok(None),
        }
    }

    pub fn stop(&self) -> bool {
        self.ask(Phase::StopAsked)
    }

    pub fn cancel(&self) {
        let _ = self.ask(Phase::CancelAsked);
    }

    fn record_region(
        &self,
        region: ScreenRect,
        output_size: (u32, u32),
        fps: GifFps,
        limit: RecordingLimit,
        settings: EncodeSettings,
        on_event: &mut dyn FnMut(RecordingEvent),
    ) -> Result<Option<Recorded>, String> {
        let mut capturer = ScreenCapturer::open(region.monitor).map_err(explain_capture)?;
        let mut spool = Spool::create_in(&std::env::temp_dir(), output_size).map_err(explain_spool)?;
        if !self.start_recording() {
            return Ok(None);
        }
        let started = Started { region, output_size, limit_ms: limit.milliseconds(fps), limited_by: limit.by };
        on_event(RecordingEvent::Started(started));

        let not_written = RefCell::new(None);
        let keep_going = || not_written.borrow().is_none() && *self.phase() == Phase::Recording;
        let keep = |frame: Vec<Rgb>, slot: u32| {
            if let Err(problem) = spool.push(&frame, slot as usize) {
                not_written.replace(Some(problem));
            }
        };
        let border = RecordingBorder::show(region).ok();
        let recorded = record_screen(&mut capturer, region, output_size, fps, limit.frames, keep_going, keep);
        drop(border);
        recorded.map_err(explain_capture)?;
        if let Some(problem) = not_written.into_inner() {
            return Err(explain_spool(problem));
        }

        let Some(ending) = self.encode_unless_cancelled() else {
            return Ok(None);
        };
        let mut on_step = |step| on_event(RecordingEvent::Encoding(step));
        let (width, height) = (output_size.0 as u16, output_size.1 as u16);
        let encoded = encode(width, height, &mut spool, fps, settings, &mut on_step).map_err(explain_encode)?;
        Ok(Some(Recorded { encoded, ended_at_limit: ending == Ending::AtLimit }))
    }

    fn phase(&self) -> MutexGuard<'_, Phase> {
        self.phase.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn begin(&self) -> Result<BackToIdle<'_>, String> {
        let mut phase = self.phase();
        if *phase != Phase::Idle {
            return Err(ALREADY_RECORDING.to_string());
        }
        *phase = Phase::Selecting;
        Ok(BackToIdle(self))
    }

    fn start_recording(&self) -> bool {
        let mut phase = self.phase();
        let nobody_cancelled = *phase == Phase::Selecting;
        if nobody_cancelled {
            *phase = Phase::Recording;
        }
        nobody_cancelled
    }

    fn ask(&self, asked: Phase) -> bool {
        let mut phase = self.phase();
        let cancel_before_recording = asked == Phase::CancelAsked && *phase == Phase::Selecting;
        let heard = *phase == Phase::Recording || cancel_before_recording;
        if heard {
            *phase = asked;
        }
        heard
    }

    fn encode_unless_cancelled(&self) -> Option<Ending> {
        let mut phase = self.phase();
        if *phase == Phase::CancelAsked {
            return None;
        }
        let ending = if *phase == Phase::Recording { Ending::AtLimit } else { Ending::Asked };
        *phase = Phase::Encoding;
        Some(ending)
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

const NO_ROOM_ON_DISK: &str =
    "임시 폴더가 있는 디스크에 여유 공간이 부족해서 녹화할 수 없습니다. 공간을 비우거나 크기를 줄이세요.";

fn recording_limit(
    fps: GifFps,
    (width, height): (u32, u32),
    free_disk_bytes: Option<u64>,
) -> Result<RecordingLimit, String> {
    let frames_in_time = u64::from(fps.per_second() * MAX_SECONDS);
    let by_time = RecordingLimit { frames: frames_in_time as u32, by: LimitedBy::Time };
    let Some(free) = free_disk_bytes else {
        return Ok(by_time);
    };

    let bytes_per_frame = Spool::frame_bytes((width, height)).max(1);
    let frames_on_disk = free.saturating_sub(DISK_SPACE_LEFT_FREE) / bytes_per_frame;
    match frames_on_disk {
        0 => Err(NO_ROOM_ON_DISK.to_string()),
        fewer if fewer < frames_in_time => Ok(RecordingLimit { frames: fewer as u32, by: LimitedBy::Disk }),
        _ => Ok(by_time),
    }
}

fn explain_spool(problem: std::io::Error) -> String {
    format!("녹화한 화면을 임시 파일에 쓰지 못했습니다: {problem}")
}

fn explain_resize(error: ResizeError) -> String {
    match error {
        ResizeError::EmptyTarget => "크기는 가로와 세로 모두 1 이상이어야 합니다.".to_string(),
        other => format!("크기를 줄이지 못했습니다: {other:?}"),
    }
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
    use gif_core::fit::{Adjust, Quality};
    use std::thread;
    use std::time::{Duration, Instant};

    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

    const SMALL: ScreenRect = ScreenRect { monitor: PRIMARY, x: 0, y: 0, width: 160, height: 90 };
    const SETTINGS: EncodeSettings = EncodeSettings { quality: Quality::Level(3), dither: 0.5 };
    const NO_SUCH_LEVEL: EncodeSettings = EncodeSettings { quality: Quality::Level(9), ..SETTINGS };
    const NO_ROOM: EncodeSettings =
        EncodeSettings { quality: Quality::WithinBytes { max_bytes: 0, adjust: Adjust::Level }, ..SETTINGS };
    const GIGABYTE: u64 = 1 << 30;
    const PLENTY: Option<u64> = Some(1 << 40);
    const LONG: RecordingLimit = RecordingLimit { frames: 300, by: LimitedBy::Time };
    const ORIGINAL: OutputSize = OutputSize::Original;
    const NO_WIDTH: OutputSize = OutputSize::FitWithin { width: 0, height: 270 };

    fn once_recording<'a>(
        recorder: &'a Recorder,
        delay: Duration,
        action: impl FnOnce() + Send + 'a,
    ) -> impl FnOnce() + Send + 'a {
        move || {
            while *recorder.phase() != Phase::Recording {
                thread::sleep(Duration::from_millis(5));
            }
            thread::sleep(delay);
            action();
        }
    }

    #[test]
    fn bad_settings_are_explained_before_anything_is_selected() {
        let recorder = Recorder::default();
        assert_eq!(
            recorder.record(60, SETTINGS, ORIGINAL, |_| {}),
            Err("지원하지 않는 fps입니다: 60 (10, 15, 30 중에서 고르세요)".to_string())
        );
        assert_eq!(
            recorder.record(15, NO_SUCH_LEVEL, ORIGINAL, |_| {}),
            Err("화질 단계는 1~6 사이여야 합니다: 9".to_string())
        );
        assert_eq!(
            recorder.record(15, SETTINGS, NO_WIDTH, |_| {}),
            Err("크기는 가로와 세로 모두 1 이상이어야 합니다.".to_string())
        );
        assert_eq!(
            recorder.record(15, NO_ROOM, ORIGINAL, |_| {}),
            Err("용량 목표는 1바이트 이상이어야 합니다: 0".to_string())
        );
        assert_eq!(*recorder.phase(), Phase::Idle);
    }

    #[test]
    fn stop_and_cancel_do_nothing_when_no_session_is_running() {
        let recorder = Recorder::default();
        recorder.stop();
        recorder.cancel();
        assert_eq!(*recorder.phase(), Phase::Idle);
    }

    #[test]
    fn a_stop_says_whether_a_recording_heard_it() {
        let recorder = Recorder::default();
        assert!(!recorder.stop(), "nothing to stop");
        *recorder.phase() = Phase::Recording;
        assert!(recorder.stop());
        assert!(!recorder.stop(), "already asked");
    }

    #[test]
    fn a_stop_before_recording_begins_is_ignored() {
        let recorder = Recorder::default();
        let _session = recorder.begin().unwrap();

        assert!(!recorder.stop());

        assert_eq!(*recorder.phase(), Phase::Selecting);
        assert!(recorder.start_recording());
        assert_eq!(*recorder.phase(), Phase::Recording);
    }

    #[test]
    fn a_cancel_before_recording_begins_keeps_it_from_beginning() {
        let recorder = Recorder::default();
        let _session = recorder.begin().unwrap();

        recorder.cancel();

        assert!(!recorder.start_recording());
        assert_ne!(*recorder.phase(), Phase::Recording);
    }

    #[test]
    fn the_recorder_is_idle_again_after_a_session_cancelled_before_it_began() {
        let recorder = Recorder::default();
        let session = recorder.begin().unwrap();
        recorder.cancel();
        drop(session);

        assert_eq!(*recorder.phase(), Phase::Idle);
        assert!(recorder.begin().is_ok());
    }

    #[test]
    fn a_second_recording_is_refused_while_one_is_running() {
        let recorder = Recorder::default();
        let _session = recorder.begin().unwrap();
        assert_eq!(recorder.record(15, SETTINGS, ORIGINAL, |_| {}), Err(ALREADY_RECORDING.to_string()));
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
        assert_eq!(recorder.encode_unless_cancelled(), None);
    }

    #[test]
    fn a_recording_that_was_stopped_ended_because_it_was_asked_to() {
        let recorder = Recorder::default();
        *recorder.phase() = Phase::Recording;
        recorder.stop();
        assert_eq!(recorder.encode_unless_cancelled(), Some(Ending::Asked));
        assert_eq!(*recorder.phase(), Phase::Encoding);
    }

    #[test]
    fn a_recording_nobody_stopped_ended_at_its_limit() {
        let recorder = Recorder::default();
        *recorder.phase() = Phase::Recording;
        assert_eq!(recorder.encode_unless_cancelled(), Some(Ending::AtLimit));
        assert_eq!(*recorder.phase(), Phase::Encoding);
    }

    #[test]
    fn a_small_region_may_record_for_the_full_thirty_seconds() {
        let by = LimitedBy::Time;
        assert_eq!(recording_limit(GifFps::Fps30, (480, 270), PLENTY), Ok(RecordingLimit { frames: 900, by }));
        assert_eq!(recording_limit(GifFps::Fps10, (480, 270), PLENTY), Ok(RecordingLimit { frames: 300, by }));
        assert_eq!(RecordingLimit { frames: 900, by }.milliseconds(GifFps::Fps30), 30_000);
    }

    #[test]
    fn a_whole_screen_may_record_for_thirty_seconds_too_when_the_disk_has_room() {
        let limit = recording_limit(GifFps::Fps30, (2560, 1440), PLENTY);
        assert_eq!(limit, Ok(RecordingLimit { frames: 900, by: LimitedBy::Time }));
    }

    #[test]
    fn a_disk_whose_free_space_is_unknown_does_not_shorten_the_recording() {
        let limit = recording_limit(GifFps::Fps30, (2560, 1440), None);
        assert_eq!(limit, Ok(RecordingLimit { frames: 900, by: LimitedBy::Time }));
    }

    #[test]
    fn a_nearly_full_disk_stops_the_recording_earlier_and_leaves_a_gigabyte_free() {
        let frame_bytes = 2560 * 1440 * 3;
        let room_for_97_frames = Some(GIGABYTE + 97 * frame_bytes + frame_bytes / 2);

        let limit = recording_limit(GifFps::Fps30, (2560, 1440), room_for_97_frames).unwrap();

        assert_eq!(limit, RecordingLimit { frames: 97, by: LimitedBy::Disk });
        assert_eq!(limit.milliseconds(GifFps::Fps30), 3233);
    }

    #[test]
    fn room_for_exactly_thirty_seconds_is_limited_by_time() {
        let frame_bytes = 480 * 270 * 3;
        let limit = recording_limit(GifFps::Fps10, (480, 270), Some(GIGABYTE + 300 * frame_bytes));
        assert_eq!(limit, Ok(RecordingLimit { frames: 300, by: LimitedBy::Time }));
    }

    #[test]
    fn a_disk_without_room_for_one_frame_refuses_to_record_and_says_why() {
        let frame_bytes = 480 * 270 * 3;
        for free in [0, GIGABYTE, GIGABYTE + frame_bytes - 1] {
            assert_eq!(recording_limit(GifFps::Fps10, (480, 270), Some(free)), Err(NO_ROOM_ON_DISK.to_string()));
        }
    }

    #[test]
    #[ignore = "needs an unlocked desktop; run with --ignored"]
    fn recording_runs_until_stop_and_becomes_a_gif_of_the_region() {
        let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner);
        let recorder = Recorder::default();
        let session = recorder.begin().unwrap();
        let mut told = Vec::new();
        let started = Instant::now();
        let gif = thread::scope(|scope| {
            scope.spawn(once_recording(&recorder, Duration::from_millis(600), || assert!(recorder.stop())));
            recorder.record_region(SMALL, (80, 45), GifFps::Fps10, LONG, SETTINGS, &mut |event| told.push(event))
        });
        let took = started.elapsed();
        drop(session);

        let recorded = gif.unwrap().expect("a stopped recording gives a GIF");
        assert!(!recorded.ended_at_limit);
        let gif = recorded.encoded.gif;
        assert!(gif.starts_with(b"GIF89a"), "{} bytes", gif.len());
        assert_eq!(gif[6..10], [80, 0, 45, 0], "the GIF is as large as the output size");
        let started = Started { region: SMALL, output_size: (80, 45), limit_ms: 30_000, limited_by: LimitedBy::Time };
        assert_eq!(told[0], RecordingEvent::Started(started));
        let Some(&RecordingEvent::Encoding(last)) = told.last() else {
            panic!("the last thing told is how far encoding got: {told:?}");
        };
        assert_eq!((last.attempt, last.progress.done), (1, last.progress.total));
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
            scope.spawn(once_recording(&recorder, Duration::from_millis(300), || recorder.cancel()));
            recorder.record_region(SMALL, (160, 90), GifFps::Fps10, LONG, SETTINGS, &mut |_| {})
        });
        assert_eq!(gif, Ok(None));
    }

    #[test]
    #[ignore = "needs an unlocked desktop; run with --ignored"]
    fn a_cancel_while_the_screen_is_being_opened_records_nothing() {
        let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner);
        let recorder = Recorder::default();
        let _session = recorder.begin().unwrap();
        recorder.cancel();
        let mut told = Vec::new();
        let started = Instant::now();

        let gif = recorder.record_region(SMALL, (160, 90), GifFps::Fps10, LONG, SETTINGS, &mut |event| told.push(event));

        assert_eq!((gif, told), (Ok(None), vec![]));
        assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
    }

    #[test]
    #[ignore = "needs an unlocked desktop; run with --ignored"]
    fn a_recording_left_alone_ends_at_its_limit_and_says_so() {
        let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner);
        let recorder = Recorder::default();
        let _session = recorder.begin().unwrap();
        let three_frames = RecordingLimit { frames: 3, by: LimitedBy::Disk };
        let mut told = Vec::new();

        let recorded = recorder
            .record_region(SMALL, (160, 90), GifFps::Fps10, three_frames, SETTINGS, &mut |event| told.push(event))
            .unwrap()
            .expect("a recording that reached its limit gives a GIF");

        assert!(recorded.ended_at_limit);
        let RecordingEvent::Started(started) = told[0] else {
            panic!("the first thing told is that recording started: {told:?}");
        };
        assert_eq!((started.limit_ms, started.limited_by), (300, LimitedBy::Disk));
    }
}
