#[path = "png_baseline.rs"]
mod baseline;

pub use baseline::{encode_png, probe_png};

use rm_compress::{crc32, zlib};
use rm_core::video::{PixelFormat, VideoFrame};
use rm_core::{MediaError, Result};

const PNG_SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];
const MAX_PIXELS: u64 = 268_435_456;

#[derive(Debug, Clone, Copy)]
struct Header16 {
    width: u32,
    height: u32,
    color_type: u8,
}

/// Decode PNG while routing already validated <=8-bit behavior to the baseline
/// decoder and keeping 16-bit sample handling isolated.
pub fn decode_png(bytes: &[u8]) -> Result<VideoFrame> {
    if is_16_bit_png(bytes) {
        decode_png_16(bytes)
    } else {
        baseline::decode_png(bytes)
    }
}

fn is_16_bit_png(bytes: &[u8]) -> bool {
    bytes.len() >= 29
        && bytes.starts_with(&PNG_SIGNATURE)
        && bytes.get(8..12) == Some(&13_u32.to_be_bytes())
        && bytes.get(12..16) == Some(b"IHDR")
        && bytes[24] == 16
}

fn decode_png_16(bytes: &[u8]) -> Result<VideoFrame> {
    if !bytes.starts_with(&PNG_SIGNATURE) {
        return Err(MediaError::invalid_data("missing PNG signature"));
    }

    let mut position = PNG_SIGNATURE.len();
    let mut header = None;
    let mut idat = Vec::new();
    let mut transparency: Option<Vec<u8>> = None;
    let mut saw_idat = false;
    let mut idat_ended = false;
    let mut saw_iend = false;
    let mut saw_plte = false;

    while position < bytes.len() {
        if saw_iend {
            return Err(MediaError::invalid_data("bytes found after PNG IEND chunk"));
        }

        let header_end = position
            .checked_add(8)
            .ok_or_else(|| MediaError::overflow("PNG chunk header range overflow"))?;
        let chunk_header = bytes
            .get(position..header_end)
            .ok_or_else(|| MediaError::eof("truncated PNG chunk header"))?;
        let length = usize::try_from(u32::from_be_bytes([
            chunk_header[0],
            chunk_header[1],
            chunk_header[2],
            chunk_header[3],
        ]))
        .map_err(|_| MediaError::overflow("PNG chunk length exceeds usize"))?;
        let chunk_type = [
            chunk_header[4],
            chunk_header[5],
            chunk_header[6],
            chunk_header[7],
        ];
        validate_chunk_type(chunk_type)?;
        position = header_end;

        let data_end = position
            .checked_add(length)
            .ok_or_else(|| MediaError::overflow("PNG chunk data range overflow"))?;
        let crc_end = data_end
            .checked_add(4)
            .ok_or_else(|| MediaError::overflow("PNG chunk CRC range overflow"))?;
        let data = bytes
            .get(position..data_end)
            .ok_or_else(|| MediaError::eof("truncated PNG chunk data"))?;
        let crc_bytes = bytes
            .get(data_end..crc_end)
            .ok_or_else(|| MediaError::eof("truncated PNG chunk CRC"))?;
        let expected_crc =
            u32::from_be_bytes([crc_bytes[0], crc_bytes[1], crc_bytes[2], crc_bytes[3]]);
        let actual_crc = chunk_crc(chunk_type, data);
        if expected_crc != actual_crc {
            return Err(MediaError::invalid_data(format!(
                "PNG chunk {} CRC mismatch: expected 0x{expected_crc:08x}, got 0x{actual_crc:08x}",
                chunk_name(chunk_type)
            )));
        }
        position = crc_end;

        match &chunk_type {
            b"IHDR" => {
                if header.is_some() || saw_idat || saw_iend {
                    return Err(MediaError::invalid_data(
                        "PNG IHDR must appear exactly once and first",
                    ));
                }
                if position - (length + 12) != PNG_SIGNATURE.len() {
                    return Err(MediaError::invalid_data("PNG IHDR is not the first chunk"));
                }
                header = Some(parse_header_16(data)?);
            }
            b"PLTE" => {
                let png = header
                    .ok_or_else(|| MediaError::invalid_data("PNG PLTE appeared before IHDR"))?;
                if saw_idat {
                    return Err(MediaError::invalid_data("PNG PLTE appeared after IDAT"));
                }
                if saw_plte {
                    return Err(MediaError::invalid_data(
                        "PNG contains multiple PLTE chunks",
                    ));
                }
                if matches!(png.color_type, 0 | 4) {
                    return Err(MediaError::invalid_data(
                        "grayscale PNG must not contain a PLTE chunk",
                    ));
                }
                if data.is_empty() || data.len() % 3 != 0 || data.len() > 768 {
                    return Err(MediaError::invalid_data("invalid PNG PLTE length"));
                }
                saw_plte = true;
            }
            b"tRNS" => {
                let png = header
                    .ok_or_else(|| MediaError::invalid_data("PNG tRNS appeared before IHDR"))?;
                if saw_idat {
                    return Err(MediaError::invalid_data("PNG tRNS appeared after IDAT"));
                }
                if transparency.is_some() {
                    return Err(MediaError::invalid_data(
                        "PNG contains multiple tRNS chunks",
                    ));
                }
                match png.color_type {
                    0 if data.len() != 2 => {
                        return Err(MediaError::invalid_data(
                            "grayscale PNG tRNS length must be 2 bytes",
                        ));
                    }
                    2 if data.len() != 6 => {
                        return Err(MediaError::invalid_data(
                            "truecolor PNG tRNS length must be 6 bytes",
                        ));
                    }
                    4 | 6 => {
                        return Err(MediaError::invalid_data(
                            "PNG color types with alpha must not contain tRNS",
                        ));
                    }
                    _ => {}
                }
                transparency = Some(data.to_vec());
            }
            b"IDAT" => {
                if header.is_none() {
                    return Err(MediaError::invalid_data("PNG IDAT appeared before IHDR"));
                }
                if idat_ended {
                    return Err(MediaError::invalid_data(
                        "PNG IDAT chunks must be consecutive",
                    ));
                }
                saw_idat = true;
                idat.try_reserve(data.len())
                    .map_err(|_| MediaError::overflow("PNG IDAT allocation failed"))?;
                idat.extend_from_slice(data);
            }
            b"IEND" => {
                if header.is_none() || !saw_idat {
                    return Err(MediaError::invalid_data(
                        "PNG IEND requires preceding IHDR and IDAT",
                    ));
                }
                if !data.is_empty() {
                    return Err(MediaError::invalid_data("PNG IEND chunk must be empty"));
                }
                saw_iend = true;
            }
            _ => {
                if saw_idat {
                    idat_ended = true;
                }
                if is_critical(chunk_type) {
                    return Err(MediaError::unsupported(format!(
                        "unsupported critical PNG chunk {}",
                        chunk_name(chunk_type)
                    )));
                }
            }
        }
    }

    if !saw_iend {
        return Err(MediaError::invalid_data("PNG has no IEND chunk"));
    }
    let header = header.ok_or_else(|| MediaError::invalid_data("PNG has no IHDR chunk"))?;
    decode_scanlines_16(header, &idat, transparency.as_deref())
}

