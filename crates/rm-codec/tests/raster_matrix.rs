use rm_codec::bmp::{decode_bmp, encode_bmp};
use rm_codec::tga::{decode_tga, encode_tga, encode_tga_with_rle};
use rm_core::video::{PixelFormat, VideoFrame};

fn rgb(width: u32, height: u32, data: Vec<u8>) -> VideoFrame {
    VideoFrame::from_vec(width, height, PixelFormat::Rgb24, data).unwrap()
}

#[test]
fn bmp_row_padding_round_trips_widths_one_through_five() {
    for width in 1..=5_u32 {
        let mut pixels = Vec::new();
        for x in 0..width {
            pixels.extend_from_slice(&[
                u8::try_from(x * 31).unwrap(),
                u8::try_from(x * 17).unwrap(),
                u8::try_from(x * 7).unwrap(),
            ]);
        }
        let frame = rgb(width, 1, pixels);
        let encoded = encode_bmp(&frame).unwrap();
        assert_eq!(decode_bmp(&encoded).unwrap(), frame, "width={width}");
        assert_eq!((encoded.len() - 54) % 4, 0);
    }
}

#[test]
fn bmp_multiline_round_trip_preserves_logical_top_left_order() {
    let frame = rgb(
        3,
        3,
        vec![
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
            25, 26, 27,
        ],
    );
    assert_eq!(decode_bmp(&encode_bmp(&frame).unwrap()).unwrap(), frame);
}

#[test]
fn bmp_rejects_zero_or_unrepresentable_dimensions_and_unsupported_depths() {
    let frame = rgb(1, 1, vec![1, 2, 3]);
    let valid = encode_bmp(&frame).unwrap();

    let mut zero_width = valid.clone();
    zero_width[18..22].copy_from_slice(&0_i32.to_le_bytes());
    assert!(decode_bmp(&zero_width).is_err());

    let mut zero_height = valid.clone();
    zero_height[22..26].copy_from_slice(&0_i32.to_le_bytes());
    assert!(decode_bmp(&zero_height).is_err());

    let mut min_height = valid.clone();
    min_height[22..26].copy_from_slice(&i32::MIN.to_le_bytes());
    assert!(decode_bmp(&min_height).is_err());

    let mut depth16 = valid.clone();
    depth16[28..30].copy_from_slice(&16_u16.to_le_bytes());
    assert!(decode_bmp(&depth16).is_err());
}

#[test]
fn bmp_rejects_declared_file_smaller_than_raster() {
    let frame = rgb(2, 2, vec![1; 12]);
    let mut encoded = encode_bmp(&frame).unwrap();
    encoded[2..6].copy_from_slice(&54_u32.to_le_bytes());
    assert!(decode_bmp(&encoded).is_err());
}

#[test]
fn tga_rle_splits_runs_longer_than_128_pixels() {
    let mut data = Vec::with_capacity(300 * 3);
    for _ in 0..300 {
        data.extend_from_slice(&[9, 8, 7]);
    }
    let frame = rgb(300, 1, data);
    let encoded = encode_tga_with_rle(&frame, true).unwrap();
    assert_eq!(decode_tga(&encoded).unwrap(), frame);
    assert!(encoded.len() < 40);
}

#[test]
fn tga_rle_splits_raw_packets_longer_than_128_pixels() {
    let mut data = Vec::with_capacity(260 * 3);
    for x in 0..260_u16 {
        data.extend_from_slice(&[
            u8::try_from(x & 0xff).unwrap(),
            u8::try_from((x * 3) & 0xff).unwrap(),
            u8::try_from((x * 7) & 0xff).unwrap(),
        ]);
    }
    let frame = rgb(260, 1, data);
    let encoded = encode_tga_with_rle(&frame, true).unwrap();
    assert_eq!(decode_tga(&encoded).unwrap(), frame);
}

#[test]
fn tga_image_id_is_skipped_before_raster() {
    let frame = rgb(1, 1, vec![5, 6, 7]);
    let encoded = encode_tga(&frame).unwrap();
    let mut with_id = encoded[..18].to_vec();
    with_id[0] = 4;
    with_id.extend_from_slice(b"TEST");
    with_id.extend_from_slice(&encoded[18..]);
    assert_eq!(decode_tga(&with_id).unwrap(), frame);
}

#[test]
fn tga_32_bit_alpha_is_ignored_but_rgb_is_preserved() {
    let mut file = vec![0_u8; 18];
    file[2] = 2;
    file[12..14].copy_from_slice(&1_u16.to_le_bytes());
    file[14..16].copy_from_slice(&1_u16.to_le_bytes());
    file[16] = 32;
    file[17] = 0x28;
    file.extend_from_slice(&[30, 20, 10, 99]);
    let frame = decode_tga(&file).unwrap();
    assert_eq!(frame.format, PixelFormat::Rgb24);
    assert_eq!(frame.data.as_slice(), &[10, 20, 30]);
}

#[test]
fn tga_grayscale_rle_round_trip_handles_mixed_packets() {
    let frame =
        VideoFrame::from_vec(8, 1, PixelFormat::Gray8, vec![1, 1, 1, 2, 3, 4, 4, 4]).unwrap();
    let encoded = encode_tga_with_rle(&frame, true).unwrap();
    assert_eq!(encoded[2], 11);
    assert_eq!(decode_tga(&encoded).unwrap(), frame);
}

#[test]
fn tga_rejects_zero_dimensions_and_truncated_id() {
    let frame = rgb(1, 1, vec![1, 2, 3]);
    let valid = encode_tga(&frame).unwrap();

    let mut zero = valid.clone();
    zero[12..14].copy_from_slice(&0_u16.to_le_bytes());
    assert!(decode_tga(&zero).is_err());

    let mut bad_id = valid[..18].to_vec();
    bad_id[0] = 10;
    assert!(decode_tga(&bad_id).is_err());
}
