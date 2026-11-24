
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

    let mut writer = GifWriter::new(width, height, palette, fps, 0)?;
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
    global_palette: Vec<Rgb>,
    local_palette: Option<Vec<Rgb>>,
    frames_written: usize,
    on_screen: Vec<u8>,
    asked: Vec<u8>,
    longer_runs: LongerRuns,
    loss: u8,
    distances: Option<ColorDistances>,
}

impl GifWriter {
    pub fn new(width: u16, height: u16, palette: &[Rgb], fps: GifFps, loss: u8) -> Result<Self, GifWriteError> {
        let pixel_count = usize::from(width) * usize::from(height);
        if pixel_count == 0 {
            return Err(GifWriteError::EmptyImage);
        }
        check_palette(palette)?;

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
            global_palette: palette.to_vec(),
            local_palette: None,
            frames_written: 0,
            on_screen: Vec::new(),
            asked: Vec::new(),
            longer_runs: LongerRuns::new(),
            loss,
            distances: (loss > 0).then(|| ColorDistances::of(palette)),
        })
    }

    pub fn use_palette(&mut self, palette: &[Rgb]) -> Result<(), GifWriteError> {
        check_palette(palette)?;
        if palette == self.palette() {
            return Ok(());
        }
        self.local_palette = (palette != self.global_palette).then(|| palette.to_vec());
        self.on_screen.clear();
        if self.loss > 0 {
            self.distances = Some(ColorDistances::of(palette));
        }
        Ok(())
    }

    fn palette(&self) -> &[Rgb] {
        self.local_palette.as_deref().unwrap_or(&self.global_palette)
    }

    pub fn add_frame(&mut self, indices: &[u8], slots: std::ops::Range<usize>) -> Result<(), GifWriteError> {
        let frame = self.frames_written;
        let palette_len = self.palette().len();
        check_frame(frame, indices, self.pixel_count, palette_len)?;

        let transparent = palette_len as u8;
        let table_bits = color_table_bits(palette_len + 1);
        let min_code_size = table_bits.max(2);
        let mut patch = if self.on_screen.is_empty() {
            FramePatch::whole_frame(indices, self.row_width)
        } else {
            let pixels = indices.iter().zip(&self.asked).zip(&self.on_screen);
            let keep_unchanged = |((&asked, &asked_before), &shown)| if asked == asked_before { shown } else { asked };
            let unchanged_ones_kept: Vec<u8> = pixels.map(keep_unchanged).collect();
            changed_patch(&self.on_screen, &unchanged_ones_kept, self.row_width, transparent)
        };
        write_frame_control(&mut self.out, slots_delay(self.fps, slots), transparent);
        let local_table_bits = self.local_palette.is_some().then_some(table_bits);
        write_image_position(&mut self.out, &patch, local_table_bits);
        if let Some(local_palette) = &self.local_palette {
            write_color_table(&mut self.out, local_palette, table_bits);
        }
        self.out.push(min_code_size);
        let stand_ins = self.distances.as_ref().map(|distances| StandIns {
            distances,
            farthest: u16::from(self.loss).pow(2),
            wanted: patch.under(indices, self.row_width),
            beneath: if self.on_screen.is_empty() { Vec::new() } else { patch.under(&self.on_screen, self.row_width) },
            transparent,
        });
        let codes = lzw(&mut patch.pixels, min_code_size, &mut self.longer_runs, stand_ins.as_ref());
        write_sub_blocks(&mut self.out, &codes);

        if self.on_screen.is_empty() {
            self.on_screen = patch.pixels;
        } else {
            patch.paint_over(&mut self.on_screen, self.row_width, transparent);
        }
        self.asked.clear();
        self.asked.extend_from_slice(indices);
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
    check_palette(palette)?;
    for (frame, pixels) in frames.iter().enumerate() {
        check_frame(frame, pixels, pixel_count, palette.len())?;
    }
    Ok(())
}

