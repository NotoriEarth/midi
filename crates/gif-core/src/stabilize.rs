use crate::writer::Rgb;

#[derive(Debug, Clone)]
pub struct Stabilizer {
    tolerance: u8,
    on_screen: Option<Vec<Rgb>>,
}

impl Stabilizer {
    pub fn new(tolerance: u8) -> Self {
        Self { tolerance, on_screen: None }
    }

    pub fn show(&mut self, frame: &[Rgb]) -> &[Rgb] {
        let tolerance = self.tolerance;
        match &mut self.on_screen {
            Some(on_screen) => {
                on_screen.truncate(frame.len());
                for (old, new) in on_screen.iter_mut().zip(frame) {
                    if !close(new, old, tolerance) {
                        *old = *new;
                    }
                }
                on_screen
            }
            none => none.insert(frame.to_vec()),
        }
    }
}

pub fn stabilize(frames: &[&[Rgb]], tolerance: u8) -> Vec<Vec<Rgb>> {
    let mut stabilizer = Stabilizer::new(tolerance);
    frames.iter().map(|frame| stabilizer.show(frame).to_vec()).collect()
}

fn close(a: &Rgb, b: &Rgb, tolerance: u8) -> bool {
    a.iter().zip(b).all(|(a, b)| a.abs_diff(*b) <= tolerance)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gray(level: u8) -> Rgb {
        [level, level, level]
    }

    fn one_pixel_film(levels: &[u8], tolerance: u8) -> Vec<u8> {
        let frames: Vec<[Rgb; 1]> = levels.iter().map(|&level| [gray(level)]).collect();
        let frames: Vec<&[Rgb]> = frames.iter().map(|frame| &frame[..]).collect();
        stabilize(&frames, tolerance).iter().map(|frame| frame[0][0]).collect()
    }

    #[test]
    fn no_tolerance_leaves_every_frame_as_it_was() {
        assert_eq!(one_pixel_film(&[100, 101, 99, 100], 0), [100, 101, 99, 100]);
    }

    #[test]
    fn a_change_within_the_tolerance_keeps_the_color_already_on_screen() {
        assert_eq!(one_pixel_film(&[100, 103, 97, 100], 3), [100, 100, 100, 100]);
    }

    #[test]
    fn a_change_beyond_the_tolerance_shows_the_new_color() {
        assert_eq!(one_pixel_film(&[100, 104, 104, 90], 3), [100, 104, 104, 90]);
    }

    #[test]
    fn a_slow_drift_is_measured_from_what_is_on_screen_not_from_the_frame_before() {
        assert_eq!(one_pixel_film(&[10, 12, 14, 16, 18], 3), [10, 10, 14, 14, 18]);
    }

    #[test]
    fn one_channel_out_of_tolerance_is_enough_to_change_the_pixel() {
        let first = [[100, 100, 100]];
        let second = [[101, 100, 110]];
        assert_eq!(stabilize(&[&first, &second], 3)[1], [[101, 100, 110]]);
    }

    #[test]
    fn each_pixel_is_judged_on_its_own() {
        let first = [gray(50), gray(50)];
        let second = [gray(52), gray(80)];
        assert_eq!(stabilize(&[&first, &second], 3)[1], [gray(50), gray(80)]);
    }

    #[test]
    fn no_frames_give_no_frames() {
        assert!(stabilize(&[], 3).is_empty());
    }
}