fn parse_header_16(data: &[u8]) -> Result<Header16> {
    if data.len() != 13 {
        return Err(MediaError::invalid_data("PNG IHDR length must be 13 bytes"));
    }
    let width = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
    let height = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    let bit_depth = data[8];
    let color_type = data[9];
    let compression_method = data[10];
    let filter_method = data[11];
    let interlace_method = data[12];

    if width == 0 || height == 0 {
        return Err(MediaError::invalid_data("PNG dimensions must be non-zero"));
    }
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or_else(|| MediaError::overflow("PNG pixel count overflow"))?;
    if pixels > MAX_PIXELS {
        return Err(MediaError::unsupported(format!(
            "PNG image has {pixels} pixels; limit is {MAX_PIXELS}"
        )));
    }
    if bit_depth != 16 {
        return Err(MediaError::unsupported(
            "16-bit PNG decoder received a non-16-bit image",
        ));
    }
    if !matches!(color_type, 0 | 2 | 4 | 6) {
        return Err(MediaError::unsupported(format!(
            "PNG color type {color_type} is invalid for 16-bit decoding"
        )));
    }
    if compression_method != 0 {
        return Err(MediaError::unsupported(
            "unsupported PNG compression method",
        ));
    }
    if filter_method != 0 {
        return Err(MediaError::unsupported("unsupported PNG filter method"));
    }
    if interlace_method != 0 {
        return Err(MediaError::unsupported(
            "Adam7-interlaced PNG is not implemented yet",
        ));
    }

    Ok(Header16 {
        width,
        height,
        color_type,
    })
}

