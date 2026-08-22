use rm_codec::bmp::{decode_bmp, encode_bmp};
use rm_codec::tga::{decode_tga, encode_tga, encode_tga_with_rle};
use rm_core::video::{PixelFormat, VideoFrame};

fn rgb(width: u32, height: u32, data: Vec<u8>) -> VideoFrame {
    VideoFrame::from_vec(width, height, PixelFormat::Rgb24, data).unwrap()
}

#[test]
fn bmp_row_padding_round_trips_widths_one_through_five() {
    for width in 1..=5 {
        let mut data = Vec::new();
        for x in 0..width {
            data.extend_from_slice(&[(x * 17) as u8, (x * 31) as u8, (x * 47) as u8]);
        }
        let frame = rgb(width, 1, data);
        assert_eq!(decode_bmp(&encode_bmp(&frame).unwrap()).unwrap(), frame);
    }
}

#[test]
fn bmp_multiline_round_trip_preserves_logical_top_left_order() {
    let frame = rgb(
        2,
        3,
        vec![
            255, 0, 0, 0, 255, 0, // top
            0, 0, 255, 255, 255, 0, // middle
            255, 0, 255, 0, 255, 255, // bottom
        ],
    );
    assert_eq!(decode_bmp(&encode_bmp(&frame).unwrap()).unwrap(), frame);
}

#[test]
fn bmp_rejects_zero_or_unrepresentable_dimensions_and_unsupported_depths() {
    let frame = VideoFrame::from_vec(1, 1, PixelFormat::Gray8, vec![0]).unwrap();
    assert!(encode_bmp(&frame).is_err());

    let mut file = encode_bmp(&rgb(1, 1, vec![1, 2, 3])).unwrap();
    file[18..22].copy_from_slice(&0_i32.to_le_bytes());
    assert!(decode_bmp(&file).is_err());

    let mut file = encode_bmp(&rgb(1, 1, vec![1, 2, 3])).unwrap();
    file[28..30].copy_from_slice(&16_u16.to_le_bytes());
    assert!(decode_bmp(&file).is_err());
}

#[test]
fn bmp_rejects_declared_file_smaller_than_raster() {
    let mut file = encode_bmp(&rgb(2, 1, vec![1, 2, 3, 4, 5, 6])).unwrap();
    file[2..6].copy_from_slice(&54_u32.to_le_bytes());
    assert!(decode_bmp(&file).is_err());
}

#[test]
fn tga_rejects_zero_dimensions_and_truncated_id() {
    let mut file = encode_tga(&rgb(1, 1, vec![1, 2, 3])).unwrap();
    file[12..14].copy_from_slice(&0_u16.to_le_bytes());
    assert!(decode_tga(&file).is_err());

    let mut file = encode_tga(&rgb(1, 1, vec![1, 2, 3])).unwrap();
    file[0] = 10;
    file.truncate(20);
    assert!(decode_tga(&file).is_err());
}

#[test]
fn tga_rle_splits_runs_longer_than_128_pixels() {
    let frame = rgb(300, 1, vec![9; 900]);
    let encoded = encode_tga_with_rle(&frame, true).unwrap();
    assert_eq!(decode_tga(&encoded).unwrap(), frame);
}

#[test]
fn tga_rle_splits_raw_packets_longer_than_128_pixels() {
    let mut data = Vec::new();
    for value in 0..=255_u8 {
        data.extend_from_slice(&[value, value.wrapping_mul(3), value.wrapping_mul(7)]);
    }
    let frame = rgb(256, 1, data);
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
fn tga_32_bit_alpha_is_preserved() {
    let mut file = vec![0_u8; 18];
    file[2] = 2;
    file[12..14].copy_from_slice(&1_u16.to_le_bytes());
    file[14..16].copy_from_slice(&1_u16.to_le_bytes());
    file[16] = 32;
    file[17] = 0x28;
    file.extend_from_slice(&[30, 20, 10, 99]);
    let frame = decode_tga(&file).unwrap();
    assert_eq!(frame.format, PixelFormat::Rgba32);
    assert_eq!(frame.data.as_slice(), &[10, 20, 30, 99]);
}

#[test]
fn tga_grayscale_rle_round_trip_handles_mixed_packets() {
    let frame =
        VideoFrame::from_vec(8, 1, PixelFormat::Gray8, vec![1, 1, 1, 2, 3, 4, 4, 4]).unwrap();
    let encoded = encode_tga_with_rle(&frame, true).unwrap();
    assert_eq!(encoded[2], 11);
    assert_eq!(decode_tga(&encoded).unwrap(), frame);
}
