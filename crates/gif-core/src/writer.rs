use std::collections::HashMap;

use crate::diff::{FramePatch, changed_patch};
use crate::GifFps;
use crate::delay::slots_delay;

pub type Rgb = [u8; 3];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GifWriteError {
    EmptyImage,
    NoFrames,
    PaletteSize(usize),
    FrameSize { frame: usize, expected: usize, actual: usize },
    ColorOutOfPalette { frame: usize, index: u8 },
}

pub fn write_gif(
    width: u16,
    height: u16,
    palette: &[Rgb],
    frames: &[&[u8]],
    fps: GifFps,
) -> Result<Vec<u8>, GifWriteError> {
    let pixel_count = usize::from(width) * usize::from(height);
    check_input(pixel_count, palette, frames)?;

    let mut writer = GifWriter::new(width, height, palette, fps)?;
    for (index, frame) in frames.iter().enumerate() {
        writer.add_frame(frame, index..index + 1)?;
    }
    writer.finish()
}

#[derive(Debug, Clone)]
pub struct GifWriter {
    out: Vec<u8>,
    fps: GifFps,
    row_width: usize,
    pixel_count: usize,
    palette_len: usize,
    min_code_size: u8,
    frames_written: usize,
    last_frame: Vec<u8>,
}

impl GifWriter {
    pub fn new(width: u16, height: u16, palette: &[Rgb], fps: GifFps) -> Result<Self, GifWriteError> {
        let pixel_count = usize::from(width) * usize::from(height);
        if pixel_count == 0 {
            return Err(GifWriteError::EmptyImage);
        }
        if palette.is_empty() || palette.len() > MAX_PALETTE_COLORS {
            return Err(GifWriteError::PaletteSize(palette.len()));
        }

        let table_bits = color_table_bits(palette.len() + 1);
        let mut out = Vec::new();
        write_header(&mut out, width, height, table_bits);
        write_color_table(&mut out, palette, table_bits);
        write_loop_forever(&mut out);
        Ok(Self {
            out,
            fps,
            row_width: usize::from(width),
            pixel_count,
            palette_len: palette.len(),
            min_code_size: table_bits.max(2),
            frames_written: 0,
            last_frame: Vec::new(),
        })
    }

    pub fn add_frame(&mut self, indices: &[u8], slots: std::ops::Range<usize>) -> Result<(), GifWriteError> {
        let frame = self.frames_written;
        check_frame(frame, indices, self.pixel_count, self.palette_len)?;

        let transparent = self.palette_len as u8;
        let patch = if frame == 0 {
            FramePatch::whole_frame(indices, self.row_width)
        } else {
            changed_patch(&self.last_frame, indices, self.row_width, transparent)
        };
        write_frame_control(&mut self.out, slots_delay(self.fps, slots), transparent);
        write_image_position(&mut self.out, &patch);
        self.out.push(self.min_code_size);
        write_sub_blocks(&mut self.out, &lzw(&patch.pixels, self.min_code_size));

        self.last_frame.clear();
        self.last_frame.extend_from_slice(indices);
        self.frames_written += 1;
        Ok(())
    }

    pub fn finish(mut self) -> Result<Vec<u8>, GifWriteError> {
        if self.frames_written == 0 {
            return Err(GifWriteError::NoFrames);
        }
        self.out.push(TRAILER);
        Ok(self.out)
    }
}

const EXTENSION: u8 = 0x21;
const GRAPHIC_CONTROL: u8 = 0xF9;
const APPLICATION: u8 = 0xFF;
const IMAGE: u8 = 0x2C;
const TRAILER: u8 = 0x3B;
const BLOCK_END: u8 = 0;
const MAX_PALETTE_COLORS: usize = 255;

fn check_input(pixel_count: usize, palette: &[Rgb], frames: &[&[u8]]) -> Result<(), GifWriteError> {
    if pixel_count == 0 {
        return Err(GifWriteError::EmptyImage);
    }
    if frames.is_empty() {
        return Err(GifWriteError::NoFrames);
    }
    if palette.is_empty() || palette.len() > MAX_PALETTE_COLORS {
        return Err(GifWriteError::PaletteSize(palette.len()));
    }
    for (frame, pixels) in frames.iter().enumerate() {
        check_frame(frame, pixels, pixel_count, palette.len())?;
    }
    Ok(())
}

