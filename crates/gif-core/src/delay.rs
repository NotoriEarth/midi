use crate::GifFps;

const CENTISECONDS_PER_SECOND: u64 = 100;

pub fn frame_delays(fps: GifFps, frame_count: usize) -> Vec<u16> {
    let fps = u64::from(fps.per_second());
    let end_of_frame = |index: u64| (index + 1) * CENTISECONDS_PER_SECOND / fps;

    (0..frame_count as u64)
        .map(|index| {
            let start = if index == 0 {
                0
            } else {
                end_of_frame(index - 1)
            };
            (end_of_frame(index) - start) as u16
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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
