use rm_compress::{crc32, zlib_encode};
use rm_core::video::{PixelFormat, VideoFrame};
use rm_core::{MediaError, Result};

const PNG_SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];
const IDAT_CHUNK_SIZE: usize = 65_536;

/// Encodes an 8-bit packed frame as a non-interlaced PNG using adaptive row
/// filtering and repository-owned compressed zlib/DEFLATE output.
///
/// All five PNG row filters are evaluated independently for every row. The
/// deterministic score is the sum of absolute signed filtered-byte magnitudes;
/// ties prefer the numerically smaller filter type.
///
/// # Errors
///
/// Returns an error for invalid frame storage, arithmetic/allocation overflow,
/// or compression/chunk-size failure.
pub fn encode_png(frame: &VideoFrame) -> Result<Vec<u8>> {
    let color_type = match frame.format {
        PixelFormat::Gray8 => 0_u8,
        PixelFormat::Rgb24 => 2_u8,
        PixelFormat::GrayAlpha8 => 4_u8,
        PixelFormat::Rgba32 => 6_u8,
    };
    let stride = frame
        .format
        .packed_row_bytes(frame.width)
        .ok_or_else(|| MediaError::overflow("PNG row stride overflow"))?;
    let row_count = usize::try_from(frame.height)
        .map_err(|_| MediaError::overflow("PNG height exceeds usize"))?;
    let filtered_capacity = stride
        .checked_add(1)
        .and_then(|row| row.checked_mul(row_count))
        .ok_or_else(|| MediaError::overflow("PNG filtered image size overflow"))?;
    let mut filtered = Vec::with_capacity(filtered_capacity);
    let mut previous = vec![0_u8; stride];
    let bytes_per_pixel = frame.format.bytes_per_pixel();

    for y in 0..frame.height {
        let raw = frame.row(y)?;
        let (filter, encoded) = choose_filter(raw, &previous, bytes_per_pixel);
        filtered.push(filter);
        filtered.extend_from_slice(&encoded);
        previous.copy_from_slice(raw);
    }

    let compressed = zlib_encode::compress(&filtered)?;
    let mut output = PNG_SIGNATURE.to_vec();
    let mut ihdr = [0_u8; 13];
    ihdr[0..4].copy_from_slice(&frame.width.to_be_bytes());
    ihdr[4..8].copy_from_slice(&frame.height.to_be_bytes());
    ihdr[8] = 8;
    ihdr[9] = color_type;
    write_chunk(&mut output, *b"IHDR", &ihdr)?;
    for chunk in compressed.chunks(IDAT_CHUNK_SIZE) {
        write_chunk(&mut output, *b"IDAT", chunk)?;
    }
    write_chunk(&mut output, *b"IEND", &[])?;
    Ok(output)
}

fn choose_filter(raw: &[u8], previous: &[u8], bpp: usize) -> (u8, Vec<u8>) {
    let mut best_filter = 0_u8;
    let mut best = filter_row(0, raw, previous, bpp);
    let mut best_score = filter_score(&best);
    for filter in 1..=4 {
        let candidate = filter_row(filter, raw, previous, bpp);
        let score = filter_score(&candidate);
        if score < best_score {
            best_filter = filter;
            best_score = score;
            best = candidate;
        }
    }
    (best_filter, best)
}

fn filter_row(filter: u8, raw: &[u8], previous: &[u8], bpp: usize) -> Vec<u8> {
    raw.iter()
        .enumerate()
        .map(|(index, &value)| {
            let left = if index >= bpp { raw[index - bpp] } else { 0 };
            let up = previous.get(index).copied().unwrap_or(0);
            let up_left = if index >= bpp {
                previous.get(index - bpp).copied().unwrap_or(0)
            } else {
                0
            };
            let predictor = match filter {
                0 => 0,
                1 => left,
                2 => up,
                3 => u8::try_from(u16::midpoint(u16::from(left), u16::from(up)))
                    .expect("midpoint of two u8 values fits u8"),
                4 => paeth(left, up, up_left),
                _ => unreachable!("PNG filter type is in 0..=4"),
            };
            value.wrapping_sub(predictor)
        })
        .collect()
}

fn filter_score(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .map(|&byte| {
            let magnitude = u16::from(byte).min(256 - u16::from(byte));
            u64::from(magnitude)
        })
        .sum()
}

fn paeth(left: u8, up: u8, up_left: u8) -> u8 {
    let left_i = i32::from(left);
    let up_i = i32::from(up);
    let diagonal_i = i32::from(up_left);
    let prediction = left_i + up_i - diagonal_i;
    let dl = (prediction - left_i).abs();
    let du = (prediction - up_i).abs();
    let dd = (prediction - diagonal_i).abs();
    if dl <= du && dl <= dd {
        left
    } else if du <= dd {
        up
    } else {
        up_left
    }
}

fn write_chunk(output: &mut Vec<u8>, kind: [u8; 4], data: &[u8]) -> Result<()> {
    let length = u32::try_from(data.len())
        .map_err(|_| MediaError::overflow("PNG chunk payload exceeds u32"))?;
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

    fn frame(format: PixelFormat, width: u32, height: u32, data: Vec<u8>) -> VideoFrame {
        VideoFrame::from_vec(width, height, format, data).unwrap()
    }

    #[test]
    fn all_packed_formats_round_trip_through_decoder() {
        let cases = [
            frame(PixelFormat::Gray8, 3, 2, vec![0, 64, 255, 1, 65, 254]),
            frame(
                PixelFormat::Rgb24,
                2,
                2,
                vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255],
            ),
            frame(
                PixelFormat::GrayAlpha8,
                2,
                2,
                vec![0, 0, 64, 85, 128, 170, 255, 255],
            ),
            frame(
                PixelFormat::Rgba32,
                2,
                2,
                vec![
                    255, 0, 0, 0, 0, 255, 0, 85, 0, 0, 255, 170, 255, 255, 255, 255,
                ],
            ),
        ];
        for source in cases {
            let encoded = encode_png(&source).unwrap();
            let decoded = crate::png::decode_png(&encoded).unwrap();
            assert_eq!(decoded, source);
        }
    }

    #[test]
    fn adaptive_filter_chooses_nonzero_filter_for_gradient_rows() {
        let first = vec![10_u8, 20, 30, 40, 50, 60];
        let second = vec![11_u8, 21, 31, 41, 51, 61];
        let (filter, _) = choose_filter(&second, &first, 1);
        assert_ne!(filter, 0);
    }

    #[test]
    fn compressed_png_is_much_smaller_than_stored_baseline_on_flat_image() {
        let source = frame(PixelFormat::Rgba32, 256, 256, vec![0x80; 256 * 256 * 4]);
        let compressed = encode_png(&source).unwrap();
        assert!(compressed.len() < source.data.len() / 8);
    }
}
