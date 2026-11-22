use std::time::{Duration, Instant};

use gif_core::GifFps;
use gif_core::schedule::frame_due;
use gif_core::session::ScreenRect;
use gif_core::writer::Rgb;

use crate::{CaptureError, ScreenCapturer};

pub fn record_screen(
    capturer: &mut ScreenCapturer,
    region: ScreenRect,
    fps: GifFps,
    max_frames: u32,
    keep_going: impl FnMut() -> bool,
) -> Result<Vec<Vec<Rgb>>, CaptureError> {
    let started = Instant::now();
    let sleep_until = |due: Duration| std::thread::sleep((started + due).saturating_duration_since(Instant::now()));
    record_frames(fps, max_frames, || capturer.capture(region), sleep_until, keep_going)
}

fn record_frames<Frame, Error>(
    fps: GifFps,
    max_frames: u32,
    mut grab: impl FnMut() -> Result<Frame, Error>,
    mut wait_until: impl FnMut(Duration),
    mut keep_going: impl FnMut() -> bool,
) -> Result<Vec<Frame>, Error> {
    let mut frames = Vec::new();
    for index in 0..max_frames {
        wait_until(frame_due(fps, index));
        if !keep_going() {
            break;
        }
        frames.push(grab()?);
    }
    Ok(frames)
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
        assert_eq!(record_frames(GifFps::Fps10, 3, grab, |_| {}, || true), Ok(vec![1, 2, 3]));
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

        record_frames(GifFps::Fps10, 3, grab, wait_until, || true).unwrap();

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
        assert_eq!(record_frames(GifFps::Fps30, 5, grab, |_| {}, || true), Err("screen went away"));
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
        assert_eq!(record_frames(GifFps::Fps10, 100, grab, |_| {}, keep_going), Ok(vec![1, 2]));
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

        let frames = record_frames(GifFps::Fps10, 100, grab, wait_until, || !stopped.get());

        assert_eq!(frames, Ok(vec![1]));
    }

    #[test]
    fn a_stop_before_the_first_frame_gives_no_frames() {
        let grab = || -> Result<u32, ()> { panic!("nothing should be grabbed") };
        assert_eq!(record_frames(GifFps::Fps10, 100, grab, |_| {}, || false), Ok(vec![]));
    }

    #[test]
    fn no_frames_asked_means_nothing_is_grabbed() {
        let grab = || -> Result<u32, ()> { panic!("nothing should be grabbed") };
        assert_eq!(record_frames(GifFps::Fps30, 0, grab, |_| {}, || true), Ok(vec![]));
    }

    #[test]
    #[ignore = "needs an unlocked desktop; run with --ignored"]
    fn half_a_second_of_screen_takes_about_half_a_second() {
        let (_alone, mut capturer) = crate::desktop::open_primary();
        let region = ScreenRect { monitor: crate::desktop::PRIMARY, x: 0, y: 0, width: 64, height: 48 };

        let started = Instant::now();
        let frames = record_screen(&mut capturer, region, GifFps::Fps10, 6, || true).unwrap();
        let took = started.elapsed();

        assert_eq!(frames.len(), 6);
        assert!(frames.iter().all(|frame| frame.len() == 64 * 48));
        assert!(took >= ms(500) && took < ms(900), "{took:?}");
    }
}
