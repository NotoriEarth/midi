use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::ops::Range;
use std::os::windows::fs::OpenOptionsExt;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};

use gif_core::encode::{FrameError, FrameSource};
use gif_core::writer::Rgb;

const BYTES_PER_PIXEL: u64 = size_of::<Rgb>() as u64;
const FILE_FLAG_DELETE_ON_CLOSE: u32 = 0x0400_0000;

static SPOOLS_MADE: AtomicU32 = AtomicU32::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChangedRows {
    runs: Vec<Range<usize>>,
    starts_at: u64,
}

#[derive(Debug)]
pub struct Spool {
    file: File,
    width: usize,
    pixel_count: usize,
    changes: Vec<ChangedRows>,
    bytes_written: u64,
    shown: Vec<Rgb>,
    shown_frame: Option<usize>,
    passing: Vec<Rgb>,
}

impl Spool {
    pub fn create_in(folder: &Path, (width, height): (u32, u32)) -> io::Result<Spool> {
        let number = SPOOLS_MADE.fetch_add(1, Ordering::Relaxed);
        let name = format!("midi-recording-{}-{number}.frames", std::process::id());
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .custom_flags(FILE_FLAG_DELETE_ON_CLOSE)
            .open(folder.join(name))?;
        Ok(Spool {
            file,
            width: width as usize,
            pixel_count: (width * height) as usize,
            changes: Vec::new(),
            bytes_written: 0,
            shown: Vec::new(),
            shown_frame: None,
            passing: Vec::new(),
        })
    }

    pub fn frame_bytes((width, height): (u32, u32)) -> u64 {
        u64::from(width) * u64::from(height) * BYTES_PER_PIXEL
    }

    pub fn bytes_written(&self) -> u64 {
        self.bytes_written
    }

    pub fn push(&mut self, pixels: &[Rgb]) -> io::Result<()> {
        if pixels.len() != self.pixel_count {
            let problem = format!("a frame of {} pixels in a spool of {}-pixel frames", pixels.len(), self.pixel_count);
            return Err(io::Error::new(io::ErrorKind::InvalidInput, problem));
        }
        let runs = match self.changes.len().checked_sub(1) {
            Some(last) => {
                self.show(last)?;
                changed_runs(&self.shown, pixels, self.width)
            }
            None => std::iter::once(0..self.pixel_count / self.width.max(1)).collect(),
        };

        self.passing.clear();
        for rows in &runs {
            self.passing.extend_from_slice(&pixels[pixels_of(rows, self.width)]);
        }
        self.file.seek(SeekFrom::Start(self.bytes_written))?;
        self.file.write_all(self.passing.as_flattened())?;

        self.shown.resize(self.pixel_count, [0; 3]);
        for rows in &runs {
            let changed = pixels_of(rows, self.width);
            self.shown[changed.clone()].copy_from_slice(&pixels[changed]);
        }
        self.shown_frame = Some(self.changes.len());
        self.changes.push(ChangedRows { runs, starts_at: self.bytes_written });
        self.bytes_written += self.passing.len() as u64 * BYTES_PER_PIXEL;
        Ok(())
    }

    fn show(&mut self, frame: usize) -> io::Result<()> {
        let first_to_apply = match self.shown_frame {
            Some(shown) if shown <= frame => shown + 1,
            _ => 0,
        };
        self.shown_frame = None;
        self.shown.resize(self.pixel_count, [0; 3]);
        for index in first_to_apply..=frame {
            let ChangedRows { runs, starts_at } = &self.changes[index];
            let changed_pixels: usize = runs.iter().map(|rows| pixels_of(rows, self.width).len()).sum();
            self.passing.resize(changed_pixels, [0; 3]);
            self.file.seek(SeekFrom::Start(*starts_at))?;
            self.file.read_exact(self.passing.as_flattened_mut())?;

            let mut read = 0;
            for rows in runs {
                let changed = pixels_of(rows, self.width);
                let next = read + changed.len();
                self.shown[changed].copy_from_slice(&self.passing[read..next]);
                read = next;
            }
        }
        self.shown_frame = Some(frame);
        Ok(())
    }
}

fn pixels_of(rows: &Range<usize>, width: usize) -> Range<usize> {
    rows.start * width..rows.end * width
}

