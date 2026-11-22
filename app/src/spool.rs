use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};

use gif_core::encode::{FrameError, FrameSource};
use gif_core::writer::Rgb;

const BYTES_PER_PIXEL: u64 = size_of::<Rgb>() as u64;
const FILE_FLAG_DELETE_ON_CLOSE: u32 = 0x0400_0000;

static SPOOLS_MADE: AtomicU32 = AtomicU32::new(0);

#[derive(Debug)]
pub struct Spool {
    file: File,
    pixel_count: usize,
    frames: usize,
}

impl Spool {
    pub fn create_in(folder: &Path, pixel_count: usize) -> io::Result<Spool> {
        let number = SPOOLS_MADE.fetch_add(1, Ordering::Relaxed);
        let name = format!("midi-recording-{}-{number}.frames", std::process::id());
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .custom_flags(FILE_FLAG_DELETE_ON_CLOSE)
            .open(folder.join(name))?;
        Ok(Spool { file, pixel_count, frames: 0 })
    }

    pub fn frame_bytes(pixel_count: usize) -> u64 {
        pixel_count as u64 * BYTES_PER_PIXEL
    }

    pub fn push(&mut self, pixels: &[Rgb]) -> io::Result<()> {
        if pixels.len() != self.pixel_count {
            let problem = format!("a frame of {} pixels in a spool of {}-pixel frames", pixels.len(), self.pixel_count);
            return Err(io::Error::new(io::ErrorKind::InvalidInput, problem));
        }
        self.file.seek(SeekFrom::Start(self.start_of(self.frames)))?;
        self.file.write_all(pixels.as_flattened())?;
        self.frames += 1;
        Ok(())
    }

    fn start_of(&self, frame: usize) -> u64 {
        frame as u64 * Self::frame_bytes(self.pixel_count)
    }
}

impl FrameSource for Spool {
    fn frame_count(&self) -> usize {
        self.frames
    }

    fn read_frame(&mut self, index: usize, pixels: &mut Vec<Rgb>) -> Result<(), FrameError> {
        if index >= self.frames {
            return Err(FrameError::Unreadable);
        }
        pixels.resize(self.pixel_count, [0; 3]);
        let whole_frame = self
            .file
            .seek(SeekFrom::Start(self.start_of(index)))
            .and_then(|_| self.file.read_exact(pixels.as_flattened_mut()));
        whole_frame.map_err(|_| FrameError::Unreadable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gif_core::GifFps;
    use gif_core::encode::{ManualSettings, encode_frames, encode_gif};
    use std::fs;
    use std::path::PathBuf;

    const PIXELS: usize = 8 * 6;

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

    fn files_in(folder: &Path) -> usize {
        fs::read_dir(folder).unwrap().count()
    }

    #[test]
    fn frames_come_back_as_they_were_pushed_in_any_order_and_more_than_once() {
        let mut spool = Spool::create_in(&empty_folder("back"), PIXELS).unwrap();
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
        let mut spool = Spool::create_in(&empty_folder("after-reading"), PIXELS).unwrap();
        spool.push(&frame(0)).unwrap();
        spool.push(&frame(1)).unwrap();
        read(&mut spool, 0).unwrap();

        spool.push(&frame(2)).unwrap();

        assert_eq!((read(&mut spool, 1), read(&mut spool, 2)), (Ok(frame(1)), Ok(frame(2))));
    }

    #[test]
    fn a_frame_of_another_size_is_refused_and_not_counted() {
        let mut spool = Spool::create_in(&empty_folder("wrong-size"), PIXELS).unwrap();
        spool.push(&frame(0)).unwrap();

        let refused = spool.push(&frame(1)[..PIXELS - 1]);

        assert_eq!(refused.map_err(|error| error.kind()), Err(io::ErrorKind::InvalidInput));
        assert_eq!(spool.frame_count(), 1);
    }

    #[test]
    fn a_frame_that_was_never_pushed_cannot_be_read() {
        let mut spool = Spool::create_in(&empty_folder("past-the-end"), PIXELS).unwrap();
        spool.push(&frame(0)).unwrap();
        assert_eq!(read(&mut spool, 1), Err(FrameError::Unreadable));
    }

    #[test]
    fn the_file_is_on_disk_while_the_spool_lives_and_gone_afterwards() {
        let folder = empty_folder("gone");
        let mut spool = Spool::create_in(&folder, PIXELS).unwrap();
        spool.push(&frame(0)).unwrap();
        assert_eq!(files_in(&folder), 1);

        drop(spool);

        assert_eq!(files_in(&folder), 0);
    }

    #[test]
    fn two_spools_in_one_folder_keep_their_own_frames() {
        let folder = empty_folder("two");
        let mut first = Spool::create_in(&folder, PIXELS).unwrap();
        let mut second = Spool::create_in(&folder, PIXELS).unwrap();
        first.push(&frame(1)).unwrap();
        second.push(&frame(2)).unwrap();

        assert_eq!((read(&mut first, 0), read(&mut second, 0)), (Ok(frame(1)), Ok(frame(2))));
    }

    #[test]
    fn a_folder_that_is_not_there_is_an_error() {
        let missing = empty_folder("missing").join("nowhere");
        assert_eq!(Spool::create_in(&missing, PIXELS).map_err(|error| error.kind()).err(), Some(io::ErrorKind::NotFound));
    }

    #[test]
    fn encoding_from_the_spool_gives_the_same_gif_as_encoding_from_memory() {
        let frames: Vec<Vec<Rgb>> = (0..5).map(frame).collect();
        let slices: Vec<&[Rgb]> = frames.iter().map(Vec::as_slice).collect();
        let settings = ManualSettings { colors: 16, dither: 1.0, tolerance: 8 };
        let mut spool = Spool::create_in(&empty_folder("same-gif"), PIXELS).unwrap();
        for pixels in &frames {
            spool.push(pixels).unwrap();
        }

        let from_disk = encode_frames(8, 6, &mut spool, GifFps::Fps10, settings, &mut |_| {});

        assert_eq!(from_disk, encode_gif(8, 6, &slices, GifFps::Fps10, settings));
    }
}