fn check_frame(frame: usize, pixels: &[u8], pixel_count: usize, palette_len: usize) -> Result<(), GifWriteError> {
    if pixels.len() != pixel_count {
        return Err(GifWriteError::FrameSize { frame, expected: pixel_count, actual: pixels.len() });
    }
    if let Some(&index) = pixels.iter().find(|&&index| usize::from(index) >= palette_len) {
        return Err(GifWriteError::ColorOutOfPalette { frame, index });
    }
    Ok(())
}

fn color_table_bits(color_count: usize) -> u8 {
    (1..=8).find(|&bits| (1usize << bits) >= color_count).unwrap_or(8)
}

fn write_header(out: &mut Vec<u8>, width: u16, height: u16, table_bits: u8) {
    let has_global_table = 0b1000_0000;
    let color_resolution = (table_bits - 1) << 4;
    let table_size = table_bits - 1;
    let background_color = 0;
    let pixel_aspect_ratio = 0;

    out.extend_from_slice(b"GIF89a");
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(&[
        has_global_table | color_resolution | table_size,
        background_color,
        pixel_aspect_ratio,
    ]);
}

fn write_color_table(out: &mut Vec<u8>, palette: &[Rgb], table_bits: u8) {
    let table_len = 1usize << table_bits;
    let padding = std::iter::repeat_n([0, 0, 0], table_len - palette.len());
    for color in palette.iter().copied().chain(padding) {
        out.extend_from_slice(&color);
    }
}

fn write_loop_forever(out: &mut Vec<u8>) {
    let loop_count: u16 = 0;
    out.extend_from_slice(&[EXTENSION, APPLICATION, 11]);
    out.extend_from_slice(b"NETSCAPE2.0");
    out.extend_from_slice(&[3, 1]);
    out.extend_from_slice(&loop_count.to_le_bytes());
    out.push(BLOCK_END);
}

fn write_frame_control(out: &mut Vec<u8>, delay: u16, transparent: u8) {
    let keep_this_frame_under_the_next = 1 << 2;
    let has_transparent_color = 1;
    out.extend_from_slice(&[
        EXTENSION,
        GRAPHIC_CONTROL,
        4,
        keep_this_frame_under_the_next | has_transparent_color,
    ]);
    out.extend_from_slice(&delay.to_le_bytes());
    out.extend_from_slice(&[transparent, BLOCK_END]);
}

fn write_image_position(out: &mut Vec<u8>, patch: &FramePatch) {
    let no_local_table = 0;
    out.push(IMAGE);
    for value in [patch.left, patch.top, patch.width, patch.height] {
        out.extend_from_slice(&(value as u16).to_le_bytes());
    }
    out.push(no_local_table);
}

fn write_sub_blocks(out: &mut Vec<u8>, data: &[u8]) {
    for block in data.chunks(255) {
        out.push(block.len() as u8);
        out.extend_from_slice(block);
    }
    out.push(BLOCK_END);
}

const MAX_CODE_BITS: u8 = 12;
const CODE_TABLE_FULL: u16 = 1 << MAX_CODE_BITS;

fn lzw(pixels: &[u8], min_code_size: u8) -> Vec<u8> {
    let clear = 1u16 << min_code_size;
    let end = clear + 1;
    let first_free_code = clear + 2;
    let starting_code_bits = min_code_size + 1;

    let mut longer_runs: HashMap<(u16, u8), u16> = HashMap::new();
    let mut next_code = first_free_code;
    let mut code_bits = starting_code_bits;
    let mut bits = BitWriter::default();
    bits.push(clear, code_bits);

    let mut run = u16::from(pixels[0]);
    for &pixel in &pixels[1..] {
        if let Some(&longer) = longer_runs.get(&(run, pixel)) {
            run = longer;
            continue;
        }

        bits.push(run, code_bits);
        if next_code == CODE_TABLE_FULL {
            bits.push(clear, code_bits);
            longer_runs.clear();
            next_code = first_free_code;
            code_bits = starting_code_bits;
        } else {
            longer_runs.insert((run, pixel), next_code);
            if next_code == 1 << code_bits {
                code_bits += 1;
            }
            next_code += 1;
        }
        run = u16::from(pixel);
    }

    bits.push(run, code_bits);
    bits.push(end, code_bits);
    bits.finish()
}

#[derive(Default)]
struct BitWriter {
    bytes: Vec<u8>,
    pending: u32,
    pending_bits: u8,
}