fn decode_scanlines_16(
    header: Header16,
    compressed: &[u8],
    transparency: Option<&[u8]>,
) -> Result<VideoFrame> {
    let channels = match header.color_type {
        0 => 1_usize,
        2 => 3_usize,
        4 => 2_usize,
        6 => 4_usize,
        _ => return Err(MediaError::unsupported("unsupported 16-bit PNG color type")),
    };
    let width = usize::try_from(header.width)
        .map_err(|_| MediaError::overflow("PNG width exceeds usize"))?;
    let height = usize::try_from(header.height)
        .map_err(|_| MediaError::overflow("PNG height exceeds usize"))?;
    let bytes_per_pixel = channels
        .checked_mul(2)
        .ok_or_else(|| MediaError::overflow("PNG 16-bit bytes-per-pixel overflow"))?;
    let stride = width
        .checked_mul(bytes_per_pixel)
        .ok_or_else(|| MediaError::overflow("PNG 16-bit row stride overflow"))?;
    let raw = decode_filtered_rows(compressed, stride, height, bytes_per_pixel)?;

    let gray_key = if header.color_type == 0 {
        transparency.map(|key| u16::from_be_bytes([key[0], key[1]]))
    } else {
        None
    };
    let rgb_key = if header.color_type == 2 {
        transparency.map(|key| {
            [
                u16::from_be_bytes([key[0], key[1]]),
                u16::from_be_bytes([key[2], key[3]]),
                u16::from_be_bytes([key[4], key[5]]),
            ]
        })
    } else {
        None
    };

    let output_format = match header.color_type {
        0 if gray_key.is_some() => PixelFormat::GrayAlpha8,
        0 => PixelFormat::Gray8,
        2 if rgb_key.is_some() => PixelFormat::Rgba32,
        2 => PixelFormat::Rgb24,
        4 => PixelFormat::GrayAlpha8,
        6 => PixelFormat::Rgba32,
        _ => return Err(MediaError::unsupported("unsupported 16-bit PNG color type")),
    };
    let pixel_count = width
        .checked_mul(height)
        .ok_or_else(|| MediaError::overflow("PNG 16-bit pixel count overflow"))?;
    let output_size = pixel_count
        .checked_mul(output_format.bytes_per_pixel())
        .ok_or_else(|| MediaError::overflow("PNG 16-bit output size overflow"))?;
    let mut output = Vec::with_capacity(output_size);

    for pixel in raw.chunks_exact(bytes_per_pixel) {
        match header.color_type {
            0 => {
                let gray = read_be_u16(pixel, 0)?;
                output.push(scale_u16_to_u8(gray)?);
                if let Some(key) = gray_key {
                    output.push(if gray == key { 0 } else { 255 });
                }
            }
            2 => {
                let red = read_be_u16(pixel, 0)?;
                let green = read_be_u16(pixel, 2)?;
                let blue = read_be_u16(pixel, 4)?;
                output.extend_from_slice(&[
                    scale_u16_to_u8(red)?,
                    scale_u16_to_u8(green)?,
                    scale_u16_to_u8(blue)?,
                ]);
                if let Some(key) = rgb_key {
                    output.push(if [red, green, blue] == key { 0 } else { 255 });
                }
            }
            4 => {
                let gray = read_be_u16(pixel, 0)?;
                let alpha = read_be_u16(pixel, 2)?;
                output.extend_from_slice(&[scale_u16_to_u8(gray)?, scale_u16_to_u8(alpha)?]);
            }
            6 => {
                let red = read_be_u16(pixel, 0)?;
                let green = read_be_u16(pixel, 2)?;
                let blue = read_be_u16(pixel, 4)?;
                let alpha = read_be_u16(pixel, 6)?;
                output.extend_from_slice(&[
                    scale_u16_to_u8(red)?,
                    scale_u16_to_u8(green)?,
                    scale_u16_to_u8(blue)?,
                    scale_u16_to_u8(alpha)?,
                ]);
            }
            _ => unreachable!(),
        }
    }

    if output.len() != output_size {
        return Err(MediaError::invalid_data("PNG 16-bit sample count mismatch"));
    }
    VideoFrame::from_vec(header.width, header.height, output_format, output)
}

