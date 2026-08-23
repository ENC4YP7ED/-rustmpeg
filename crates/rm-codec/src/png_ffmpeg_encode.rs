use rm_compress::{crc32, zlib_level};
use rm_core::video::{PixelFormat, VideoFrame};
use rm_core::{MediaError, Result};

use crate::png_options::{PngEncodeOptions, PngPrediction};

const PNG_SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];
const IDAT_CHUNK_SIZE: usize = 65_536;
const PASSES: [(usize, usize, usize, usize); 7] = [
    (0, 0, 8, 8),
    (4, 0, 8, 8),
    (0, 4, 4, 8),
    (2, 0, 4, 4),
    (0, 2, 2, 4),
    (1, 0, 2, 2),
    (0, 1, 1, 2),
];

/// Encodes PNG with FFmpeg 9.0.1-compatible default private options.
pub fn encode_png(frame: &VideoFrame) -> Result<Vec<u8>> {
    encode_png_with_options(frame, PngEncodeOptions::default())
}

/// Encodes an 8-bit packed frame with explicit PNG encoder options.
///
/// # Errors
/// Returns an error for invalid options/frame storage, unsupported formats,
/// arithmetic overflow, or compression failure.
pub fn encode_png_with_options(frame: &VideoFrame, options: PngEncodeOptions) -> Result<Vec<u8>> {
    let options = options.validate()?;
    let color_type = match frame.format {
        PixelFormat::Gray8 => 0_u8,
        PixelFormat::Rgb24 => 2_u8,
        PixelFormat::GrayAlpha8 => 4_u8,
        PixelFormat::Rgba32 => 6_u8,
    };
    let filtered = if options.interlaced {
        encode_adam7_scanlines(frame, options.prediction)?
    } else {
        encode_regular_scanlines(frame, options.prediction)?
    };
    let compressed = zlib_level::compress_with_level(&filtered, options.compression_level)?;

    let mut output = PNG_SIGNATURE.to_vec();
    let mut ihdr = [0_u8; 13];
    ihdr[0..4].copy_from_slice(&frame.width.to_be_bytes());
    ihdr[4..8].copy_from_slice(&frame.height.to_be_bytes());
    ihdr[8] = 8;
    ihdr[9] = color_type;
    ihdr[12] = u8::from(options.interlaced);
    write_chunk(&mut output, *b"IHDR", &ihdr)?;
    write_phys(&mut output, options)?;
    for chunk in compressed.chunks(IDAT_CHUNK_SIZE) {
        write_chunk(&mut output, *b"IDAT", chunk)?;
    }
    write_chunk(&mut output, *b"IEND", &[])?;
    Ok(output)
}

fn write_phys(output: &mut Vec<u8>, options: PngEncodeOptions) -> Result<()> {
    let mut phys = [0_u8; 9];
    if let Some(dpm) = options.dots_per_meter()? {
        phys[0..4].copy_from_slice(&dpm.to_be_bytes());
        phys[4..8].copy_from_slice(&dpm.to_be_bytes());
        phys[8] = 1;
    } else {
        phys[0..4].copy_from_slice(&options.sample_aspect_ratio.0.to_be_bytes());
        phys[4..8].copy_from_slice(&options.sample_aspect_ratio.1.to_be_bytes());
        phys[8] = 0;
    }
    write_chunk(output, *b"pHYs", &phys)
}

fn encode_regular_scanlines(frame: &VideoFrame, prediction: PngPrediction) -> Result<Vec<u8>> {
    let stride = frame.format.packed_row_bytes(frame.width)
        .ok_or_else(|| MediaError::overflow("PNG row stride overflow"))?;
    let height = usize::try_from(frame.height)
        .map_err(|_| MediaError::overflow("PNG height exceeds usize"))?;
    let capacity = stride.checked_add(1).and_then(|row| row.checked_mul(height))
        .ok_or_else(|| MediaError::overflow("PNG scanline size overflow"))?;
    let mut output = Vec::with_capacity(capacity);
    let mut previous = vec![0_u8; stride];
    for y in 0..frame.height {
        let raw = frame.row(y)?;
        append_filtered(&mut output, raw, &previous, frame.format.bytes_per_pixel(), prediction);
        previous.copy_from_slice(raw);
    }
    Ok(output)
}

