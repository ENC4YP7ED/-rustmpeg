use rm_codec::png::decode_png;
use rm_compress::{crc32, zlib};
use rm_core::video::PixelFormat;

const PNG_SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];
const PASSES: [(usize, usize, usize, usize); 7] = [
    (0, 0, 8, 8),
    (4, 0, 8, 8),
    (0, 4, 4, 8),
    (2, 0, 4, 4),
    (0, 2, 2, 4),
    (1, 0, 2, 2),
    (0, 1, 1, 2),
];
const FORMATS: [(u8, u8); 15] = [
    (0, 1),
    (0, 2),
    (0, 4),
    (0, 8),
    (0, 16),
    (2, 8),
    (2, 16),
    (3, 1),
    (3, 2),
    (3, 4),
    (3, 8),
    (4, 8),
    (4, 16),
    (6, 8),
    (6, 16),
];

#[test]
fn upstream_pngsuite_adam7_matches_non_interlaced_reference() {
    let interlaced = include_bytes!("../testdata/pngsuite/basi0g01.png");
    let reference = include_bytes!("../testdata/pngsuite/basn0g01-reference.png");
    let actual = decode_png(interlaced).expect("upstream Adam7 fixture must decode");
    let expected = decode_png(reference).expect("upstream baseline fixture must decode");
    assert_eq!((actual.width, actual.height), (expected.width, expected.height));
    assert_eq!(actual.format, expected.format);
    assert_eq!(actual.data.as_slice(), expected.data.as_slice());
}

#[test]
fn every_legal_basic_png_format_decodes_through_adam7() {
    for &(width, height) in &[(1_u32, 1_u32), (5, 7), (9, 6)] {
        for &(color_type, bit_depth) in &FORMATS {
            let fixture = make_adam7_png(width, height, color_type, bit_depth, None);
            let frame = decode_png(&fixture).unwrap_or_else(|error| {
                panic!(
                    "Adam7 decode failed for {width}x{height} color_type={color_type} bit_depth={bit_depth}: {error}"
                )
            });
            let (expected_format, expected) = expected_pixels(width, height, color_type, bit_depth);
            assert_eq!(frame.format, expected_format);
            assert_eq!(frame.data.as_slice(), expected.as_slice());
        }
    }
}

#[test]
fn all_png_filters_work_inside_adam7_passes() {
    let fixture = make_adam7_png(17, 13, 6, 8, None);
    let frame = decode_png(&fixture).expect("filtered Adam7 RGBA fixture must decode");
    let (_, expected) = expected_pixels(17, 13, 6, 8);
    assert_eq!(frame.data.as_slice(), expected.as_slice());
}

#[test]
fn malformed_adam7_filter_and_pass_size_are_rejected() {
    let invalid_filter = make_adam7_png(8, 8, 2, 8, Some(5));
    assert!(decode_png(&invalid_filter).is_err());

    let mut scanlines = make_scanlines(8, 8, 2, 8, None);
    scanlines.pop();
    let truncated = build_png(8, 8, 2, 8, &scanlines);
    assert!(decode_png(&truncated).is_err());
}

fn make_adam7_png(
    width: u32,
    height: u32,
    color_type: u8,
    bit_depth: u8,
    first_filter_override: Option<u8>,
) -> Vec<u8> {
    let scanlines = make_scanlines(
        width,
        height,
        color_type,
        bit_depth,
        first_filter_override,
    );
    build_png(width, height, color_type, bit_depth, &scanlines)
}

fn make_scanlines(
    width: u32,
    height: u32,
    color_type: u8,
    bit_depth: u8,
    first_filter_override: Option<u8>,
) -> Vec<u8> {
    let width = usize::try_from(width).unwrap();
    let height = usize::try_from(height).unwrap();
    let channels = channels(color_type);
    let bits_per_pixel = channels * usize::from(bit_depth);
    let filter_bpp = bits_per_pixel.div_ceil(8).max(1);
    let mut output = Vec::new();
    let mut global_row = 0_usize;

    for &(x0, y0, dx, dy) in &PASSES {
        let pass_width = extent(width, x0, dx);
        let pass_height = extent(height, y0, dy);
        if pass_width == 0 || pass_height == 0 {
            continue;
        }
        let mut previous = vec![0_u8; (pass_width * bits_per_pixel).div_ceil(8)];
        for py in 0..pass_height {
            let y = y0 + py * dy;
            let raw = make_raw_pass_row(pass_width, x0, dx, y, color_type, bit_depth);
            let filter = if global_row == 0 {
                first_filter_override.unwrap_or(0)
            } else {
                u8::try_from(global_row % 5).unwrap()
            };
            output.push(filter);
            if filter <= 4 {
                output.extend_from_slice(&filter_row(filter, &raw, &previous, filter_bpp));
            } else {
                output.extend_from_slice(&raw);
            }
            previous = raw;
            global_row += 1;
        }
    }
    output
}