fn decode_filtered_rows(
    compressed: &[u8],
    stride: usize,
    height: usize,
    bytes_per_pixel: usize,
) -> Result<Vec<u8>> {
    let encoded_row = stride
        .checked_add(1)
        .ok_or_else(|| MediaError::overflow("PNG encoded row size overflow"))?;
    let expected_size = encoded_row
        .checked_mul(height)
        .ok_or_else(|| MediaError::overflow("PNG decompressed size overflow"))?;
    let filtered = zlib::decompress(compressed, expected_size)?;
    if filtered.len() != expected_size {
        return Err(MediaError::invalid_data(format!(
            "PNG decompressed data has {} bytes but {expected_size} are required",
            filtered.len()
        )));
    }

    let raw_size = stride
        .checked_mul(height)
        .ok_or_else(|| MediaError::overflow("PNG raster size overflow"))?;
    let mut output = vec![0_u8; raw_size];
    for row_index in 0..height {
        let encoded_start = row_index * encoded_row;
        let filter = filtered[encoded_start];
        let source = &filtered[encoded_start + 1..encoded_start + encoded_row];
        let output_start = row_index * stride;
        let (before, current_and_after) = output.split_at_mut(output_start);
        let current = &mut current_and_after[..stride];
        let previous = if row_index == 0 {
            None
        } else {
            Some(&before[before.len() - stride..])
        };
        unfilter_row(filter, source, current, previous, bytes_per_pixel)?;
    }
    Ok(output)
}

fn unfilter_row(
    filter: u8,
    source: &[u8],
    output: &mut [u8],
    previous: Option<&[u8]>,
    bytes_per_pixel: usize,
) -> Result<()> {
    for index in 0..source.len() {
        let left = if index >= bytes_per_pixel {
            output[index - bytes_per_pixel]
        } else {
            0
        };
        let up = previous.map_or(0, |row| row[index]);
        let up_left = if index >= bytes_per_pixel {
            previous.map_or(0, |row| row[index - bytes_per_pixel])
        } else {
            0
        };
        let predictor = match filter {
            0 => 0,
            1 => left,
            2 => up,
            3 => ((u16::from(left) + u16::from(up)) / 2) as u8,
            4 => paeth(left, up, up_left),
            _ => {
                return Err(MediaError::invalid_data(format!(
                    "PNG scanline uses invalid filter type {filter}"
                )));
            }
        };
        output[index] = source[index].wrapping_add(predictor);
    }
    Ok(())
}

fn paeth(left: u8, up: u8, up_left: u8) -> u8 {
    let left = i32::from(left);
    let up = i32::from(up);
    let up_left = i32::from(up_left);
    let estimate = left + up - up_left;
    let left_distance = (estimate - left).abs();
    let up_distance = (estimate - up).abs();
    let up_left_distance = (estimate - up_left).abs();
    if left_distance <= up_distance && left_distance <= up_left_distance {
        left as u8
    } else if up_distance <= up_left_distance {
        up as u8
    } else {
        up_left as u8
    }
}