fn encode_adam7_scanlines(frame: &VideoFrame, prediction: PngPrediction) -> Result<Vec<u8>> {
    let width = usize::try_from(frame.width)
        .map_err(|_| MediaError::overflow("PNG width exceeds usize"))?;
    let height = usize::try_from(frame.height)
        .map_err(|_| MediaError::overflow("PNG height exceeds usize"))?;
    let bpp = frame.format.bytes_per_pixel();
    let mut output = Vec::new();

    for &(x0, y0, dx, dy) in &PASSES {
        let pass_width = extent(width, x0, dx);
        let pass_height = extent(height, y0, dy);
        if pass_width == 0 || pass_height == 0 {
            continue;
        }
        let row_len = pass_width.checked_mul(bpp)
            .ok_or_else(|| MediaError::overflow("PNG Adam7 row size overflow"))?;
        let mut previous = vec![0_u8; row_len];
        let mut row = Vec::with_capacity(row_len);
        for py in 0..pass_height {
            row.clear();
            let y = y0 + py * dy;
            let source = frame.row(u32::try_from(y).map_err(|_| MediaError::overflow("PNG row index exceeds u32"))?)?;
            for px in 0..pass_width {
                let x = x0 + px * dx;
                let start = x.checked_mul(bpp)
                    .ok_or_else(|| MediaError::overflow("PNG pixel offset overflow"))?;
                row.extend_from_slice(source.get(start..start + bpp)
                    .ok_or_else(|| MediaError::invalid_data("PNG frame row is truncated"))?);
            }
            append_filtered(&mut output, &row, &previous, bpp, prediction);
            previous.copy_from_slice(&row);
        }
    }
    Ok(output)
}

fn append_filtered(output: &mut Vec<u8>, raw: &[u8], previous: &[u8], bpp: usize, prediction: PngPrediction) {
    let (filter, encoded) = match prediction.filter_type() {
        Some(filter) => (filter, filter_row(filter, raw, previous, bpp)),
        None => choose_mixed_filter(raw, previous, bpp),
    };
    output.push(filter);
    output.extend_from_slice(&encoded);
}

fn choose_mixed_filter(raw: &[u8], previous: &[u8], bpp: usize) -> (u8, Vec<u8>) {
    let mut best_filter = 0_u8;
    let mut best = filter_row(0, raw, previous, bpp);
    let mut score = filter_score(&best);
    for filter in 1..=4 {
        let candidate = filter_row(filter, raw, previous, bpp);
        let candidate_score = filter_score(&candidate);
        if candidate_score < score {
            score = candidate_score;
            best_filter = filter;
            best = candidate;
        }
    }
    (best_filter, best)
}

fn filter_row(filter: u8, raw: &[u8], previous: &[u8], bpp: usize) -> Vec<u8> {
    raw.iter().enumerate().map(|(index, &value)| {
        let left = if index >= bpp { raw[index - bpp] } else { 0 };
        let up = previous.get(index).copied().unwrap_or(0);
        let up_left = if index >= bpp { previous.get(index - bpp).copied().unwrap_or(0) } else { 0 };
        let predictor = match filter {
            0 => 0,
            1 => left,
            2 => up,
            3 => u8::try_from(u16::midpoint(u16::from(left), u16::from(up))).expect("average fits u8"),
            4 => paeth(left, up, up_left),
            _ => unreachable!(),
        };
        value.wrapping_sub(predictor)
    }).collect()
}

fn filter_score(bytes: &[u8]) -> u64 {
    bytes.iter().map(|&byte| {
        let value = u16::from(byte);
        u64::from(value.min(256 - value))
    }).sum()
}

