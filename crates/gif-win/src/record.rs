use std::time::{Duration, Instant};

use gif_core::GifFps;
use gif_core::resize::downscale;
use gif_core::schedule::{frame_due, slot_at};
use gif_core::session::ScreenRect;
use gif_core::writer::Rgb;

use crate::{CaptureError, ScreenCapturer};

pub fn record_screen(
    capturer: &mut ScreenCapturer,
    region: ScreenRect,
    output_size: (u32, u32),
    fps: GifFps,
    max_frames: u32,
    keep_going: impl FnMut() -> bool,
    keep: impl FnMut(Vec<Rgb>, u32),
) -> Result<(), CaptureError> {
    let started = Instant::now();
    let sleep_until = |due: Duration| std::thread::sleep((started + due).saturating_duration_since(Instant::now()));
    let grab = || {
        let pixels = capturer.capture(region)?;
        downscale(&pixels, (region.width, region.height), output_size)
            .map_err(|error| CaptureError::System(format!("the frame could not be resized: {error:?}")))
    };
    let elapsed = || started.elapsed();
    record_frames(fps, max_frames, Clock { wait_until: sleep_until, elapsed }, grab, keep_going, keep)
}

struct Clock<Wait, Elapsed> {
    wait_until: Wait,
    elapsed: Elapsed,
}