fn read_be_u16(bytes: &[u8], offset: usize) -> Result<u16> {
    let pair = bytes
        .get(offset..offset + 2)
        .ok_or_else(|| MediaError::invalid_data("PNG 16-bit sample is truncated"))?;
    Ok(u16::from_be_bytes([pair[0], pair[1]]))
}

fn scale_u16_to_u8(sample: u16) -> Result<u8> {
    let scaled = (u32::from(sample) * 255 + 32_767) / 65_535;
    u8::try_from(scaled).map_err(|_| MediaError::overflow("PNG 16-bit scaling exceeds u8"))
}

fn validate_chunk_type(chunk_type: [u8; 4]) -> Result<()> {
    if !chunk_type.iter().all(u8::is_ascii_alphabetic) {
        return Err(MediaError::invalid_data(
            "PNG chunk type must contain four ASCII letters",
        ));
    }
    if chunk_type[2].is_ascii_lowercase() {
        return Err(MediaError::invalid_data(
            "PNG chunk type has invalid reserved lowercase bit",
        ));
    }
    Ok(())
}

fn is_critical(chunk_type: [u8; 4]) -> bool {
    chunk_type[0].is_ascii_uppercase()
}

fn chunk_name(chunk_type: [u8; 4]) -> String {
    String::from_utf8_lossy(&chunk_type).into_owned()
}

fn chunk_crc(chunk_type: [u8; 4], data: &[u8]) -> u32 {
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(&chunk_type);
    crc_input.extend_from_slice(data);
    crc32(&crc_input)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_chunk(output: &mut Vec<u8>, chunk_type: [u8; 4], data: &[u8]) {
        output.extend_from_slice(&u32::try_from(data.len()).unwrap().to_be_bytes());
        output.extend_from_slice(&chunk_type);
        output.extend_from_slice(data);
        output.extend_from_slice(&chunk_crc(chunk_type, data).to_be_bytes());
    }

    fn gray16_with_transparency(samples: &[u16], key: u16) -> Vec<u8> {
        let mut filtered = Vec::with_capacity(1 + samples.len() * 2);
        filtered.push(0);
        for sample in samples {
            filtered.extend_from_slice(&sample.to_be_bytes());
        }
        let compressed = zlib::compress_stored(&filtered).unwrap();

        let mut png = PNG_SIGNATURE.to_vec();
        let mut ihdr = [0_u8; 13];
        ihdr[0..4].copy_from_slice(&u32::try_from(samples.len()).unwrap().to_be_bytes());
        ihdr[4..8].copy_from_slice(&1_u32.to_be_bytes());
        ihdr[8] = 16;
        ihdr[9] = 0;
        write_chunk(&mut png, *b"IHDR", &ihdr);
        write_chunk(&mut png, *b"tRNS", &key.to_be_bytes());
        write_chunk(&mut png, *b"IDAT", &compressed);
        write_chunk(&mut png, *b"IEND", &[]);
        png
    }

    #[test]
    fn sixteen_bit_scaling_has_exact_endpoints_and_midpoint() {
        assert_eq!(scale_u16_to_u8(0).unwrap(), 0);
        assert_eq!(scale_u16_to_u8(u16::MAX).unwrap(), 255);
        assert_eq!(scale_u16_to_u8(32_768).unwrap(), 128);
    }

    #[test]
    fn transparency_key_is_compared_before_downconversion() {
        let png = gray16_with_transparency(&[0x1201, 0x1202], 0x1201);
        let frame = decode_png(&png).unwrap();
        assert_eq!(frame.format, PixelFormat::GrayAlpha8);
        assert_eq!(frame.data.as_slice()[1], 0);
        assert_eq!(frame.data.as_slice()[3], 255);
        assert_eq!(frame.data.as_slice()[0], frame.data.as_slice()[2]);
    }

    #[test]
    fn truncated_sixteen_bit_payload_is_rejected() {
        let mut png = gray16_with_transparency(&[0x1234], 0x1234);
        let idat = png.windows(4).position(|window| window == b"IDAT").unwrap();
        png.truncate(idat + 6);
        assert!(decode_png(&png).is_err());
    }
}