impl BitWriter {
    fn push(&mut self, code: u16, code_bits: u8) {
        self.pending |= u32::from(code) << self.pending_bits;
        self.pending_bits += code_bits;
        while self.pending_bits >= 8 {
            self.bytes.push(self.pending as u8);
            self.pending >>= 8;
            self.pending_bits -= 8;
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.pending_bits > 0 {
            self.bytes.push(self.pending as u8);
        }
        self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLACK: Rgb = [0, 0, 0];
    const RED: Rgb = [255, 0, 0];
    const GREEN: Rgb = [0, 255, 0];

    struct Decoded {
        width: u16,
        height: u16,
        palette: Vec<u8>,
        frames: Vec<(u16, Vec<u8>)>,
    }

    fn decode(bytes: &[u8]) -> Decoded {
        let mut options = gif::DecodeOptions::new();
        options.set_color_output(gif::ColorOutput::Indexed);
        let mut decoder = options.read_info(bytes).expect("a readable GIF header");
        let (width, height) = (usize::from(decoder.width()), usize::from(decoder.height()));
        let mut canvas = vec![0u8; width * height];
        let mut frames = Vec::new();
        while let Some(frame) = decoder.read_next_frame().expect("a readable frame") {
            let (left, top) = (usize::from(frame.left), usize::from(frame.top));
            let patch_width = usize::from(frame.width);
            for (position, &index) in frame.buffer.iter().enumerate() {
                if Some(index) != frame.transparent {
                    let (x, y) = (left + position % patch_width, top + position / patch_width);
                    canvas[y * width + x] = index;
                }
            }
            frames.push((frame.delay, canvas.clone()));
        }
        Decoded {
            width: decoder.width(),
            height: decoder.height(),
            palette: decoder.global_palette().expect("a global palette").to_vec(),
            frames,
        }
    }

    #[test]
    fn an_independent_decoder_reads_back_the_same_pixels() {
        let first = [0, 1, 2, 2, 1, 0];
        let second = [2, 2, 2, 0, 0, 0];
        let bytes = write_gif(3, 2, &[BLACK, RED, GREEN], &[&first, &second], GifFps::Fps10).unwrap();

        let decoded = decode(&bytes);
        assert_eq!((decoded.width, decoded.height), (3, 2));
        assert_eq!(decoded.palette[..9], [0, 0, 0, 255, 0, 0, 0, 255, 0]);
        assert_eq!(decoded.frames, [(10, first.to_vec()), (10, second.to_vec())]);
    }

    #[test]
    fn frame_delays_follow_the_frame_rate() {
        let frame = [0u8; 4];
        let bytes = write_gif(2, 2, &[BLACK, RED], &[&frame[..]; 3], GifFps::Fps30).unwrap();
        let delays: Vec<u16> = decode(&bytes).frames.iter().map(|(delay, _)| *delay).collect();
        assert_eq!(delays, [3, 3, 4]);
    }

    #[test]
    fn a_full_255_color_image_survives_the_round_trip() {
        let palette: Vec<Rgb> = (0..255).map(|value| [value, value, value]).collect();
        let pixels: Vec<u8> = (0..64 * 48).map(|index| (index % 255) as u8).collect();
        let bytes = write_gif(64, 48, &palette, &[&pixels], GifFps::Fps15).unwrap();
        assert_eq!(decode(&bytes).frames[0].1, pixels);
    }

    #[test]
    fn a_flat_image_compresses_to_a_fraction_of_its_pixel_count() {
        let pixels = [1u8; 100 * 100];
        let bytes = write_gif(100, 100, &[BLACK, RED], &[&pixels], GifFps::Fps10).unwrap();
        assert!(bytes.len() < 300, "{} bytes for 10000 pixels", bytes.len());
        assert_eq!(decode(&bytes).frames[0].1, pixels);
    }

    #[test]
    fn noise_that_fills_the_code_table_many_times_survives_the_round_trip() {
        let palette: Vec<Rgb> = (0..255).map(|value| [value, 0, 255 - value]).collect();
        let mut seed = 0x2545_F491u32;
        let pixels: Vec<u8> = (0..300 * 200)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                (seed % 255) as u8
            })
            .collect();
        let bytes = write_gif(300, 200, &palette, &[&pixels], GifFps::Fps10).unwrap();
        assert_eq!(decode(&bytes).frames[0].1, pixels);
    }