fn record_frames<Frame, Error>(
    fps: GifFps,
    max_slots: u32,
    mut clock: Clock<impl FnMut(Duration), impl FnMut() -> Duration>,
    mut grab: impl FnMut() -> Result<Frame, Error>,
    mut keep_going: impl FnMut() -> bool,
    mut keep: impl FnMut(Frame, u32),
) -> Result<(), Error> {
    let mut slot = 0;
    while slot < max_slots {
        (clock.wait_until)(frame_due(fps, slot));
        if !keep_going() {
            break;
        }
        keep(grab()?, slot);
        slot = (slot + 1).max(slot_at(fps, (clock.elapsed)()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Debug, PartialEq)]
    enum Step {
        WaitUntil(Duration),
        Grab(u32),
    }

    fn kept<Frame, Error>(
        fps: GifFps,
        max_frames: u32,
        grab: impl FnMut() -> Result<Frame, Error>,
        wait_until: impl FnMut(Duration),
        keep_going: impl FnMut() -> bool,
    ) -> Result<Vec<Frame>, Error> {
        let mut frames = Vec::new();
        let clock = Clock { wait_until, elapsed: || Duration::ZERO };
        record_frames(fps, max_frames, clock, grab, keep_going, |frame, _| frames.push(frame)).map(|()| frames)
    }

    struct FakeTime(std::cell::Cell<Duration>);

    impl FakeTime {
        fn pass(&self, time: Duration) {
            self.0.set(self.0.get() + time);
        }
    }

    fn slots_kept(fps: GifFps, max_slots: u32, grab_takes_ms: &[u64]) -> Vec<(u32, Duration)> {
        let now = FakeTime(std::cell::Cell::new(Duration::ZERO));
        let clock = Clock { wait_until: |due: Duration| now.0.set(now.0.get().max(due)), elapsed: || now.0.get() };
        let mut grabs = grab_takes_ms.iter();
        let grab = || {
            let taken_at = now.0.get();
            now.pass(ms(*grabs.next().expect("no more grabs than planned")));
            Ok::<Duration, ()>(taken_at)
        };
        let mut kept = Vec::new();
        record_frames(fps, max_slots, clock, grab, || true, |taken_at, slot| kept.push((slot, taken_at))).unwrap();
        kept
    }

    #[test]
    fn frames_taken_on_time_fill_every_slot() {
        let kept = slots_kept(GifFps::Fps10, 4, &[10, 10, 10, 10]);
        assert_eq!(kept, [(0, ms(0)), (1, ms(100)), (2, ms(200)), (3, ms(300))]);
    }

    #[test]
    fn a_slow_frame_skips_the_slots_it_ran_over_and_the_next_is_taken_at_once() {
        let kept = slots_kept(GifFps::Fps10, 6, &[10, 250, 10, 10, 10]);
        assert_eq!(kept, [(0, ms(0)), (1, ms(100)), (3, ms(350)), (4, ms(400)), (5, ms(500))]);
    }

    #[test]
    fn a_slow_frame_does_not_make_the_recording_run_past_its_last_slot() {
        let kept = slots_kept(GifFps::Fps10, 5, &[10, 10, 330, 10]);
        assert_eq!(kept.iter().map(|&(slot, _)| slot).collect::<Vec<_>>(), [0, 1, 2]);
        assert!(kept.last().unwrap().1 < frame_due(GifFps::Fps10, 5));
    }

    #[test]
    fn frames_grabbed_before_a_failure_were_already_handed_over() {
        let mut grabs = 0;
        let grab = || {
            grabs += 1;
            if grabs == 3 { Err("screen went away") } else { Ok(grabs) }
        };
        let mut handed_over = Vec::new();

        let clock = Clock { wait_until: |_| {}, elapsed: || Duration::ZERO };
        let ended = record_frames(GifFps::Fps30, 5, clock, grab, || true, |frame, _| handed_over.push(frame));

        assert_eq!((ended, handed_over), (Err("screen went away"), vec![1, 2]));
    }

    fn ms(milliseconds: u64) -> Duration {
        Duration::from_millis(milliseconds)
    }

    #[test]
    fn every_frame_is_grabbed_once_in_order() {
        let mut next = 0;
        let grab = || {
            next += 1;
            Ok::<u32, ()>(next)
        };
        assert_eq!(kept(GifFps::Fps10, 3, grab, |_| {}, || true), Ok(vec![1, 2, 3]));
    }

    #[test]
    fn each_grab_waits_for_its_own_due_time_first() {
        let steps = RefCell::new(Vec::new());
        let mut next = 0;
        let grab = || {
            next += 1;
            steps.borrow_mut().push(Step::Grab(next));
            Ok::<u32, ()>(next)
        };
        let wait_until = |due| steps.borrow_mut().push(Step::WaitUntil(due));

        kept(GifFps::Fps10, 3, grab, wait_until, || true).unwrap();

        let expected = vec![
            Step::WaitUntil(ms(0)),
            Step::Grab(1),
            Step::WaitUntil(ms(100)),
            Step::Grab(2),
            Step::WaitUntil(ms(200)),
            Step::Grab(3),
        ];
        assert_eq!(steps.into_inner(), expected);
    }

    #[test]
    fn a_failed_grab_stops_the_recording_with_that_error() {
        let mut grabs = 0;
        let grab = || {
            grabs += 1;
            if grabs == 2 { Err("screen went away") } else { Ok(grabs) }
        };
        assert_eq!(kept(GifFps::Fps30, 5, grab, |_| {}, || true), Err("screen went away"));
        assert_eq!(grabs, 2);
    }

    #[test]
    fn recording_ends_when_told_to_stop() {
        let mut next = 0;
        let grab = || {
            next += 1;
            Ok::<u32, ()>(next)
        };
        let mut asked = 0;
        let keep_going = || {
            asked += 1;
            asked <= 2
        };
        assert_eq!(kept(GifFps::Fps10, 100, grab, |_| {}, keep_going), Ok(vec![1, 2]));
    }

    #[test]
    fn the_stop_is_checked_after_the_wait_so_a_stop_during_it_grabs_nothing_more() {
        let stopped = std::cell::Cell::new(false);
        let mut grabs = 0;
        let grab = || {
            grabs += 1;
            Ok::<u32, ()>(grabs)
        };
        let wait_until = |due: Duration| stopped.set(due >= ms(100));

        let frames = kept(GifFps::Fps10, 100, grab, wait_until, || !stopped.get());

        assert_eq!(frames, Ok(vec![1]));
    }

    #[test]
    fn a_stop_before_the_first_frame_gives_no_frames() {
        let grab = || -> Result<u32, ()> { panic!("nothing should be grabbed") };
        assert_eq!(kept(GifFps::Fps10, 100, grab, |_| {}, || false), Ok(vec![]));
    }

    #[test]
    fn no_frames_asked_means_nothing_is_grabbed() {
        let grab = || -> Result<u32, ()> { panic!("nothing should be grabbed") };
        assert_eq!(kept(GifFps::Fps30, 0, grab, |_| {}, || true), Ok(vec![]));
    }

    #[test]
    #[ignore = "needs an unlocked desktop; run with --ignored"]
    fn half_a_second_of_screen_takes_about_half_a_second() {
        let (_alone, mut capturer) = crate::desktop::open_primary();
        let region = ScreenRect { monitor: crate::desktop::PRIMARY, x: 0, y: 0, width: 64, height: 48 };

        let started = Instant::now();
        let mut frames = Vec::new();
        record_screen(&mut capturer, region, (32, 24), GifFps::Fps10, 6, || true, |frame, _| frames.push(frame)).unwrap();
        let took = started.elapsed();

        assert_eq!(frames.len(), 6);
        assert!(frames.iter().all(|frame| frame.len() == 32 * 24));
        assert!(took >= ms(500) && took < ms(900), "{took:?}");
    }
}
