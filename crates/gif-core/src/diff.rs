#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FramePatch {
    pub left: usize,
    pub top: usize,
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}

impl FramePatch {
    pub(crate) fn whole_frame(pixels: &[u8], width: usize) -> FramePatch {
        FramePatch { left: 0, top: 0, width, height: pixels.len() / width, pixels: pixels.to_vec() }
    }
}

pub(crate) fn changed_patch(previous: &[u8], current: &[u8], width: usize, transparent: u8) -> FramePatch {
    let changed = |index: &usize| previous[*index] != current[*index];
    let changed_columns = || (0..current.len()).filter(changed).map(|index| index % width);
    let changed_rows = || (0..current.len()).filter(changed).map(|index| index / width);

    let (Some(left), Some(right), Some(top), Some(bottom)) = (
        changed_columns().min(),
        changed_columns().max(),
        changed_rows().min(),
        changed_rows().max(),
    ) else {
        return FramePatch { left: 0, top: 0, width: 1, height: 1, pixels: vec![transparent] };
    };

    let pixels = (top..=bottom)
        .flat_map(|y| (left..=right).map(move |x| y * width + x))
        .map(|index| if changed(&index) { current[index] } else { transparent })
        .collect();
    FramePatch { left, top, width: right - left + 1, height: bottom - top + 1, pixels }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: u8 = 9;

    #[rustfmt::skip]
    const BEFORE: [u8; 20] = [
        0, 0, 0, 0, 0,
        0, 1, 1, 1, 0,
        0, 1, 1, 1, 0,
        0, 0, 0, 0, 0,
    ];

    #[test]
    fn one_changed_pixel_becomes_a_one_pixel_patch() {
        let mut after = BEFORE;
        after[2 * 5 + 3] = 2;
        assert_eq!(
            changed_patch(&BEFORE, &after, 5, T),
            FramePatch { left: 3, top: 2, width: 1, height: 1, pixels: vec![2] }
        );
    }

    #[test]
    fn the_patch_is_the_smallest_rectangle_around_every_change() {
        let mut after = BEFORE;
        after[5 + 1] = 2;
        after[3 * 5 + 3] = 3;
        #[rustfmt::skip]
        let expected = vec![
            2, T, T,
            T, T, T,
            T, T, 3,
        ];
        assert_eq!(
            changed_patch(&BEFORE, &after, 5, T),
            FramePatch { left: 1, top: 1, width: 3, height: 3, pixels: expected }
        );
    }

    #[test]
    fn an_unchanged_frame_becomes_one_transparent_pixel() {
        assert_eq!(
            changed_patch(&BEFORE, &BEFORE, 5, T),
            FramePatch { left: 0, top: 0, width: 1, height: 1, pixels: vec![T] }
        );
    }

    #[test]
    fn a_frame_that_changed_everywhere_is_kept_whole() {
        let after = BEFORE.map(|index| index + 4);
        assert_eq!(changed_patch(&BEFORE, &after, 5, T), FramePatch::whole_frame(&after, 5));
    }
}