    #[test]
    fn a_repeating_pattern_survives_the_round_trip_at_every_palette_size() {
        for color_count in [1usize, 2, 3, 4, 5, 15, 16, 127, 128, 255] {
            let palette = vec![BLACK; color_count];
            let pixels: Vec<u8> = (0..97 * 61).map(|index| ((index / 7) % color_count) as u8).collect();
            let bytes = write_gif(97, 61, &palette, &[&pixels], GifFps::Fps10).unwrap();
            assert_eq!(decode(&bytes).frames[0].1, pixels, "{color_count} colors");
        }
    }

    fn dot_crossing_a_busy_background() -> Vec<Vec<u8>> {
        let mut seed = 0x9E37_79B9u32;
        let background: Vec<u8> = (0..60 * 40)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                (seed % 200) as u8
            })
            .collect();
        (0..10)
            .map(|step| {
                let mut frame = background.clone();
                frame[20 * 60 + step * 5] = 200;
                frame
            })
            .collect()
    }

    fn yellows() -> Vec<Rgb> {
        (0..=200).map(|value| [value, value, 0]).collect()
    }

    #[test]
    fn a_small_change_on_a_busy_background_shows_the_right_picture() {
        let frames = dot_crossing_a_busy_background();
        let slices: Vec<&[u8]> = frames.iter().map(Vec::as_slice).collect();

        let bytes = write_gif(60, 40, &yellows(), &slices, GifFps::Fps10).unwrap();
        let shown: Vec<Vec<u8>> = decode(&bytes).frames.into_iter().map(|(_, pixels)| pixels).collect();
        assert_eq!(shown, frames);
    }

    #[test]
    fn frames_that_barely_change_cost_almost_nothing() {
        let frames = dot_crossing_a_busy_background();
        let slices: Vec<&[u8]> = frames.iter().map(Vec::as_slice).collect();

        let one_frame = write_gif(60, 40, &yellows(), &slices[..1], GifFps::Fps10).unwrap();
        let ten_frames = write_gif(60, 40, &yellows(), &slices, GifFps::Fps10).unwrap();
        assert!(
            ten_frames.len() < one_frame.len() + 400,
            "one frame {} bytes, ten frames {} bytes",
            one_frame.len(),
            ten_frames.len()
        );
    }

    #[test]
    fn identical_frames_still_show_for_their_full_time() {
        let frame = [0u8, 1, 1, 0];
        let bytes = write_gif(2, 2, &[BLACK, RED], &[&frame[..]; 4], GifFps::Fps15).unwrap();
        let shown = vec![(6, frame.to_vec()), (7, frame.to_vec()), (7, frame.to_vec()), (6, frame.to_vec())];
        assert_eq!(decode(&bytes).frames, shown);
    }

    #[test]
    fn the_file_is_a_looping_gif89a() {
        let bytes = write_gif(1, 1, &[BLACK], &[&[0]], GifFps::Fps10).unwrap();
        assert!(bytes.starts_with(b"GIF89a"));
        assert!(bytes.windows(11).any(|window| window == b"NETSCAPE2.0"));
        assert_eq!(bytes.last(), Some(&0x3B));
    }

    #[test]
    fn input_that_cannot_become_a_gif_is_rejected() {
        let ok_frame: &[u8] = &[0, 0];
        let fps = GifFps::Fps10;
        assert_eq!(write_gif(0, 2, &[BLACK], &[ok_frame], fps), Err(GifWriteError::EmptyImage));
        assert_eq!(write_gif(2, 1, &[BLACK], &[], fps), Err(GifWriteError::NoFrames));
        assert_eq!(write_gif(2, 1, &[], &[ok_frame], fps), Err(GifWriteError::PaletteSize(0)));
        assert_eq!(
            write_gif(2, 1, &[BLACK; 256], &[ok_frame], fps),
            Err(GifWriteError::PaletteSize(256))
        );
        assert_eq!(
            write_gif(2, 1, &[BLACK], &[ok_frame, &[0]], fps),
            Err(GifWriteError::FrameSize { frame: 1, expected: 2, actual: 1 })
        );
        assert_eq!(
            write_gif(2, 1, &[BLACK, RED], &[&[0, 2]], fps),
            Err(GifWriteError::ColorOutOfPalette { frame: 0, index: 2 })
        );
    }
}
