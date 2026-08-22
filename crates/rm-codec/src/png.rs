use rm_compress::{crc32, zlib};
use rm_core::video::{PixelFormat, VideoFrame};
use rm_core::{MediaError, Result};

const PNG_SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];
const MAX_PIXELS: u64 = 268_435_456;
const IDAT_CHUNK_SIZE: usize = 1 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Ihdr {
    width: u32,
    height: u32,
    bit_depth: u8,
    color_type: u8,
    compression_method: u8,
    filter_method: u8,
    interlace_method: u8,
}

#[must_use]
pub fn probe_png(bytes: &[u8]) -> u8 {
    if bytes.starts_with(&PNG_SIGNATURE) {
        100
    } else {
        0
    }
}

/// Decodes non-interlaced 8-bit grayscale, RGB, gray+alpha, or RGBA PNG images.
///
/// # Errors
///
/// Returns an error for malformed chunk framing/order/CRC, unsupported PNG
/// features, invalid filters, zlib/DEFLATE failures, dimension overflow, or
/// decompressed scanline sizes that do not exactly match IHDR.
pub fn decode_png(bytes: &[u8]) -> Result<VideoFrame> {
    if !bytes.starts_with(&PNG_SIGNATURE) {
        return Err(MediaError::invalid_data("missing PNG signature"));
    }

    let mut position = PNG_SIGNATURE.len();
    let mut ihdr = None;
    let mut idat = Vec::new();
    let mut saw_idat = false;
    let mut idat_ended = false;
    let mut saw_iend = false;
    let mut saw_plte = false;
    let mut palette: Option<Vec<u8>> = None;
    let mut transparency: Option<Vec<u8>> = None;

    while position < bytes.len() {
        if saw_iend {
            return Err(MediaError::invalid_data("bytes found after PNG IEND chunk"));
        }
        let header_end = position
            .checked_add(8)
            .ok_or_else(|| MediaError::overflow("PNG chunk header range overflow"))?;
        let header = bytes
            .get(position..header_end)
            .ok_or_else(|| MediaError::eof("truncated PNG chunk header"))?;
        let length = usize::try_from(u32::from_be_bytes([
            header[0], header[1], header[2], header[3],
        ]))
        .map_err(|_| MediaError::overflow("PNG chunk length exceeds usize"))?;
        let chunk_type = [header[4], header[5], header[6], header[7]];
        position = header_end;

        validate_chunk_type(chunk_type)?;
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
        if actual_crc != expected_crc {
            return Err(MediaError::invalid_data(format!(
                "PNG chunk {} CRC mismatch: expected 0x{expected_crc:08x}, got 0x{actual_crc:08x}",
                chunk_name(chunk_type)
            )));
        }
        position = crc_end;

        match &chunk_type {
            b"IHDR" => {
                if ihdr.is_some() || saw_idat || saw_iend {
                    return Err(MediaError::invalid_data(
                        "PNG IHDR must appear exactly once and first",
                    ));
                }
                if position - (length + 12) != PNG_SIGNATURE.len() {
                    return Err(MediaError::invalid_data("PNG IHDR is not the first chunk"));
                }
                ihdr = Some(parse_ihdr(data)?);
            }
            b"PLTE" => {
                let header =
                    ihdr.ok_or_else(|| MediaError::invalid_data("PNG PLTE appeared before IHDR"))?;
                if saw_idat {
                    return Err(MediaError::invalid_data("PNG PLTE appeared after IDAT"));
                }
                if saw_plte {
                    return Err(MediaError::invalid_data(
                        "PNG contains multiple PLTE chunks",
                    ));
                }
                if matches!(header.color_type, 0 | 4) {
                    return Err(MediaError::invalid_data(
                        "grayscale PNG must not contain a PLTE chunk",
                    ));
                }
                if data.is_empty() || data.len() % 3 != 0 || data.len() > 768 {
                    return Err(MediaError::invalid_data("invalid PNG PLTE length"));
                }
                let entries = data.len() / 3;
                if header.color_type == 3 && entries > (1_usize << header.bit_depth) {
                    return Err(MediaError::invalid_data(
                        "PNG PLTE has more entries than indexed bit depth permits",
                    ));
                }
                palette = Some(data.to_vec());
                saw_plte = true;
            }
            b"tRNS" => {
                let header =
                    ihdr.ok_or_else(|| MediaError::invalid_data("PNG tRNS appeared before IHDR"))?;
                if saw_idat {
                    return Err(MediaError::invalid_data("PNG tRNS appeared after IDAT"));
                }
                if transparency.is_some() {
                    return Err(MediaError::invalid_data(
                        "PNG contains multiple tRNS chunks",
                    ));
                }
                match header.color_type {
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
                    3 => {
                        let entries = palette
                            .as_ref()
                            .ok_or_else(|| {
                                MediaError::invalid_data("indexed PNG tRNS requires PLTE")
                            })?
                            .len()
                            / 3;
                        if data.len() > entries {
                            return Err(MediaError::invalid_data(
                                "indexed PNG tRNS has more alpha entries than PLTE",
                            ));
                        }
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
                if ihdr.is_none() {
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
                if ihdr.is_none() || !saw_idat {
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
    let header = ihdr.ok_or_else(|| MediaError::invalid_data("PNG has no IHDR chunk"))?;
    if header.color_type == 3 && palette.is_none() {
        return Err(MediaError::invalid_data(
            "indexed PNG requires a PLTE chunk",
        ));
    }
    decode_scanlines(header, &idat, palette.as_deref(), transparency.as_deref())
}

/// Encodes an 8-bit grayscale, RGB, gray+alpha, or RGBA frame as a non-interlaced PNG.
///
/// The baseline encoder emits filter type 0 scanlines and repository-owned zlib
/// stored blocks. This is standards-compliant but intentionally not yet size
/// optimized.
///
/// # Errors
///
/// Returns an error for unsupported pixel formats or integer/allocation overflow.
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
    let filtered_size = stride
        .checked_add(1)
        .and_then(|row| row.checked_mul(row_count))
        .ok_or_else(|| MediaError::overflow("PNG filtered image size overflow"))?;
    let mut filtered = Vec::with_capacity(filtered_size);
    for y in 0..frame.height {
        filtered.push(0);
        filtered.extend_from_slice(frame.row(y)?);
    }
    let compressed = zlib::compress_stored(&filtered)?;

    let mut output = Vec::new();
    output.extend_from_slice(&PNG_SIGNATURE);

    let mut ihdr = [0_u8; 13];
    ihdr[0..4].copy_from_slice(&frame.width.to_be_bytes());
    ihdr[4..8].copy_from_slice(&frame.height.to_be_bytes());
    ihdr[8] = 8;
    ihdr[9] = color_type;
    ihdr[10] = 0;
    ihdr[11] = 0;
    ihdr[12] = 0;
    write_chunk(&mut output, *b"IHDR", &ihdr)?;

    for chunk in compressed.chunks(IDAT_CHUNK_SIZE) {
        write_chunk(&mut output, *b"IDAT", chunk)?;
    }
    write_chunk(&mut output, *b"IEND", &[])?;
    Ok(output)
}

fn parse_ihdr(data: &[u8]) -> Result<Ihdr> {
    if data.len() != 13 {
        return Err(MediaError::invalid_data("PNG IHDR length must be 13 bytes"));
    }
    let header = Ihdr {
        width: u32::from_be_bytes([data[0], data[1], data[2], data[3]]),
        height: u32::from_be_bytes([data[4], data[5], data[6], data[7]]),
        bit_depth: data[8],
        color_type: data[9],
        compression_method: data[10],
        filter_method: data[11],
        interlace_method: data[12],
    };
    if header.width == 0 || header.height == 0 {
        return Err(MediaError::invalid_data("PNG dimensions must be non-zero"));
    }
    let pixels = u64::from(header.width)
        .checked_mul(u64::from(header.height))
        .ok_or_else(|| MediaError::overflow("PNG pixel count overflow"))?;
    if pixels > MAX_PIXELS {
        return Err(MediaError::unsupported(format!(
            "PNG image has {pixels} pixels; limit is {MAX_PIXELS}"
        )));
    }
    if header.bit_depth != 8 {
        return Err(MediaError::unsupported(format!(
            "PNG bit depth {} is not implemented yet",
            header.bit_depth
        )));
    }
    if !matches!(header.color_type, 0 | 2 | 3 | 4 | 6) {
        return Err(MediaError::unsupported(format!(
            "PNG color type {} is not implemented yet",
            header.color_type
        )));
    }
    if header.compression_method != 0 {
        return Err(MediaError::unsupported(
            "unsupported PNG compression method",
        ));
    }
    if header.filter_method != 0 {
        return Err(MediaError::unsupported("unsupported PNG filter method"));
    }
    if header.interlace_method != 0 {
        return Err(MediaError::unsupported(
            "Adam7-interlaced PNG is not implemented yet",
        ));
    }
    Ok(header)
}

fn decode_scanlines(
    header: Ihdr,
    compressed: &[u8],
    palette: Option<&[u8]>,
    transparency: Option<&[u8]>,
) -> Result<VideoFrame> {
    if header.color_type == 3 {
        return decode_indexed_scanlines(header, compressed, palette, transparency);
    }

    let (format, bytes_per_pixel) = match header.color_type {
        0 => (PixelFormat::Gray8, 1_usize),
        2 => (PixelFormat::Rgb24, 3_usize),
        4 => (PixelFormat::GrayAlpha8, 2_usize),
        6 => (PixelFormat::Rgba32, 4_usize),
        _ => return Err(MediaError::unsupported("unsupported PNG color type")),
    };
    let width = usize::try_from(header.width)
        .map_err(|_| MediaError::overflow("PNG width exceeds usize"))?;
    let height = usize::try_from(header.height)
        .map_err(|_| MediaError::overflow("PNG height exceeds usize"))?;
    let stride = width
        .checked_mul(bytes_per_pixel)
        .ok_or_else(|| MediaError::overflow("PNG row stride overflow"))?;
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

    let pixel_size = stride
        .checked_mul(height)
        .ok_or_else(|| MediaError::overflow("PNG pixel storage overflow"))?;
    let mut pixels = vec![0_u8; pixel_size];
    for row_index in 0..height {
        let encoded_start = row_index * encoded_row;
        let filter = filtered[encoded_start];
        let raw = &filtered[encoded_start + 1..encoded_start + encoded_row];
        let output_start = row_index * stride;
        let (before, current_and_after) = pixels.split_at_mut(output_start);
        let current = &mut current_and_after[..stride];
        let previous = if row_index == 0 {
            None
        } else {
            Some(&before[before.len() - stride..])
        };
        unfilter_row(filter, raw, current, previous, bytes_per_pixel)?;
    }

    VideoFrame::from_vec(header.width, header.height, format, pixels)
}

fn decode_indexed_scanlines(
    header: Ihdr,
    compressed: &[u8],
    palette: Option<&[u8]>,
    transparency: Option<&[u8]>,
) -> Result<VideoFrame> {
    let palette = palette.ok_or_else(|| MediaError::invalid_data("indexed PNG requires PLTE"))?;
    let palette_entries = palette.len() / 3;
    let width = usize::try_from(header.width)
        .map_err(|_| MediaError::overflow("PNG width exceeds usize"))?;
    let height = usize::try_from(header.height)
        .map_err(|_| MediaError::overflow("PNG height exceeds usize"))?;
    let encoded_row = width
        .checked_add(1)
        .ok_or_else(|| MediaError::overflow("PNG indexed row size overflow"))?;
    let expected_size = encoded_row
        .checked_mul(height)
        .ok_or_else(|| MediaError::overflow("PNG indexed decompressed size overflow"))?;
    let filtered = zlib::decompress(compressed, expected_size)?;
    if filtered.len() != expected_size {
        return Err(MediaError::invalid_data(format!(
            "PNG decompressed data has {} bytes but {expected_size} are required",
            filtered.len()
        )));
    }

    let index_count = width
        .checked_mul(height)
        .ok_or_else(|| MediaError::overflow("PNG indexed pixel count overflow"))?;
    let mut indices = vec![0_u8; index_count];
    for row_index in 0..height {
        let encoded_start = row_index * encoded_row;
        let filter = filtered[encoded_start];
        let raw = &filtered[encoded_start + 1..encoded_start + encoded_row];
        let output_start = row_index * width;
        let (before, current_and_after) = indices.split_at_mut(output_start);
        let current = &mut current_and_after[..width];
        let previous = if row_index == 0 {
            None
        } else {
            Some(&before[before.len() - width..])
        };
        unfilter_row(filter, raw, current, previous, 1)?;
    }

    let has_alpha = transparency.is_some();
    let format = if has_alpha {
        PixelFormat::Rgba32
    } else {
        PixelFormat::Rgb24
    };
    let channels = format.bytes_per_pixel();
    let output_len = index_count
        .checked_mul(channels)
        .ok_or_else(|| MediaError::overflow("PNG indexed expansion size overflow"))?;
    let mut output = Vec::with_capacity(output_len);
    for index in indices {
        let entry = usize::from(index);
        if entry >= palette_entries {
            return Err(MediaError::invalid_data(format!(
                "PNG palette index {entry} exceeds PLTE entry count {palette_entries}"
            )));
        }
        let offset = entry * 3;
        output.extend_from_slice(&palette[offset..offset + 3]);
        if let Some(alpha) = transparency {
            output.push(alpha.get(entry).copied().unwrap_or(255));
        }
    }

    VideoFrame::from_vec(header.width, header.height, format, output)
}

fn unfilter_row(
    filter: u8,
    raw: &[u8],
    output: &mut [u8],
    previous: Option<&[u8]>,
    bytes_per_pixel: usize,
) -> Result<()> {
    for index in 0..raw.len() {
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
        output[index] = raw[index].wrapping_add(predictor);
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

fn write_chunk(output: &mut Vec<u8>, chunk_type: [u8; 4], data: &[u8]) -> Result<()> {
    let length = u32::try_from(data.len())
        .map_err(|_| MediaError::unsupported("PNG chunk exceeds 4 GiB"))?;
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(&chunk_type);
    output.extend_from_slice(data);
    output.extend_from_slice(&chunk_crc(chunk_type, data).to_be_bytes());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_from_filtered(width: u32, height: u32, color_type: u8, filtered: &[u8]) -> Vec<u8> {
        let compressed = zlib::compress_stored(filtered).unwrap();
        let mut output = PNG_SIGNATURE.to_vec();
        let mut ihdr = [0_u8; 13];
        ihdr[0..4].copy_from_slice(&width.to_be_bytes());
        ihdr[4..8].copy_from_slice(&height.to_be_bytes());
        ihdr[8] = 8;
        ihdr[9] = color_type;
        write_chunk(&mut output, *b"IHDR", &ihdr).unwrap();
        write_chunk(&mut output, *b"IDAT", &compressed).unwrap();
        write_chunk(&mut output, *b"IEND", &[]).unwrap();
        output
    }

    #[test]
    fn grayscale_round_trip_is_exact() {
        let frame =
            VideoFrame::from_vec(3, 2, PixelFormat::Gray8, vec![0, 127, 255, 20, 30, 40]).unwrap();
        let encoded = encode_png(&frame).unwrap();
        assert_eq!(probe_png(&encoded), 100);
        assert_eq!(decode_png(&encoded).unwrap(), frame);
    }

    #[test]
    fn rgb_round_trip_is_exact() {
        let frame = VideoFrame::from_vec(
            2,
            2,
            PixelFormat::Rgb24,
            vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255],
        )
        .unwrap();
        assert_eq!(decode_png(&encode_png(&frame).unwrap()).unwrap(), frame);
    }

    #[test]
    fn all_five_png_filters_reconstruct_expected_rows() {
        let first = [10_u8, 20, 30, 40, 50];
        for filter in 0_u8..=4 {
            let second = [15_u8, 25, 35, 45, 55];
            let mut filtered = vec![0];
            filtered.extend_from_slice(&first);
            filtered.push(filter);
            for index in 0..second.len() {
                let left = if index > 0 { second[index - 1] } else { 0 };
                let up = first[index];
                let up_left = if index > 0 { first[index - 1] } else { 0 };
                let predictor = match filter {
                    0 => 0,
                    1 => left,
                    2 => up,
                    3 => ((u16::from(left) + u16::from(up)) / 2) as u8,
                    4 => paeth(left, up, up_left),
                    _ => unreachable!(),
                };
                filtered.push(second[index].wrapping_sub(predictor));
            }
            let png = png_from_filtered(5, 2, 0, &filtered);
            let decoded = decode_png(&png).unwrap();
            assert_eq!(decoded.data.as_slice(), &[first, second].concat());
        }
    }

    #[test]
    fn corrupt_crc_is_rejected() {
        let frame = VideoFrame::from_vec(1, 1, PixelFormat::Gray8, vec![42]).unwrap();
        let mut encoded = encode_png(&frame).unwrap();
        encoded[29] ^= 1;
        assert!(decode_png(&encoded).is_err());
    }

    #[test]
    fn unsupported_ihdr_features_are_rejected() {
        for (bit_depth, color_type, interlace) in [(16, 0, 0), (8, 6, 0), (8, 2, 1)] {
            let mut png = png_from_filtered(1, 1, 0, &[0, 0]);
            png[24] = bit_depth;
            png[25] = color_type;
            png[28] = interlace;
            let crc = chunk_crc(*b"IHDR", &png[16..29]);
            png[29..33].copy_from_slice(&crc.to_be_bytes());
            assert!(decode_png(&png).is_err());
        }
    }

    #[test]
    fn unknown_critical_chunk_and_nonconsecutive_idat_are_rejected() {
        let compressed = zlib::compress_stored(&[0, 1]).unwrap();
        let mut critical = PNG_SIGNATURE.to_vec();
        let mut ihdr = [0_u8; 13];
        ihdr[0..4].copy_from_slice(&1_u32.to_be_bytes());
        ihdr[4..8].copy_from_slice(&1_u32.to_be_bytes());
        ihdr[8] = 8;
        ihdr[9] = 0;
        write_chunk(&mut critical, *b"IHDR", &ihdr).unwrap();
        write_chunk(&mut critical, *b"ABCD", &[]).unwrap();
        write_chunk(&mut critical, *b"IDAT", &compressed).unwrap();
        write_chunk(&mut critical, *b"IEND", &[]).unwrap();
        assert!(decode_png(&critical).is_err());

        let mut split = PNG_SIGNATURE.to_vec();
        write_chunk(&mut split, *b"IHDR", &ihdr).unwrap();
        let midpoint = compressed.len() / 2;
        write_chunk(&mut split, *b"IDAT", &compressed[..midpoint]).unwrap();
        write_chunk(&mut split, *b"tEXt", b"x\0y").unwrap();
        write_chunk(&mut split, *b"IDAT", &compressed[midpoint..]).unwrap();
        write_chunk(&mut split, *b"IEND", &[]).unwrap();
        assert!(decode_png(&split).is_err());
    }

    #[test]
    fn invalid_filter_and_wrong_decompressed_size_are_rejected() {
        assert!(decode_png(&png_from_filtered(1, 1, 0, &[5, 1])).is_err());
        assert!(decode_png(&png_from_filtered(2, 1, 0, &[0, 1])).is_err());
    }
}
