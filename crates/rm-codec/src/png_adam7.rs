use rm_compress::{crc32, zlib};
use rm_core::video::{PixelFormat, VideoFrame};
use rm_core::{MediaError, Result};

const PNG_SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];
const MAX_PIXELS: u64 = 268_435_456;
const PASSES: [(usize, usize, usize, usize); 7] = [
    (0, 0, 8, 8),
    (4, 0, 8, 8),
    (0, 4, 4, 8),
    (2, 0, 4, 4),
    (0, 2, 2, 4),
    (1, 0, 2, 2),
    (0, 1, 1, 2),
];

#[derive(Debug, Clone, Copy)]
struct Header {
    width: u32,
    height: u32,
    bit_depth: u8,
    color_type: u8,
}

struct Parsed<'a> {
    header: Header,
    palette: Option<&'a [u8]>,
    transparency: Option<&'a [u8]>,
    idat: Vec<u8>,
}

pub(super) fn decode_png_adam7(bytes: &[u8]) -> Result<VideoFrame> {
    let parsed = parse(bytes)?;
    decode_passes(parsed)
}

fn parse(bytes: &[u8]) -> Result<Parsed<'_>> {
    if !bytes.starts_with(&PNG_SIGNATURE) {
        return Err(MediaError::invalid_data("missing PNG signature"));
    }

    let mut position = PNG_SIGNATURE.len();
    let mut header = None;
    let mut palette = None;
    let mut transparency = None;
    let mut idat = Vec::new();
    let mut saw_idat = false;
    let mut idat_ended = false;
    let mut saw_iend = false;

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
                if header.is_some() || saw_idat || position - (length + 12) != PNG_SIGNATURE.len() {
                    return Err(MediaError::invalid_data(
                        "PNG IHDR must appear exactly once and first",
                    ));
                }
                header = Some(parse_header(data)?);
            }
            b"PLTE" => {
                let png = header
                    .ok_or_else(|| MediaError::invalid_data("PNG PLTE appeared before IHDR"))?;
                if saw_idat {
                    return Err(MediaError::invalid_data("PNG PLTE appeared after IDAT"));
                }
                if palette.is_some() {
                    return Err(MediaError::invalid_data(
                        "PNG contains multiple PLTE chunks",
                    ));
                }
                if matches!(png.color_type, 0 | 4) {
                    return Err(MediaError::invalid_data(
                        "grayscale PNG must not contain a PLTE chunk",
                    ));
                }
                if data.is_empty() || !data.len().is_multiple_of(3) || data.len() > 768 {
                    return Err(MediaError::invalid_data("invalid PNG PLTE length"));
                }
                if png.color_type == 3 && data.len() / 3 > (1_usize << png.bit_depth) {
                    return Err(MediaError::invalid_data(
                        "PNG PLTE has more entries than indexed bit depth permits",
                    ));
                }
                palette = Some(data);
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
                    3 => {
                        let entries = palette
                            .ok_or_else(|| {
                                MediaError::invalid_data("indexed PNG tRNS requires PLTE")
                            })?
                            .len()
                            / 3;
                        if data.len() > entries {
                            return Err(MediaError::invalid_data(
                                "indexed PNG tRNS exceeds PLTE entry count",
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
                transparency = Some(data);
            }
            b"IDAT" => {
                let png = header
                    .ok_or_else(|| MediaError::invalid_data("PNG IDAT appeared before IHDR"))?;
                if png.color_type == 3 && palette.is_none() {
                    return Err(MediaError::invalid_data(
                        "indexed PNG requires PLTE before IDAT",
                    ));
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
    Ok(Parsed {
        header: header.ok_or_else(|| MediaError::invalid_data("PNG has no IHDR chunk"))?,
        palette,
        transparency,
        idat,
    })
}

fn parse_header(data: &[u8]) -> Result<Header> {
    if data.len() != 13 {
        return Err(MediaError::invalid_data("PNG IHDR length must be 13 bytes"));
    }
    let width = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
    let height = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    let bit_depth = data[8];
    let color_type = data[9];
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
    let valid_depth = match color_type {
        0 => matches!(bit_depth, 1 | 2 | 4 | 8 | 16),
        2 => matches!(bit_depth, 8 | 16),
        3 => matches!(bit_depth, 1 | 2 | 4 | 8),
        4 | 6 => matches!(bit_depth, 8 | 16),
        _ => false,
    };
    if !valid_depth {
        return Err(MediaError::unsupported(format!(
            "PNG bit depth {bit_depth} is invalid for color type {color_type}"
        )));
    }
    if data[10] != 0 {
        return Err(MediaError::unsupported(
            "unsupported PNG compression method",
        ));
    }
    if data[11] != 0 {
        return Err(MediaError::unsupported("unsupported PNG filter method"));
    }
    if data[12] != 1 {
        return Err(MediaError::unsupported(
            "Adam7 decoder received a non-interlaced PNG",
        ));
    }
    Ok(Header {
        width,
        height,
        bit_depth,
        color_type,
    })
}

fn decode_passes(parsed: Parsed<'_>) -> Result<VideoFrame> {
    let header = parsed.header;
    let width = usize::try_from(header.width)
        .map_err(|_| MediaError::overflow("PNG width exceeds usize"))?;
    let height = usize::try_from(header.height)
        .map_err(|_| MediaError::overflow("PNG height exceeds usize"))?;
    let channels = channels(header.color_type)?;
    let bits_per_pixel = channels
        .checked_mul(usize::from(header.bit_depth))
        .ok_or_else(|| MediaError::overflow("PNG bits-per-pixel overflow"))?;
    let filter_bpp = bits_per_pixel.div_ceil(8).max(1);

    let mut inflated_len = 0_usize;
    for &(x0, y0, dx, dy) in &PASSES {
        let pass_width = pass_extent(width, x0, dx);
        let pass_height = pass_extent(height, y0, dy);
        if pass_width == 0 || pass_height == 0 {
            continue;
        }
        let row_bytes = packed_row_bytes(pass_width, bits_per_pixel)?;
        inflated_len = inflated_len
            .checked_add(
                row_bytes
                    .checked_add(1)
                    .and_then(|row| row.checked_mul(pass_height))
                    .ok_or_else(|| MediaError::overflow("PNG Adam7 pass size overflow"))?,
            )
            .ok_or_else(|| MediaError::overflow("PNG Adam7 inflated size overflow"))?;
    }

    let inflated = zlib::decompress(&parsed.idat, inflated_len)?;
    if inflated.len() != inflated_len {
        return Err(MediaError::invalid_data(format!(
            "PNG Adam7 data has {} bytes but {inflated_len} are required",
            inflated.len()
        )));
    }

    let format = output_format(header.color_type, parsed.transparency.is_some());
    let output_bpp = format.bytes_per_pixel();
    let output_len = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(output_bpp))
        .ok_or_else(|| MediaError::overflow("PNG Adam7 output size overflow"))?;
    let mut output = vec![0_u8; output_len];
    let mut offset = 0_usize;

    for &(x0, y0, dx, dy) in &PASSES {
        let pass_width = pass_extent(width, x0, dx);
        let pass_height = pass_extent(height, y0, dy);
        if pass_width == 0 || pass_height == 0 {
            continue;
        }
        let row_bytes = packed_row_bytes(pass_width, bits_per_pixel)?;
        let mut previous = vec![0_u8; row_bytes];
        let mut current = vec![0_u8; row_bytes];

        for pass_y in 0..pass_height {
            let filter = *inflated
                .get(offset)
                .ok_or_else(|| MediaError::eof("truncated PNG Adam7 filter byte"))?;
            offset += 1;
            let end = offset
                .checked_add(row_bytes)
                .ok_or_else(|| MediaError::overflow("PNG Adam7 row range overflow"))?;
            let source = inflated
                .get(offset..end)
                .ok_or_else(|| MediaError::eof("truncated PNG Adam7 row"))?;
            offset = end;
            unfilter_row(filter, source, &mut current, &previous, filter_bpp)?;

            let image_y = y0 + pass_y * dy;
            for pass_x in 0..pass_width {
                let image_x = x0 + pass_x * dx;
                let samples = read_samples(&current, pass_x, header.bit_depth, channels)?;
                write_pixel(
                    &mut output,
                    width,
                    image_x,
                    image_y,
                    output_bpp,
                    header,
                    samples,
                    parsed.palette,
                    parsed.transparency,
                )?;
            }
            previous.copy_from_slice(&current);
        }
    }

    if offset != inflated.len() {
        return Err(MediaError::invalid_data(
            "PNG Adam7 stream contains trailing decompressed bytes",
        ));
    }
    VideoFrame::from_vec(header.width, header.height, format, output)
}

fn output_format(color_type: u8, has_transparency: bool) -> PixelFormat {
    match color_type {
        0 if has_transparency => PixelFormat::GrayAlpha8,
        0 => PixelFormat::Gray8,
        2 if has_transparency => PixelFormat::Rgba32,
        2 => PixelFormat::Rgb24,
        3 if has_transparency => PixelFormat::Rgba32,
        3 => PixelFormat::Rgb24,
        4 => PixelFormat::GrayAlpha8,
        6 => PixelFormat::Rgba32,
        _ => unreachable!(),
    }
}

fn channels(color_type: u8) -> Result<usize> {
    match color_type {
        0 | 3 => Ok(1),
        2 => Ok(3),
        4 => Ok(2),
        6 => Ok(4),
        _ => Err(MediaError::unsupported("unsupported PNG color type")),
    }
}

fn pass_extent(total: usize, start: usize, step: usize) -> usize {
    if total <= start {
        0
    } else {
        (total - start).div_ceil(step)
    }
}

fn packed_row_bytes(width: usize, bits_per_pixel: usize) -> Result<usize> {
    width
        .checked_mul(bits_per_pixel)
        .and_then(|bits| bits.checked_add(7))
        .map(|bits| bits / 8)
        .ok_or_else(|| MediaError::overflow("PNG packed row size overflow"))
}

fn read_samples(row: &[u8], pixel: usize, bit_depth: u8, channels: usize) -> Result<[u16; 4]> {
    let mut out = [0_u16; 4];
    if bit_depth < 8 {
        let bit = pixel
            .checked_mul(usize::from(bit_depth))
            .ok_or_else(|| MediaError::overflow("PNG packed sample offset overflow"))?;
        let byte = *row
            .get(bit / 8)
            .ok_or_else(|| MediaError::eof("truncated PNG packed Adam7 sample"))?;
        let shift = 8 - usize::from(bit_depth) - (bit % 8);
        let mask = (1_u16 << bit_depth) - 1;
        out[0] = (u16::from(byte) >> shift) & mask;
        return Ok(out);
    }

    let bytes_per_sample = usize::from(bit_depth / 8);
    let pixel_bytes = channels
        .checked_mul(bytes_per_sample)
        .ok_or_else(|| MediaError::overflow("PNG pixel byte width overflow"))?;
    let start = pixel
        .checked_mul(pixel_bytes)
        .ok_or_else(|| MediaError::overflow("PNG sample offset overflow"))?;
    for (channel, slot) in out.iter_mut().enumerate().take(channels) {
        let sample_offset = start + channel * bytes_per_sample;
        *slot = if bit_depth == 8 {
            u16::from(
                *row.get(sample_offset)
                    .ok_or_else(|| MediaError::eof("truncated PNG Adam7 sample"))?,
            )
        } else {
            let pair = row
                .get(sample_offset..sample_offset + 2)
                .ok_or_else(|| MediaError::eof("truncated PNG 16-bit Adam7 sample"))?;
            u16::from_be_bytes([pair[0], pair[1]])
        };
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn write_pixel(
    output: &mut [u8],
    width: usize,
    x: usize,
    y: usize,
    output_bpp: usize,
    header: Header,
    samples: [u16; 4],
    palette: Option<&[u8]>,
    transparency: Option<&[u8]>,
) -> Result<()> {
    let pixel_index = y
        .checked_mul(width)
        .and_then(|row| row.checked_add(x))
        .ok_or_else(|| MediaError::overflow("PNG Adam7 pixel index overflow"))?;
    let start = pixel_index
        .checked_mul(output_bpp)
        .ok_or_else(|| MediaError::overflow("PNG Adam7 output offset overflow"))?;
    let target = output
        .get_mut(start..start + output_bpp)
        .ok_or_else(|| MediaError::overflow("PNG Adam7 output range overflow"))?;

    match header.color_type {
        0 => {
            target[0] = scale_sample(samples[0], header.bit_depth)?;
            if target.len() == 2 {
                let key = transparency
                    .and_then(|data| data.get(..2))
                    .map(|data| u16::from_be_bytes([data[0], data[1]]))
                    .ok_or_else(|| MediaError::invalid_data("missing grayscale tRNS key"))?;
                target[1] = if samples[0] == key { 0 } else { 255 };
            }
        }
        2 => {
            target[0] = scale_sample(samples[0], header.bit_depth)?;
            target[1] = scale_sample(samples[1], header.bit_depth)?;
            target[2] = scale_sample(samples[2], header.bit_depth)?;
            if target.len() == 4 {
                let key = transparency
                    .and_then(|data| data.get(..6))
                    .ok_or_else(|| MediaError::invalid_data("missing truecolor tRNS key"))?;
                let matches = samples[0] == u16::from_be_bytes([key[0], key[1]])
                    && samples[1] == u16::from_be_bytes([key[2], key[3]])
                    && samples[2] == u16::from_be_bytes([key[4], key[5]]);
                target[3] = if matches { 0 } else { 255 };
            }
        }
        3 => {
            let palette =
                palette.ok_or_else(|| MediaError::invalid_data("indexed PNG requires PLTE"))?;
            let index = usize::from(samples[0]);
            let entry = index
                .checked_mul(3)
                .ok_or_else(|| MediaError::overflow("PNG palette offset overflow"))?;
            let rgb = palette
                .get(entry..entry + 3)
                .ok_or_else(|| MediaError::invalid_data("PNG palette index exceeds PLTE"))?;
            target[..3].copy_from_slice(rgb);
            if target.len() == 4 {
                target[3] = transparency
                    .and_then(|alpha| alpha.get(index))
                    .copied()
                    .unwrap_or(255);
            }
        }
        4 => {
            target[0] = scale_sample(samples[0], header.bit_depth)?;
            target[1] = scale_sample(samples[1], header.bit_depth)?;
        }
        6 => {
            target[0] = scale_sample(samples[0], header.bit_depth)?;
            target[1] = scale_sample(samples[1], header.bit_depth)?;
            target[2] = scale_sample(samples[2], header.bit_depth)?;
            target[3] = scale_sample(samples[3], header.bit_depth)?;
        }
        _ => unreachable!(),
    }
    Ok(())
}

fn scale_sample(sample: u16, bit_depth: u8) -> Result<u8> {
    match bit_depth {
        8 => u8::try_from(sample).map_err(|_| MediaError::overflow("PNG 8-bit sample exceeds u8")),
        16 => {
            let scaled = (u32::from(sample) * 255 + 32_767) / 65_535;
            u8::try_from(scaled).map_err(|_| MediaError::overflow("PNG 16-bit scaling exceeds u8"))
        }
        1 | 2 | 4 => {
            let max = (1_u32 << bit_depth) - 1;
            let scaled = (u32::from(sample) * 255 + max / 2) / max;
            u8::try_from(scaled)
                .map_err(|_| MediaError::overflow("PNG packed sample scaling exceeds u8"))
        }
        _ => Err(MediaError::unsupported("unsupported PNG bit depth")),
    }
}

fn unfilter_row(
    filter: u8,
    source: &[u8],
    output: &mut [u8],
    previous: &[u8],
    bytes_per_pixel: usize,
) -> Result<()> {
    if source.len() != output.len() || previous.len() != output.len() {
        return Err(MediaError::invalid_data("PNG Adam7 row length mismatch"));
    }
    if filter > 4 {
        return Err(MediaError::invalid_data(format!(
            "unsupported PNG filter type {filter}"
        )));
    }
    for index in 0..source.len() {
        let left = if index >= bytes_per_pixel {
            output[index - bytes_per_pixel]
        } else {
            0
        };
        let up = previous[index];
        let up_left = if index >= bytes_per_pixel {
            previous[index - bytes_per_pixel]
        } else {
            0
        };
        output[index] = match filter {
            0 => source[index],
            1 => source[index].wrapping_add(left),
            2 => source[index].wrapping_add(up),
            3 => source[index].wrapping_add(u16::midpoint(u16::from(left), u16::from(up)) as u8),
            4 => source[index].wrapping_add(paeth(left, up, up_left)),
            _ => unreachable!(),
        };
    }
    Ok(())
}

fn paeth(left: u8, up: u8, up_left: u8) -> u8 {
    let left = i32::from(left);
    let up = i32::from(up);
    let up_left = i32::from(up_left);
    let prediction = left + up - up_left;
    let left_distance = (prediction - left).abs();
    let up_distance = (prediction - up).abs();
    let diagonal_distance = (prediction - up_left).abs();
    if left_distance <= up_distance && left_distance <= diagonal_distance {
        u8::try_from(left).expect("left originated from u8")
    } else if up_distance <= diagonal_distance {
        u8::try_from(up).expect("up originated from u8")
    } else {
        u8::try_from(up_left).expect("up-left originated from u8")
    }
}

fn validate_chunk_type(chunk_type: [u8; 4]) -> Result<()> {
    if chunk_type.iter().all(u8::is_ascii_alphabetic) {
        Ok(())
    } else {
        Err(MediaError::invalid_data("PNG chunk type is not alphabetic"))
    }
}

fn is_critical(chunk_type: [u8; 4]) -> bool {
    chunk_type[0].is_ascii_uppercase()
}

fn chunk_crc(chunk_type: [u8; 4], data: &[u8]) -> u32 {
    let mut bytes = Vec::with_capacity(4 + data.len());
    bytes.extend_from_slice(&chunk_type);
    bytes.extend_from_slice(data);
    crc32(&bytes)
}

fn chunk_name(chunk_type: [u8; 4]) -> String {
    String::from_utf8_lossy(&chunk_type).into_owned()
}
