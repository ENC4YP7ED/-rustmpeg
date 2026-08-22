use rm_core::video::{PixelFormat, VideoFrame};
use rm_core::{MediaError, Result};

const HEADER_SIZE: usize = 18;
const TYPE_TRUECOLOR: u8 = 2;
const TYPE_GRAYSCALE: u8 = 3;
const TYPE_RLE_TRUECOLOR: u8 = 10;
const TYPE_RLE_GRAYSCALE: u8 = 11;

#[must_use]
pub fn probe_tga(bytes: &[u8]) -> u8 {
    if bytes.len() < HEADER_SIZE || bytes[1] != 0 {
        return 0;
    }
    let image_type = bytes[2];
    let depth = bytes[16];
    let valid = matches!(image_type, TYPE_TRUECOLOR | TYPE_RLE_TRUECOLOR)
        && matches!(depth, 24 | 32)
        || matches!(image_type, TYPE_GRAYSCALE | TYPE_RLE_GRAYSCALE) && depth == 8;
    if valid { 75 } else { 0 }
}

pub fn decode_tga(bytes: &[u8]) -> Result<VideoFrame> {
    if bytes.len() < HEADER_SIZE {
        return Err(MediaError::invalid_data(
            "TGA input is shorter than its 18-byte header",
        ));
    }
    if bytes[1] != 0 {
        return Err(MediaError::unsupported(
            "color-mapped TGA images are not implemented yet",
        ));
    }

    let id_length = usize::from(bytes[0]);
    let image_type = bytes[2];
    let width = u32::from(read_u16_le(bytes, 12)?);
    let height = u32::from(read_u16_le(bytes, 14)?);
    if width == 0 || height == 0 {
        return Err(MediaError::invalid_data("TGA dimensions must be non-zero"));
    }

    let depth = bytes[16];
    let (format, bytes_per_pixel, rle) = match (image_type, depth) {
        (TYPE_TRUECOLOR, 24 | 32) => (PixelFormat::Rgb24, usize::from(depth / 8), false),
        (TYPE_GRAYSCALE, 8) => (PixelFormat::Gray8, 1, false),
        (TYPE_RLE_TRUECOLOR, 24 | 32) => (PixelFormat::Rgb24, usize::from(depth / 8), true),
        (TYPE_RLE_GRAYSCALE, 8) => (PixelFormat::Gray8, 1, true),
        (TYPE_TRUECOLOR | TYPE_RLE_TRUECOLOR, _) => {
            return Err(MediaError::unsupported(format!(
                "TGA truecolor depth {depth} is not implemented"
            )));
        }
        (TYPE_GRAYSCALE | TYPE_RLE_GRAYSCALE, _) => {
            return Err(MediaError::unsupported(format!(
                "TGA grayscale depth {depth} is not implemented"
            )));
        }
        _ => {
            return Err(MediaError::unsupported(format!(
                "TGA image type {image_type} is not implemented"
            )));
        }
    };

    let data_offset = HEADER_SIZE
        .checked_add(id_length)
        .ok_or_else(|| MediaError::overflow("TGA image ID offset overflow"))?;
    if data_offset > bytes.len() {
        return Err(MediaError::invalid_data("TGA image ID field is truncated"));
    }

    let pixel_count = usize::try_from(u64::from(width) * u64::from(height))
        .map_err(|_| MediaError::overflow("TGA pixel count exceeds usize"))?;
    let source_size = pixel_count
        .checked_mul(bytes_per_pixel)
        .ok_or_else(|| MediaError::overflow("TGA source raster size overflow"))?;
    let source = if rle {
        decode_rle(&bytes[data_offset..], pixel_count, bytes_per_pixel)?
    } else {
        let end = data_offset
            .checked_add(source_size)
            .ok_or_else(|| MediaError::overflow("TGA raster range overflow"))?;
        bytes
            .get(data_offset..end)
            .ok_or_else(|| MediaError::invalid_data("TGA raster is truncated"))?
            .to_vec()
    };

    let output_channels = format.bytes_per_pixel();
    let output_len = pixel_count
        .checked_mul(output_channels)
        .ok_or_else(|| MediaError::overflow("TGA output raster size overflow"))?;
    let mut output = vec![0_u8; output_len];
    let descriptor = bytes[17];
    let right_origin = descriptor & 0x10 != 0;
    let top_origin = descriptor & 0x20 != 0;
    let width_usize =
        usize::try_from(width).map_err(|_| MediaError::overflow("TGA width exceeds usize"))?;
    let height_usize =
        usize::try_from(height).map_err(|_| MediaError::overflow("TGA height exceeds usize"))?;

    for file_index in 0..pixel_count {
        let file_y = file_index / width_usize;
        let file_x = file_index % width_usize;
        let x = if right_origin {
            width_usize - 1 - file_x
        } else {
            file_x
        };
        let y = if top_origin {
            file_y
        } else {
            height_usize - 1 - file_y
        };
        let destination_index = y
            .checked_mul(width_usize)
            .and_then(|value| value.checked_add(x))
            .and_then(|value| value.checked_mul(output_channels))
            .ok_or_else(|| MediaError::overflow("TGA destination pixel offset overflow"))?;
        let source_index = file_index
            .checked_mul(bytes_per_pixel)
            .ok_or_else(|| MediaError::overflow("TGA source pixel offset overflow"))?;

        if format == PixelFormat::Gray8 {
            output[destination_index] = source[source_index];
        } else {
            output[destination_index] = source[source_index + 2];
            output[destination_index + 1] = source[source_index + 1];
            output[destination_index + 2] = source[source_index];
        }
    }

    VideoFrame::from_vec(width, height, format, output)
}

