use crate::writer::Rgb;

pub const DARKEST_VISIBLE: u8 = 9;

pub fn looks_black(pixels: &[Rgb]) -> bool {
    !pixels.is_empty() && pixels.iter().flatten().all(|&channel| channel < DARKEST_VISIBLE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_of_pure_black_looks_black() {
        assert!(looks_black(&[[0, 0, 0]; 64]));
    }

    #[test]
    fn noise_just_above_black_still_looks_black() {
        let almost: Vec<Rgb> = (0..64).map(|index| [(index % 9) as u8, 0, (index % 5) as u8]).collect();
        assert!(looks_black(&almost));
    }

    #[test]
    fn one_visible_pixel_anywhere_is_enough_to_show_a_picture() {
        for place in [0, 31, 63] {
            let mut pixels = vec![[0, 0, 0]; 64];
            pixels[place] = [0, DARKEST_VISIBLE, 0];
            assert!(!looks_black(&pixels), "a visible pixel at {place}");
        }
    }

    #[test]
    fn an_ordinary_dark_scene_is_not_black() {
        let night: Vec<Rgb> = (0..64).map(|index| [20, 24, 40 + (index % 10) as u8]).collect();
        assert!(!looks_black(&night));
    }

    #[test]
    fn a_frame_with_no_pixels_is_not_called_black() {
        assert!(!looks_black(&[]));
    }
}
