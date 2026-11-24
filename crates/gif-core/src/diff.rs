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

    pub(crate) fn under(&self, frame: &[u8], frame_width: usize) -> Vec<u8> {
        let mut under = Vec::with_capacity(self.width * self.height);
        for y in self.top..self.top + self.height {
            under.extend_from_slice(&frame[y * frame_width + self.left..][..self.width]);
        }
        under
    }

    pub(crate) fn paint_over(&self, frame: &mut [u8], frame_width: usize, transparent: u8) {
        for (row, patch_row) in self.pixels.chunks_exact(self.width).enumerate() {
            let start = (self.top + row) * frame_width + self.left;
            for (shown, &pixel) in frame[start..start + self.width].iter_mut().zip(patch_row) {
                if pixel != transparent {
                    *shown = pixel;
                }
            }
        }
    }
}

pub(crate) fn changed_patch(previous: &[u8], current: &[u8], width: usize, transparent: u8) -> FramePatch {
    let rows = previous.chunks_exact(width).zip(current.chunks_exact(width));
    let mut bounds: Option<(usize, usize, usize, usize)> = None;
    for (y, (before, after)) in rows.enumerate() {
        let differs = |(a, b): (&u8, &u8)| a != b;
        let Some(first) = before.iter().zip(after).position(differs) else {
            continue;
        };
        let last = before.iter().zip(after).rposition(differs).expect("a row with a change has a last change");
        bounds = Some(match bounds {
            None => (first, last, y, y),
            Some((left, right, top, _)) => (left.min(first), right.max(last), top, y),
        });
    }
    let Some((left, right, top, bottom)) = bounds else {
        return FramePatch { left: 0, top: 0, width: 1, height: 1, pixels: vec![transparent] };
    };

    let mut pixels = Vec::with_capacity((right - left + 1) * (bottom - top + 1));
    for y in top..=bottom {
        let span = y * width + left..=y * width + right;
        let shown = previous[span.clone()].iter().zip(&current[span]);
        pixels.extend(shown.map(|(&before, &after)| if before != after { after } else { transparent }));
    }
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