pub fn encode_tga(frame: &VideoFrame) -> Result<Vec<u8>> {
    encode_tga_with_rle(frame, false)
}

pub fn encode_tga_with_rle(frame: &VideoFrame, rle: bool) -> Result<Vec<u8>> {
    let (image_type, depth, source_channels) = match frame.format {
        PixelFormat::Gray8 => (
            if rle {
                TYPE_RLE_GRAYSCALE
            } else {
                TYPE_GRAYSCALE
            },
            8_u8,
            1_usize,
        ),
        PixelFormat::Rgb24 => (
            if rle {
                TYPE_RLE_TRUECOLOR
            } else {
                TYPE_TRUECOLOR
            },
            24_u8,
            3_usize,
        ),
    };
    let width = u16::try_from(frame.width)
        .map_err(|_| MediaError::unsupported("TGA width exceeds 65535 pixels"))?;
    let height = u16::try_from(frame.height)
        .map_err(|_| MediaError::unsupported("TGA height exceeds 65535 pixels"))?;

    let pixel_count = usize::try_from(frame.pixel_count())
        .map_err(|_| MediaError::overflow("TGA pixel count exceeds usize"))?;
    let raw_size = pixel_count
        .checked_mul(source_channels)
        .ok_or_else(|| MediaError::overflow("TGA raster size overflow"))?;
    let mut raw = Vec::with_capacity(raw_size);
    match frame.format {
        PixelFormat::Gray8 => raw.extend_from_slice(frame.data.as_slice()),
        PixelFormat::Rgb24 => {
            for pixel in frame.data.as_slice().chunks_exact(3) {
                raw.extend_from_slice(&[pixel[2], pixel[1], pixel[0]]);
            }
        }
    }

    let raster = if rle {
        encode_rle(&raw, source_channels)?
    } else {
        raw
    };
    let mut output = Vec::with_capacity(
        HEADER_SIZE
            .checked_add(raster.len())
            .ok_or_else(|| MediaError::overflow("TGA file size overflow"))?,
    );
    output.push(0);
    output.push(0);
    output.push(image_type);
    output.extend_from_slice(&[0; 5]);
    output.extend_from_slice(&0_u16.to_le_bytes());
    output.extend_from_slice(&0_u16.to_le_bytes());
    output.extend_from_slice(&width.to_le_bytes());
    output.extend_from_slice(&height.to_le_bytes());
    output.push(depth);
    output.push(0x20);
    output.extend_from_slice(&raster);
    Ok(output)
}