fn changed_runs(before: &[Rgb], after: &[Rgb], width: usize) -> Vec<Range<usize>> {
    let mut runs: Vec<Range<usize>> = Vec::new();
    let rows = before.chunks(width).zip(after.chunks(width));
    for (row, _) in rows.enumerate().filter(|(_, (old, new))| old != new) {
        match runs.last_mut() {
            Some(run) if run.end == row => run.end = row + 1,
            _ => runs.push(row..row + 1),
        }
    }
    runs
}

impl FrameSource for Spool {
    fn frame_count(&self) -> usize {
        self.changes.len()
    }

    fn read_frame(&mut self, index: usize, pixels: &mut Vec<Rgb>) -> Result<(), FrameError> {
        if index >= self.changes.len() {
            return Err(FrameError::Unreadable);
        }
        self.show(index).map_err(|_| FrameError::Unreadable)?;
        pixels.clear();
        pixels.extend_from_slice(&self.shown);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gif_core::GifFps;
    use gif_core::encode::{ManualSettings, encode_frames, encode_gif};
    use std::fs;
    use std::path::PathBuf;

    const SIZE: (u32, u32) = (8, 6);
    const PIXELS: usize = 8 * 6;
    const ROW_BYTES: u64 = 8 * 3;

    fn empty_folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("midi-spool-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn frame(number: u8) -> Vec<Rgb> {
        (0..PIXELS).map(|index| [index as u8 * 5, number * 40, 90]).collect()
    }

    fn read(spool: &mut Spool, index: usize) -> Result<Vec<Rgb>, FrameError> {
        let mut pixels = vec![[1, 2, 3]; 5];
        spool.read_frame(index, &mut pixels).map(|()| pixels)
    }

    fn with_rows_changed(mut pixels: Vec<Rgb>, rows: std::ops::Range<usize>, to: Rgb) -> Vec<Rgb> {
        pixels[rows.start * 8 + 3..rows.end * 8 - 2].fill(to);
        pixels
    }

    #[test]
    fn only_the_rows_that_changed_since_the_frame_before_are_written() {
        let mut spool = Spool::create_in(&empty_folder("changed-rows"), SIZE).unwrap();
        let first = frame(0);
        let two_rows_changed = with_rows_changed(first.clone(), 2..4, [9, 9, 9]);

        spool.push(&first).unwrap();
        assert_eq!(spool.bytes_written(), 6 * ROW_BYTES, "the first frame is written whole");
        spool.push(&two_rows_changed).unwrap();
        assert_eq!(spool.bytes_written(), 8 * ROW_BYTES, "rows 2 and 3");
        spool.push(&two_rows_changed).unwrap();
        assert_eq!(spool.bytes_written(), 8 * ROW_BYTES, "nothing changed, nothing written");
        spool.push(&first).unwrap();
        assert_eq!(spool.bytes_written(), 10 * ROW_BYTES, "rows 2 and 3 changed back");
    }

    #[test]
    fn rows_that_changed_far_apart_are_written_without_the_rows_between_them() {
        let mut spool = Spool::create_in(&empty_folder("far-apart"), SIZE).unwrap();
        let first = frame(0);
        let top_and_bottom = with_rows_changed(with_rows_changed(first.clone(), 0..1, [3, 3, 3]), 5..6, [4, 4, 4]);

        spool.push(&first).unwrap();
        spool.push(&top_and_bottom).unwrap();

        assert_eq!(spool.bytes_written(), (6 + 2) * ROW_BYTES);
        assert_eq!(read(&mut spool, 1), Ok(top_and_bottom));
    }

    #[test]
    fn frames_that_changed_only_in_places_come_back_whole_in_any_order() {
        let mut spool = Spool::create_in(&empty_folder("in-places"), SIZE).unwrap();
        let first = frame(0);
        let frames = [
            first.clone(),
            with_rows_changed(first.clone(), 0..1, [1, 1, 1]),
            with_rows_changed(first.clone(), 0..1, [1, 1, 1]),
            with_rows_changed(first.clone(), 5..6, [2, 2, 2]),
            with_rows_changed(with_rows_changed(first.clone(), 1..2, [3, 3, 3]), 4..5, [4, 4, 4]),
            first.clone(),
        ];
        for pixels in &frames {
            spool.push(pixels).unwrap();
        }

        for index in [0, 1, 2, 3, 4, 5, 0, 4, 2, 5, 3, 3, 1] {
            assert_eq!(read(&mut spool, index).as_ref(), Ok(&frames[index].to_vec()), "frame {index}");
        }
    }

    #[test]
    fn pushing_after_reading_an_earlier_frame_still_compares_with_the_last_one() {
        let mut spool = Spool::create_in(&empty_folder("push-after-earlier"), SIZE).unwrap();
        let first = frame(0);
        let second = with_rows_changed(first.clone(), 2..4, [9, 9, 9]);
        let third = with_rows_changed(second.clone(), 5..6, [7, 7, 7]);
        spool.push(&first).unwrap();
        spool.push(&second).unwrap();
        read(&mut spool, 0).unwrap();

        spool.push(&third).unwrap();

        assert_eq!(spool.bytes_written(), 9 * ROW_BYTES, "only row 5 was added");
        assert_eq!((read(&mut spool, 1), read(&mut spool, 2)), (Ok(second), Ok(third)));
    }

    fn files_in(folder: &Path) -> usize {
        fs::read_dir(folder).unwrap().count()
    }

    #[test]
    fn frames_come_back_as_they_were_pushed_in_any_order_and_more_than_once() {
        let mut spool = Spool::create_in(&empty_folder("back"), SIZE).unwrap();
        for number in 0..4 {
            spool.push(&frame(number)).unwrap();
        }

        assert_eq!(spool.frame_count(), 4);
        for index in [2, 0, 3, 1, 2, 0] {
            assert_eq!(read(&mut spool, index), Ok(frame(index as u8)), "frame {index}");
        }
    }

    #[test]
    fn a_frame_can_still_be_pushed_after_reading() {
        let mut spool = Spool::create_in(&empty_folder("after-reading"), SIZE).unwrap();
        spool.push(&frame(0)).unwrap();
        spool.push(&frame(1)).unwrap();
        read(&mut spool, 0).unwrap();

        spool.push(&frame(2)).unwrap();

        assert_eq!((read(&mut spool, 1), read(&mut spool, 2)), (Ok(frame(1)), Ok(frame(2))));
    }

    #[test]
    fn a_frame_of_another_size_is_refused_and_not_counted() {
        let mut spool = Spool::create_in(&empty_folder("wrong-size"), SIZE).unwrap();
        spool.push(&frame(0)).unwrap();

        let refused = spool.push(&frame(1)[..PIXELS - 1]);

        assert_eq!(refused.map_err(|error| error.kind()), Err(io::ErrorKind::InvalidInput));
        assert_eq!(spool.frame_count(), 1);
    }

    #[test]
    fn a_frame_that_was_never_pushed_cannot_be_read() {
        let mut spool = Spool::create_in(&empty_folder("past-the-end"), SIZE).unwrap();
        spool.push(&frame(0)).unwrap();
        assert_eq!(read(&mut spool, 1), Err(FrameError::Unreadable));
    }

    #[test]
    fn the_file_is_on_disk_while_the_spool_lives_and_gone_afterwards() {
        let folder = empty_folder("gone");
        let mut spool = Spool::create_in(&folder, SIZE).unwrap();
        spool.push(&frame(0)).unwrap();
        assert_eq!(files_in(&folder), 1);

        drop(spool);

        assert_eq!(files_in(&folder), 0);
    }

    #[test]
    fn two_spools_in_one_folder_keep_their_own_frames() {
        let folder = empty_folder("two");
        let mut first = Spool::create_in(&folder, SIZE).unwrap();
        let mut second = Spool::create_in(&folder, SIZE).unwrap();
        first.push(&frame(1)).unwrap();
        second.push(&frame(2)).unwrap();

        assert_eq!((read(&mut first, 0), read(&mut second, 0)), (Ok(frame(1)), Ok(frame(2))));
    }

    #[test]
    fn a_folder_that_is_not_there_is_an_error() {
        let missing = empty_folder("missing").join("nowhere");
        assert_eq!(Spool::create_in(&missing, SIZE).map_err(|error| error.kind()).err(), Some(io::ErrorKind::NotFound));
    }

    #[test]
    fn encoding_from_the_spool_gives_the_same_gif_as_encoding_from_memory() {
        let frames: Vec<Vec<Rgb>> = (0..5).map(frame).collect();
        let slices: Vec<&[Rgb]> = frames.iter().map(Vec::as_slice).collect();
        let settings = ManualSettings { colors: 16, dither: 1.0, tolerance: 8 };
        let mut spool = Spool::create_in(&empty_folder("same-gif"), SIZE).unwrap();
        for pixels in &frames {
            spool.push(pixels).unwrap();
        }

        let from_disk = encode_frames(8, 6, &mut spool, GifFps::Fps10, settings, &mut |_| {});

        assert_eq!(from_disk, encode_gif(8, 6, &slices, GifFps::Fps10, settings));
    }
}