fn paeth(left: u8, up: u8, up_left: u8) -> u8 {
    let a = i32::from(left);
    let b = i32::from(up);
    let c = i32::from(up_left);
    let p = a + b - c;
    let pa = (p - a).abs();
    let pb = (p - b).abs();
    let pc = (p - c).abs();
    if pa <= pb && pa <= pc { left } else if pb <= pc { up } else { up_left }
}

fn extent(total: usize, start: usize, step: usize) -> usize {
    if total <= start { 0 } else { (total - start).div_ceil(step) }
}

fn write_chunk(output: &mut Vec<u8>, kind: [u8; 4], data: &[u8]) -> Result<()> {
    let length = u32::try_from(data.len())
        .map_err(|_| MediaError::overflow("PNG chunk exceeds u32"))?;
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(&kind);
    output.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(&kind);
    crc_input.extend_from_slice(data);
    output.extend_from_slice(&crc32(&crc_input).to_be_bytes());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame() -> VideoFrame {
        let mut data = Vec::new();
        for y in 0..9_u8 {
            for x in 0..11_u8 {
                data.extend_from_slice(&[x.wrapping_mul(17), y.wrapping_mul(29), x ^ y, 255_u8.wrapping_sub(x.wrapping_mul(7))]);
            }
        }
        VideoFrame::from_vec(11, 9, PixelFormat::Rgba32, data).unwrap()
    }

    #[test]
    fn default_is_paeth_and_round_trips() {
        let source = frame();
        let encoded = encode_png(&source).unwrap();
        let decoded = crate::png::decode_png(&encoded).unwrap();
        assert_eq!(decoded, source);
    }

    #[test]
    fn every_prediction_mode_round_trips() {
        let source = frame();
        for prediction in [PngPrediction::None, PngPrediction::Sub, PngPrediction::Up, PngPrediction::Average, PngPrediction::Paeth, PngPrediction::Mixed] {
            let encoded = encode_png_with_options(&source, PngEncodeOptions { prediction, ..PngEncodeOptions::default() }).unwrap();
            assert_eq!(crate::png::decode_png(&encoded).unwrap(), source);
        }
    }

    #[test]
    fn adam7_encode_round_trips_through_independent_decoder_path() {
        let source = frame();
        let encoded = encode_png_with_options(&source, PngEncodeOptions { interlaced: true, ..PngEncodeOptions::default() }).unwrap();
        assert_eq!(encoded[28], 1);
        assert_eq!(crate::png::decode_png(&encoded).unwrap(), source);
    }

    #[test]
    fn phys_dpi_and_default_sar_follow_ffmpeg_shape() {
        let source = frame();
        let encoded = encode_png_with_options(&source, PngEncodeOptions { dpi: Some(300), ..PngEncodeOptions::default() }).unwrap();
        let phys = find_chunk(&encoded, b"pHYs").unwrap();
        assert_eq!(u32::from_be_bytes(phys[0..4].try_into().unwrap()), 11_811);
        assert_eq!(u32::from_be_bytes(phys[4..8].try_into().unwrap()), 11_811);
        assert_eq!(phys[8], 1);

        let default = encode_png(&source).unwrap();
        let phys = find_chunk(&default, b"pHYs").unwrap();
        assert_eq!(&phys[..4], &[0,0,0,0]);
        assert_eq!(&phys[4..8], &[0,0,0,1]);
        assert_eq!(phys[8], 0);
    }

    fn find_chunk<'a>(png: &'a [u8], wanted: &[u8;4]) -> Option<&'a [u8]> {
        let mut offset = 8_usize;
        while offset + 12 <= png.len() {
            let len = usize::try_from(u32::from_be_bytes(png.get(offset..offset+4)?.try_into().ok()?)).ok()?;
            let kind: &[u8;4] = png.get(offset+4..offset+8)?.try_into().ok()?;
            let start = offset + 8;
            let end = start.checked_add(len)?;
            let data = png.get(start..end)?;
            if kind == wanted { return Some(data); }
            offset = end.checked_add(4)?;
        }
        None
    }
}