fn decode_rle(input: &[u8], pixel_count: usize, bytes_per_pixel: usize) -> Result<Vec<u8>> {
    let output_size = pixel_count
        .checked_mul(bytes_per_pixel)
        .ok_or_else(|| MediaError::overflow("TGA RLE output size overflow"))?;
    let mut output = Vec::with_capacity(output_size);
    let mut position = 0_usize;
    let mut produced = 0_usize;

    while produced < pixel_count {
        let header = *input
            .get(position)
            .ok_or_else(|| MediaError::invalid_data("TGA RLE packet header is truncated"))?;
        position += 1;
        let count = usize::from(header & 0x7F) + 1;
        if produced
            .checked_add(count)
            .is_none_or(|value| value > pixel_count)
        {
            return Err(MediaError::invalid_data(
                "TGA RLE packet exceeds declared pixel count",
            ));
        }

        if header & 0x80 != 0 {
            let end = position
                .checked_add(bytes_per_pixel)
                .ok_or_else(|| MediaError::overflow("TGA RLE run range overflow"))?;
            let pixel = input
                .get(position..end)
                .ok_or_else(|| MediaError::invalid_data("TGA RLE run pixel is truncated"))?;
            position = end;
            for _ in 0..count {
                output.extend_from_slice(pixel);
            }
        } else {
            let bytes = count
                .checked_mul(bytes_per_pixel)
                .ok_or_else(|| MediaError::overflow("TGA raw RLE packet size overflow"))?;
            let end = position
                .checked_add(bytes)
                .ok_or_else(|| MediaError::overflow("TGA raw RLE packet range overflow"))?;
            let packet = input
                .get(position..end)
                .ok_or_else(|| MediaError::invalid_data("TGA raw RLE packet is truncated"))?;
            position = end;
            output.extend_from_slice(packet);
        }
        produced += count;
    }

    debug_assert_eq!(output.len(), output_size);
    Ok(output)
}

fn encode_rle(raw: &[u8], bytes_per_pixel: usize) -> Result<Vec<u8>> {
    if bytes_per_pixel == 0 || !raw.len().is_multiple_of(bytes_per_pixel) {
        return Err(MediaError::invalid_argument(
            "TGA RLE source is not aligned to complete pixels",
        ));
    }
    let pixels = raw.len() / bytes_per_pixel;
    let mut output = Vec::with_capacity(raw.len());
    let mut index = 0_usize;

    while index < pixels {
        let run = run_length(raw, bytes_per_pixel, index, pixels);
        if run >= 2 {
            let count = run.min(128);
            output.push(0x80 | u8::try_from(count - 1).expect("TGA RLE packet count <= 128"));
            let start = index * bytes_per_pixel;
            output.extend_from_slice(&raw[start..start + bytes_per_pixel]);
            index += count;
            continue;
        }

        let raw_start = index;
        index += 1;
        while index < pixels && index - raw_start < 128 {
            if run_length(raw, bytes_per_pixel, index, pixels) >= 2 {
                break;
            }
            index += 1;
        }
        let count = index - raw_start;
        output.push(u8::try_from(count - 1).expect("TGA raw packet count <= 128"));
        let byte_start = raw_start
            .checked_mul(bytes_per_pixel)
            .ok_or_else(|| MediaError::overflow("TGA raw packet offset overflow"))?;
        let byte_end = index
            .checked_mul(bytes_per_pixel)
            .ok_or_else(|| MediaError::overflow("TGA raw packet range overflow"))?;
        output.extend_from_slice(&raw[byte_start..byte_end]);
    }
    Ok(output)
}

fn run_length(raw: &[u8], bytes_per_pixel: usize, start: usize, pixels: usize) -> usize {
    let first_start = start * bytes_per_pixel;
    let first = &raw[first_start..first_start + bytes_per_pixel];
    let mut count = 1_usize;
    while start + count < pixels && count < 128 {
        let candidate_start = (start + count) * bytes_per_pixel;
        if &raw[candidate_start..candidate_start + bytes_per_pixel] != first {
            break;
        }
        count += 1;
    }
    count
}

