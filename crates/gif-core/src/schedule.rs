use std::time::Duration;

use crate::GifFps;

const NANOSECONDS_PER_SECOND: u64 = 1_000_000_000;

pub fn frame_due(fps: GifFps, index: u32) -> Duration {
    let nanoseconds = u64::from(index) * NANOSECONDS_PER_SECOND / u64::from(fps.per_second());
    Duration::from_nanos(nanoseconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_frame_is_due_at_once() {
        for fps in GifFps::ALL {
            assert_eq!(frame_due(fps, 0), Duration::ZERO, "{fps:?}");
        }
    }

    #[test]
    fn ten_fps_frames_are_a_tenth_of_a_second_apart() {
        assert_eq!(frame_due(GifFps::Fps10, 1), Duration::from_millis(100));
        assert_eq!(frame_due(GifFps::Fps10, 7), Duration::from_millis(700));
    }

    #[test]
    fn thirty_fps_keeps_the_third_of_a_millisecond_a_rounded_step_would_lose() {
        assert_eq!(frame_due(GifFps::Fps30, 1), Duration::from_nanos(33_333_333));
        assert_eq!(frame_due(GifFps::Fps30, 2), Duration::from_nanos(66_666_666));
        assert_eq!(frame_due(GifFps::Fps30, 3), Duration::from_millis(100));
    }

    #[test]
    fn one_second_of_frames_ends_exactly_on_the_second() {
        for fps in GifFps::ALL {
            assert_eq!(frame_due(fps, fps.per_second()), Duration::from_secs(1), "{fps:?}");
        }
    }

    #[test]
    fn an_hour_in_the_due_time_has_not_drifted() {
        assert_eq!(frame_due(GifFps::Fps15, 15 * 3600), Duration::from_secs(3600));
    }
}
