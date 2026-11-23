use crate::GifFps;

const CENTISECONDS_PER_SECOND: u64 = 100;

pub fn frame_delays(fps: GifFps, frame_count: usize) -> Vec<u16> {
    (0..frame_count).map(|index| frame_delay(fps, index)).collect()
}

pub fn slots_delay(fps: GifFps, slots: std::ops::Range<usize>) -> u16 {
    let fps = u64::from(fps.per_second());
    let start_of = |slot: usize| slot as u64 * CENTISECONDS_PER_SECOND / fps;
    let shown = start_of(slots.end).saturating_sub(start_of(slots.start));
    shown.min(u64::from(u16::MAX)) as u16
}

pub fn frame_delay(fps: GifFps, index: usize) -> u16 {
    slots_delay(fps, index..index + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_covering_several_slots_is_shown_for_all_of_them() {
        assert_eq!(slots_delay(GifFps::Fps10, 1..4), 30);
        assert_eq!(slots_delay(GifFps::Fps30, 0..3), 10);
        assert_eq!(slots_delay(GifFps::Fps30, 2..5), 3 + 4 + 3);
    }

    #[test]
    fn covering_one_slot_is_the_same_as_the_frame_delay() {
        for fps in GifFps::ALL {
            for slot in 0..100 {
                assert_eq!(slots_delay(fps, slot..slot + 1), frame_delay(fps, slot), "{fps:?} slot {slot}");
            }
        }
    }

    #[test]
    fn slots_that_add_up_to_a_second_are_shown_for_a_second_wherever_they_start() {
        for fps in GifFps::ALL {
            let second = fps.per_second() as usize;
            for start in 0..second {
                let shown: u32 = (start..start + second).map(|slot| u32::from(frame_delay(fps, slot))).sum();
                assert_eq!(shown, 100, "{fps:?} from slot {start}");
                assert_eq!(slots_delay(fps, start..start + second), 100, "{fps:?} from slot {start}");
            }
        }
    }

    #[test]
    fn ten_fps_divides_evenly_so_every_frame_gets_ten() {
        assert_eq!(frame_delays(GifFps::Fps10, 4), [10, 10, 10, 10]);
    }

    #[test]
    fn thirty_fps_repeats_three_three_four() {
        assert_eq!(frame_delays(GifFps::Fps30, 6), [3, 3, 4, 3, 3, 4]);
    }

    #[test]
    fn fifteen_fps_repeats_six_seven_seven() {
        assert_eq!(frame_delays(GifFps::Fps15, 6), [6, 7, 7, 6, 7, 7]);
    }

    #[test]
    fn one_second_of_frames_always_adds_up_to_one_second() {
        for fps in GifFps::ALL {
            let delays = frame_delays(fps, fps.per_second() as usize);
            let total: u32 = delays.iter().map(|&d| u32::from(d)).sum();
            assert_eq!(total, 100, "{fps:?}");
        }
    }

    #[test]
    fn ten_minutes_of_frames_do_not_drift() {
        let delays = frame_delays(GifFps::Fps30, 30 * 600);
        let total: u32 = delays.iter().map(|&d| u32::from(d)).sum();
        assert_eq!(total, 100 * 600);
    }

    #[test]
    fn no_frames_means_no_delays() {
        assert!(frame_delays(GifFps::Fps30, 0).is_empty());
    }
}