fn read_u16_le(bytes: &[u8], offset: usize) -> Result<u16> {
    let value = bytes
        .get(offset..offset + 2)
        .ok_or_else(|| MediaError::eof("unexpected end of TGA header"))?;
    Ok(u16::from_le_bytes([value[0], value[1]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb_frame() -> VideoFrame {
        VideoFrame::from_vec(
            2,
            2,
            PixelFormat::Rgb24,
            vec![
                255, 0, 0, 0, 255, 0, // top
                0, 0, 255, 255, 255, 255, // bottom
            ],
        )
        .unwrap()
    }

    #[test]
    fn uncompressed_truecolor_round_trip_is_exact() {
        let frame = rgb_frame();
        let encoded = encode_tga(&frame).unwrap();
        assert_eq!(probe_tga(&encoded), 75);
        assert_eq!(decode_tga(&encoded).unwrap(), frame);
        assert_eq!(encoded[2], TYPE_TRUECOLOR);
        assert_eq!(encoded[17] & 0x20, 0x20);
    }

    #[test]
    fn grayscale_round_trip_is_exact() {
        let frame = VideoFrame::from_vec(3, 1, PixelFormat::Gray8, vec![0, 127, 255]).unwrap();
        let encoded = encode_tga(&frame).unwrap();
        assert_eq!(encoded[2], TYPE_GRAYSCALE);
        assert_eq!(decode_tga(&encoded).unwrap(), frame);
    }

    #[test]
    fn rle_encoder_and_decoder_preserve_repeated_and_raw_packets() {
        let frame = VideoFrame::from_vec(
            6,
            1,
            PixelFormat::Rgb24,
            vec![1, 2, 3, 1, 2, 3, 1, 2, 3, 9, 8, 7, 6, 5, 4, 6, 5, 4],
        )
        .unwrap();
        let encoded = encode_tga_with_rle(&frame, true).unwrap();
        assert_eq!(encoded[2], TYPE_RLE_TRUECOLOR);
        assert_eq!(decode_tga(&encoded).unwrap(), frame);
        assert!(encoded.len() < HEADER_SIZE + frame.data.len());
    }

    #[test]
    fn all_origin_combinations_map_to_top_left_frame() {
        let canonical = encode_tga(&rgb_frame()).unwrap();
        let raster = canonical[HEADER_SIZE..].to_vec();

        for descriptor in [0x00_u8, 0x10, 0x20, 0x30] {
            let mut file = canonical.clone();
            file[17] = descriptor;
            let top = descriptor & 0x20 != 0;
            let right = descriptor & 0x10 != 0;
            let mut reordered = Vec::with_capacity(raster.len());
            for file_y in 0..2_usize {
                let y = if top { file_y } else { 1 - file_y };
                for file_x in 0..2_usize {
                    let x = if right { 1 - file_x } else { file_x };
                    let source = (y * 2 + x) * 3;
                    reordered.extend_from_slice(&raster[source..source + 3]);
                }
            }
            file[HEADER_SIZE..].copy_from_slice(&reordered);
            assert_eq!(decode_tga(&file).unwrap(), rgb_frame());
        }
    }

    #[test]
    fn malformed_rle_and_unsupported_headers_are_rejected() {
        let mut mapped = encode_tga(&rgb_frame()).unwrap();
        mapped[1] = 1;
        assert!(decode_tga(&mapped).is_err());

        let mut bad_depth = encode_tga(&rgb_frame()).unwrap();
        bad_depth[16] = 16;
        assert!(decode_tga(&bad_depth).is_err());

        let mut rle = encode_tga_with_rle(&rgb_frame(), true).unwrap();
        rle.truncate(HEADER_SIZE + 1);
        assert!(decode_tga(&rle).is_err());

        let mut overflow_packet = encode_tga_with_rle(&rgb_frame(), true).unwrap();
        overflow_packet.truncate(HEADER_SIZE);
        overflow_packet.push(0x84);
        overflow_packet.extend_from_slice(&[0, 0, 0]);
        assert!(decode_tga(&overflow_packet).is_err());
    }
}
