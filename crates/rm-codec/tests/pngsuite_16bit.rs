use rm_codec::png::decode_png;
use rm_core::video::PixelFormat;

const BASN0G16: &[u8] = include_bytes!("../testdata/pngsuite/basn0g16.png");
const BASN2C16: &[u8] = include_bytes!("../testdata/pngsuite/basn2c16.png");
const BASN4A16: &[u8] = include_bytes!("../testdata/pngsuite/basn4a16.png");
const BASN6A16: &[u8] = include_bytes!("../testdata/pngsuite/basn6a16.png");

#[test]
fn pngsuite_16_bit_grayscale_decodes_to_gray8() {
    let frame = decode_png(BASN0G16).expect("basn0g16 must decode");
    assert_eq!(
        (frame.width, frame.height, frame.format),
        (32, 32, PixelFormat::Gray8)
    );
    assert_eq!(frame.data.len(), 32 * 32);
}

#[test]
fn pngsuite_16_bit_truecolor_decodes_to_rgb24() {
    let frame = decode_png(BASN2C16).expect("basn2c16 must decode");
    assert_eq!(
        (frame.width, frame.height, frame.format),
        (32, 32, PixelFormat::Rgb24)
    );
    assert_eq!(frame.data.len(), 32 * 32 * 3);
}

#[test]
fn pngsuite_16_bit_gray_alpha_preserves_alpha() {
    let frame = decode_png(BASN4A16).expect("basn4a16 must decode");
    assert_eq!(
        (frame.width, frame.height, frame.format),
        (32, 32, PixelFormat::GrayAlpha8)
    );
    assert_eq!(frame.data.len(), 32 * 32 * 2);
}

#[test]
fn pngsuite_16_bit_rgba_preserves_alpha() {
    let frame = decode_png(BASN6A16).expect("basn6a16 must decode");
    assert_eq!(
        (frame.width, frame.height, frame.format),
        (32, 32, PixelFormat::Rgba32)
    );
    assert_eq!(frame.data.len(), 32 * 32 * 4);
}

#[test]
fn upstream_fixtures_keep_expected_png_ihdr_depth_and_types() {
    for (name, bytes, color_type) in [
        ("basn0g16", BASN0G16, 0_u8),
        ("basn2c16", BASN2C16, 2_u8),
        ("basn4a16", BASN4A16, 4_u8),
        ("basn6a16", BASN6A16, 6_u8),
    ] {
        assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"), "{name}");
        assert_eq!(bytes[24], 16, "{name} bit depth fixture drift");
        assert_eq!(bytes[25], color_type, "{name} color type fixture drift");
    }
}