fn check_palette(palette: &[Rgb]) -> Result<(), GifWriteError> {
    if palette.is_empty() || palette.len() > MAX_PALETTE_COLORS {
        return Err(GifWriteError::PaletteSize(palette.len()));
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

fn write_image_position(out: &mut Vec<u8>, patch: &FramePatch, local_table_bits: Option<u8>) {
    let has_local_table = 0b1000_0000;
    out.push(IMAGE);
    for value in [patch.left, patch.top, patch.width, patch.height] {
        out.extend_from_slice(&(value as u16).to_le_bytes());
    }
    out.push(local_table_bits.map_or(0, |bits| has_local_table | (bits - 1)));
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

#[derive(Debug, Clone)]
struct LongerRuns {
    codes: Vec<u16>,
    filled: Vec<usize>,
    first_longer: Vec<u16>,
    next_longer: Vec<u16>,
    last_pixel: Vec<u8>,
}

impl LongerRuns {
    const NONE: u16 = 0;

    fn new() -> Self {
        let every_run_and_pixel = usize::from(CODE_TABLE_FULL) << 8;
        let every_code = usize::from(CODE_TABLE_FULL);
        Self {
            codes: vec![Self::NONE; every_run_and_pixel],
            filled: Vec::with_capacity(every_code),
            first_longer: vec![Self::NONE; every_code],
            next_longer: vec![Self::NONE; every_code],
            last_pixel: vec![0; every_code],
        }
    }

    fn slot(run: u16, pixel: u8) -> usize {
        usize::from(run) << 8 | usize::from(pixel)
    }

    fn get(&self, run: u16, pixel: u8) -> Option<u16> {
        let code = self.codes[Self::slot(run, pixel)];
        (code != Self::NONE).then_some(code)
    }

    fn insert(&mut self, run: u16, pixel: u8, code: u16) {
        let slot = Self::slot(run, pixel);
        self.codes[slot] = code;
        self.filled.push(slot);
        self.next_longer[usize::from(code)] = self.first_longer[usize::from(run)];
        self.first_longer[usize::from(run)] = code;
        self.last_pixel[usize::from(code)] = pixel;
    }

    fn nearest(&self, run: u16, position: usize, stand_ins: &StandIns) -> Option<(u16, u8)> {
        let distance_to = stand_ins.distances.from(stand_ins.wanted[position]);
        let mut nearest = None;
        let mut nearest_distance = stand_ins.farthest;
        let mut code = self.first_longer[usize::from(run)];
        while code != Self::NONE {
            let candidate = self.last_pixel[usize::from(code)];
            let distance = distance_to[usize::from(stand_ins.shown(candidate, position))];
            if distance <= nearest_distance {
                (nearest, nearest_distance) = (Some((code, candidate)), distance);
            }
            code = self.next_longer[usize::from(code)];
        }
        nearest
    }

    fn clear(&mut self) {
        for &slot in &self.filled {
            self.codes[slot] = Self::NONE;
            self.first_longer[slot >> 8] = Self::NONE;
        }
        self.filled.clear();
    }
}

#[derive(Debug, Clone)]
struct ColorDistances(Vec<[u16; 256]>);

impl ColorDistances {
    const UNREACHABLE: u16 = u16::MAX;

    fn of(palette: &[Rgb]) -> Self {
        let mut distances = vec![[Self::UNREACHABLE; 256]; 256];
        for (from, a) in palette.iter().enumerate() {
            for (to, b) in palette.iter().enumerate() {
                let squared: u32 = a.iter().zip(b).map(|(&a, &b)| u32::from(a.abs_diff(b)).pow(2)).sum();
                distances[from][to] = squared.min(u32::from(Self::UNREACHABLE)) as u16;
            }
        }
        Self(distances)
    }

    fn from(&self, pixel: u8) -> &[u16; 256] {
        &self.0[usize::from(pixel)]
    }
}

#[derive(Debug, Clone)]
struct StandIns<'a> {
    distances: &'a ColorDistances,
    farthest: u16,
    wanted: Vec<u8>,
    beneath: Vec<u8>,
    transparent: u8,
}

impl StandIns<'_> {
    fn shown(&self, pixel: u8, position: usize) -> u8 {
        if pixel == self.transparent { self.beneath[position] } else { pixel }
    }
}

fn lzw(pixels: &mut [u8], min_code_size: u8, longer_runs: &mut LongerRuns, stand_ins: Option<&StandIns>) -> Vec<u8> {
    let clear = 1u16 << min_code_size;
    let end = clear + 1;
    let first_free_code = clear + 2;
    let starting_code_bits = min_code_size + 1;

    longer_runs.clear();
    let mut next_code = first_free_code;
    let mut code_bits = starting_code_bits;
    let mut bits = BitWriter::default();
    bits.push(clear, code_bits);

    let mut run = u16::from(pixels[0]);
    for (position, shown) in pixels.iter_mut().enumerate().skip(1) {
        let pixel = *shown;
        if let Some(longer) = longer_runs.get(run, pixel) {
            run = longer;
            continue;
        }
        let nearest = stand_ins.and_then(|stand_ins| longer_runs.nearest(run, position, stand_ins));
        if let Some((longer, stand_in)) = nearest {
            *shown = stand_in;
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
            longer_runs.insert(run, pixel, next_code);
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

    fn decoded_colors(bytes: &[u8]) -> Vec<(Vec<Rgb>, bool)> {
        let mut options = gif::DecodeOptions::new();
        options.set_color_output(gif::ColorOutput::Indexed);
        let mut decoder = options.read_info(bytes).expect("a readable GIF header");
        let global = decoder.global_palette().expect("a global palette").to_vec();
        let width = usize::from(decoder.width());
        let mut canvas = vec![BLACK; width * usize::from(decoder.height())];
        let mut frames = Vec::new();
        while let Some(frame) = decoder.read_next_frame().expect("a readable frame") {
            let table = frame.palette.as_deref().unwrap_or(&global);
            let (left, top) = (usize::from(frame.left), usize::from(frame.top));
            let patch_width = usize::from(frame.width);
            for (position, &index) in frame.buffer.iter().enumerate() {
                if Some(index) != frame.transparent {
                    let (x, y) = (left + position % patch_width, top + position / patch_width);
                    let color = &table[usize::from(index) * 3..][..3];
                    canvas[y * width + x] = [color[0], color[1], color[2]];
                }
            }
            frames.push((canvas.clone(), frame.palette.is_some()));
        }
        frames
    }

    fn gif_with_palettes(scenes: &[(&[Rgb], &[&[u8]])]) -> Vec<u8> {
        gif_with_loss(3, scenes, 0)
    }

    fn gif_with_loss(width: u16, scenes: &[(&[Rgb], &[&[u8]])], loss: u8) -> Vec<u8> {
        let height = (scenes[0].1[0].len() / usize::from(width)) as u16;
        let mut writer = GifWriter::new(width, height, scenes[0].0, GifFps::Fps10, loss).unwrap();
        for (palette, frames) in scenes {
            writer.use_palette(palette).unwrap();
            for frame in *frames {
                writer.add_frame(frame, 0..1).unwrap();
            }
        }
        writer.finish().unwrap()
    }

    #[test]
    fn frames_after_a_palette_switch_show_the_colors_of_the_new_palette() {
        let blue = [0, 0, 255];
        let first: &[Rgb] = &[BLACK, RED];
        let second: &[Rgb] = &[GREEN, blue, RED];
        let bytes = gif_with_palettes(&[
            (first, &[&[0, 1, 0], &[0, 1, 1]]),
            (second, &[&[0, 1, 2], &[0, 0, 2]]),
            (first, &[&[1, 1, 0]]),
        ]);

        let shown_and_whether_it_carries_its_own_table = vec![
            (vec![BLACK, RED, BLACK], false),
            (vec![BLACK, RED, RED], false),
            (vec![GREEN, blue, RED], true),
            (vec![GREEN, GREEN, RED], true),
            (vec![RED, RED, BLACK], false),
        ];
        assert_eq!(decoded_colors(&bytes), shown_and_whether_it_carries_its_own_table);
    }

    #[test]
    fn switching_to_the_palette_already_in_use_changes_nothing() {
        let palette: &[Rgb] = &[BLACK, RED];
        let frames: &[&[u8]] = &[&[0, 1, 0], &[0, 1, 1], &[1, 1, 1]];
        let switched = gif_with_palettes(&[(palette, &frames[..1]), (palette, &frames[1..])]);
        assert_eq!(switched, write_gif(3, 1, palette, frames, GifFps::Fps10).unwrap());
    }

    const CLIP_WIDTH: usize = 64;
    const GOLDEN_LOSSLESS: (usize, u64) = (10427, 0x8137_e1c0_0c13_d6ca);

    fn grays() -> Vec<Rgb> {
        (0..255).map(|value| [value, value, value]).collect()
    }

    fn shuffled_grays() -> Vec<Rgb> {
        (0..255).map(|index| [(index * 97 % 255) as u8; 3]).collect()
    }

    fn grainy_clip_with_a_moving_square() -> Vec<Vec<u8>> {
        let mut seed = 0x1234_5678u32;
        let mut grain = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (seed % 9) as usize
        };
        let background: Vec<u8> =
            (0..CLIP_WIDTH * 48).map(|index| ((index % CLIP_WIDTH) * 2 + index / CLIP_WIDTH + grain()) as u8).collect();
        (0..6)
            .map(|number| {
                let mut frame = background.clone();
                for y in 4 + number * 4..16 + number * 4 {
                    for x in 4 + number * 6..16 + number * 6 {
                        frame[y * CLIP_WIDTH + x] = (200 - x - y + grain()) as u8;
                    }
                }
                frame
            })
            .collect()
    }

    fn two_scenes_of(clip: &[Vec<u8>]) -> (Vec<Rgb>, Vec<Rgb>, Vec<&[u8]>) {
        (grays(), shuffled_grays(), clip.iter().map(Vec::as_slice).collect())
    }

    fn fnv1a(bytes: &[u8]) -> u64 {
        bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3))
    }

    #[test]
    fn without_a_loss_allowance_the_file_is_byte_for_byte_what_the_lossless_writer_made() {
        let clip = grainy_clip_with_a_moving_square();
        let (first, second, frames) = two_scenes_of(&clip);

        let bytes = gif_with_loss(CLIP_WIDTH as u16, &[(&first, &frames[..3]), (&second, &frames[3..])], 0);

        assert_eq!((bytes.len(), fnv1a(&bytes)), GOLDEN_LOSSLESS);
    }

    fn farthest_shown_from_asked(bytes: &[u8], asked: &[Vec<Rgb>]) -> (u32, usize) {
        let shown = decoded_colors(bytes);
        assert_eq!(shown.len(), asked.len());
        let pairs = shown.iter().zip(asked).flat_map(|((shown, _), asked)| shown.iter().zip(asked));
        let distances: Vec<u32> = pairs
            .map(|(shown, asked)| shown.iter().zip(asked).map(|(&a, &b)| u32::from(a.abs_diff(b)).pow(2)).sum())
            .collect();
        (distances.iter().copied().max().unwrap(), distances.iter().filter(|&&distance| distance > 0).count())
    }

    #[test]
    fn with_a_loss_allowance_every_shown_pixel_is_within_the_allowance_of_the_color_asked_for() {
        let clip = grainy_clip_with_a_moving_square();
        let (first, second, frames) = two_scenes_of(&clip);
        let asked: Vec<Vec<Rgb>> = clip
            .iter()
            .enumerate()
            .map(|(number, frame)| {
                let palette = if number < 3 { &first } else { &second };
                frame.iter().map(|&index| palette[usize::from(index)]).collect()
            })
            .collect();

        for loss in [1u8, 7, 20, 64] {
            let bytes = gif_with_loss(CLIP_WIDTH as u16, &[(&first, &frames[..3]), (&second, &frames[3..])], loss);

            let (farthest, moved) = farthest_shown_from_asked(&bytes, &asked);
            assert!(farthest <= u32::from(loss).pow(2), "loss {loss}: a pixel moved {farthest} squared");
            if loss >= 7 {
                assert!(moved > 0, "loss {loss} moved no pixel");
            }
        }
    }

    #[test]
    fn a_larger_loss_allowance_gives_a_smaller_file() {
        let clip = grainy_clip_with_a_moving_square();
        let frames: Vec<&[u8]> = clip.iter().map(Vec::as_slice).collect();

        let size_at = |loss| gif_with_loss(CLIP_WIDTH as u16, &[(&grays(), &frames)], loss).len();
        let sizes: Vec<usize> = [0, 4, 8, 16, 32].into_iter().map(size_at).collect();

        assert!(sizes.windows(2).all(|pair| pair[0] > pair[1]), "{sizes:?}");
    }

    #[test]
    fn a_still_after_a_lossy_frame_keeps_its_picture_and_later_frames_stay_within_the_allowance() {
        let clip = grainy_clip_with_a_moving_square();
        let still = vec![clip[0].clone(); 3];
        let frames: Vec<&[u8]> = still.iter().chain(&clip[1..]).map(Vec::as_slice).collect();
        let palette = grays();
        let loss = 12;

        let bytes = gif_with_loss(CLIP_WIDTH as u16, &[(&palette, &frames)], loss);

        let asked: Vec<Vec<Rgb>> =
            frames.iter().map(|frame| frame.iter().map(|&index| palette[usize::from(index)]).collect()).collect();
        let first_frame_alone = gif_with_loss(CLIP_WIDTH as u16, &[(&palette, &frames[..1])], loss);
        assert!(farthest_shown_from_asked(&first_frame_alone, &asked[..1]).1 > 0, "the first frame is lossy");
        let shown = decoded_colors(&bytes);
        assert!(shown[1..3].iter().all(|(picture, _)| *picture == shown[0].0), "the still shows the first picture");
        assert!(farthest_shown_from_asked(&bytes, &asked).0 <= u32::from(loss).pow(2));
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