fn make_raw_pass_row(
    pass_width: usize,
    x0: usize,
    dx: usize,
    y: usize,
    color_type: u8,
    bit_depth: u8,
) -> Vec<u8> {
    let channels = channels(color_type);
    if bit_depth < 8 {
        let mut row = vec![0_u8; (pass_width * usize::from(bit_depth)).div_ceil(8)];
        for px in 0..pass_width {
            let x = x0 + px * dx;
            let sample = sample_value(x, y, 0, bit_depth);
            let bit = px * usize::from(bit_depth);
            let shift = 8 - usize::from(bit_depth) - (bit % 8);
            row[bit / 8] |= u8::try_from(sample).unwrap() << shift;
        }
        return row;
    }

    let mut row = Vec::new();
    for px in 0..pass_width {
        let x = x0 + px * dx;
        for channel in 0..channels {
            let sample = sample_value(x, y, channel, bit_depth);
            if bit_depth == 8 {
                row.push(u8::try_from(sample).unwrap());
            } else {
                row.extend_from_slice(&sample.to_be_bytes());
            }
        }
    }
    row
}

fn filter_row(filter: u8, raw: &[u8], previous: &[u8], bpp: usize) -> Vec<u8> {
    raw.iter()
        .enumerate()
        .map(|(index, &value)| {
            let left = if index >= bpp { raw[index - bpp] } else { 0 };
            let up = previous[index];
            let up_left = if index >= bpp { previous[index - bpp] } else { 0 };
            let predictor = match filter {
                0 => 0,
                1 => left,
                2 => up,
                3 => u8::try_from(u16::midpoint(u16::from(left), u16::from(up))).unwrap(),
                4 => paeth(left, up, up_left),
                _ => 0,
            };
            value.wrapping_sub(predictor)
        })
        .collect()
}

fn expected_pixels(width: u32, height: u32, color_type: u8, bit_depth: u8) -> (PixelFormat, Vec<u8>) {
    let format = match color_type {
        0 => PixelFormat::Gray8,
        2 | 3 => PixelFormat::Rgb24,
        4 => PixelFormat::GrayAlpha8,
        6 => PixelFormat::Rgba32,
        _ => unreachable!(),
    };
    let mut output = Vec::new();
    for y in 0..usize::try_from(height).unwrap() {
        for x in 0..usize::try_from(width).unwrap() {
            match color_type {
                0 => output.push(scale(sample_value(x, y, 0, bit_depth), bit_depth)),
                2 => {
                    for channel in 0..3 {
                        output.push(scale(sample_value(x, y, channel, bit_depth), bit_depth));
                    }
                }
                3 => {
                    let index = usize::from(sample_value(x, y, 0, bit_depth));
                    output.extend_from_slice(&palette_entry(index));
                }
                4 => {
                    output.push(scale(sample_value(x, y, 0, bit_depth), bit_depth));
                    output.push(scale(sample_value(x, y, 1, bit_depth), bit_depth));
                }
                6 => {
                    for channel in 0..4 {
                        output.push(scale(sample_value(x, y, channel, bit_depth), bit_depth));
                    }
                }
                _ => unreachable!(),
            }
        }
    }
    (format, output)
}

fn build_png(width: u32, height: u32, color_type: u8, bit_depth: u8, scanlines: &[u8]) -> Vec<u8> {
    let mut output = PNG_SIGNATURE.to_vec();
    let mut ihdr = [0_u8; 13];
    ihdr[0..4].copy_from_slice(&width.to_be_bytes());
    ihdr[4..8].copy_from_slice(&height.to_be_bytes());
    ihdr[8] = bit_depth;
    ihdr[9] = color_type;
    ihdr[12] = 1;
    push_chunk(&mut output, *b"IHDR", &ihdr);
    if color_type == 3 {
        let entries = 1_usize << bit_depth;
        let mut palette = Vec::with_capacity(entries * 3);
        for index in 0..entries {
            palette.extend_from_slice(&palette_entry(index));
        }
        push_chunk(&mut output, *b"PLTE", &palette);
    }
    let compressed = zlib::compress_stored(scanlines).unwrap();
    push_chunk(&mut output, *b"IDAT", &compressed);
    push_chunk(&mut output, *b"IEND", &[]);
    output
}

fn push_chunk(output: &mut Vec<u8>, kind: [u8; 4], data: &[u8]) {
    output.extend_from_slice(&u32::try_from(data.len()).unwrap().to_be_bytes());
    output.extend_from_slice(&kind);
    output.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(&kind);
    crc_input.extend_from_slice(data);
    output.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

fn sample_value(x: usize, y: usize, channel: usize, bit_depth: u8) -> u16 {
    let max = if bit_depth == 16 {
        u32::from(u16::MAX)
    } else {
        (1_u32 << bit_depth) - 1
    };
    let value = (u32::try_from(x).unwrap() * 17
        + u32::try_from(y).unwrap() * 31
        + u32::try_from(channel).unwrap() * 47)
        % (max + 1);
    u16::try_from(value).unwrap()
}

fn scale(sample: u16, bit_depth: u8) -> u8 {
    let max = if bit_depth == 16 {
        65_535_u32
    } else {
        (1_u32 << bit_depth) - 1
    };
    u8::try_from((u32::from(sample) * 255 + max / 2) / max).unwrap()
}

fn palette_entry(index: usize) -> [u8; 3] {
    let value = u8::try_from(index & 0xff).unwrap();
    [value, value.wrapping_mul(37), value.wrapping_mul(91)]
}

fn channels(color_type: u8) -> usize {
    match color_type {
        0 | 3 => 1,
        2 => 3,
        4 => 2,
        6 => 4,
        _ => unreachable!(),
    }
}

fn extent(total: usize, start: usize, step: usize) -> usize {
    if total <= start {
        0
    } else {
        (total - start).div_ceil(step)
    }
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
