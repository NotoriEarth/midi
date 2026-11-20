use crate::{GifFps, frame_delays};

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

    let table_bits = color_table_bits(palette.len());
    let min_code_size = table_bits.max(2);
    let delays = frame_delays(fps, frames.len());

    let mut out = Vec::new();
    write_header(&mut out, width, height, table_bits);
    write_color_table(&mut out, palette, table_bits);
    write_loop_forever(&mut out);
    for (pixels, delay) in frames.iter().zip(delays) {
        write_frame_delay(&mut out, delay);
        write_image_position(&mut out, width, height);
        out.push(min_code_size);
        write_sub_blocks(&mut out, &uncompressed_lzw(pixels, min_code_size));
    }
    out.push(TRAILER);
    Ok(out)
}

const EXTENSION: u8 = 0x21;
const GRAPHIC_CONTROL: u8 = 0xF9;
const APPLICATION: u8 = 0xFF;
const IMAGE: u8 = 0x2C;
const TRAILER: u8 = 0x3B;
const BLOCK_END: u8 = 0;

fn check_input(pixel_count: usize, palette: &[Rgb], frames: &[&[u8]]) -> Result<(), GifWriteError> {
    if pixel_count == 0 {
        return Err(GifWriteError::EmptyImage);
    }
    if frames.is_empty() {
        return Err(GifWriteError::NoFrames);
    }
    if palette.is_empty() || palette.len() > 256 {
        return Err(GifWriteError::PaletteSize(palette.len()));
    }
    for (frame, pixels) in frames.iter().enumerate() {
        if pixels.len() != pixel_count {
            return Err(GifWriteError::FrameSize {
                frame,
                expected: pixel_count,
                actual: pixels.len(),
            });
        }
        if let Some(&index) = pixels.iter().find(|&&index| usize::from(index) >= palette.len()) {
            return Err(GifWriteError::ColorOutOfPalette { frame, index });
        }
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

fn write_frame_delay(out: &mut Vec<u8>, delay: u16) {
    let no_disposal_no_transparency = 0;
    let transparent_color = 0;
    out.extend_from_slice(&[EXTENSION, GRAPHIC_CONTROL, 4, no_disposal_no_transparency]);
    out.extend_from_slice(&delay.to_le_bytes());
    out.extend_from_slice(&[transparent_color, BLOCK_END]);
}

fn write_image_position(out: &mut Vec<u8>, width: u16, height: u16) {
    let no_local_table = 0;
    out.push(IMAGE);
    for value in [0, 0, width, height] {
        out.extend_from_slice(&value.to_le_bytes());
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

fn uncompressed_lzw(pixels: &[u8], min_code_size: u8) -> Vec<u8> {
    let clear = 1u16 << min_code_size;
    let end = clear + 1;
    let code_bits = min_code_size + 1;
    let pixels_before_codes_would_grow = usize::from(clear) - 2;

    let mut bits = BitWriter::default();
    for run in pixels.chunks(pixels_before_codes_would_grow) {
        bits.push(clear, code_bits);
        for &pixel in run {
            bits.push(u16::from(pixel), code_bits);
        }
    }
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
        let mut frames = Vec::new();
        while let Some(frame) = decoder.read_next_frame().expect("a readable frame") {
            frames.push((frame.delay, frame.buffer.to_vec()));
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
    fn a_full_256_color_image_survives_the_round_trip() {
        let palette: Vec<Rgb> = (0..=255).map(|value| [value, value, value]).collect();
        let pixels: Vec<u8> = (0..64 * 48).map(|index| (index % 256) as u8).collect();
        let bytes = write_gif(64, 48, &palette, &[&pixels], GifFps::Fps15).unwrap();
        assert_eq!(decode(&bytes).frames[0].1, pixels);
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
            write_gif(2, 1, &[BLACK; 257], &[ok_frame], fps),
            Err(GifWriteError::PaletteSize(257))
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
