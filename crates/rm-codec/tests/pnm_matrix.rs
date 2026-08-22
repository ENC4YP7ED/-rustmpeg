use rm_codec::pnm::{PnmKind, decode_pnm, encode_pnm, probe_pnm};
use rm_core::video::{PixelFormat, VideoFrame};

#[test]
fn all_six_netpbm_encodings_decode() {
    let p1 = decode_pnm(b"P1\n2 1\n0 1\n").unwrap();
    assert_eq!(p1.frame.data.as_slice(), &[255, 0]);

    let p2 = decode_pnm(b"P2\n2 1\n15\n0 15\n").unwrap();
    assert_eq!(p2.frame.data.as_slice(), &[0, 255]);

    let p3 = decode_pnm(b"P3\n1 1\n15\n15 7 0\n").unwrap();
    assert_eq!(p3.frame.data.as_slice(), &[255, 119, 0]);

    let p4 = decode_pnm(b"P4\n2 1\n\x40").unwrap();
    assert_eq!(p4.frame.data.as_slice(), &[255, 0]);

    let p5 = decode_pnm(b"P5\n2 1\n255\n\x00\xff").unwrap();
    assert_eq!(p5.frame.data.as_slice(), &[0, 255]);

    let p6 = decode_pnm(b"P6\n1 1\n255\n\xff\x80\x00").unwrap();
    assert_eq!(p6.frame.data.as_slice(), &[255, 128, 0]);
}

#[test]
fn comments_and_all_ascii_whitespace_are_accepted_in_ascii_headers() {
    let image = decode_pnm(b"P3\t# first\r\n 1\t1\r# second\n15\r\n15\t0  7\n").unwrap();
    assert_eq!(image.frame.width, 1);
    assert_eq!(image.frame.height, 1);
    assert_eq!(image.frame.data.as_slice(), &[255, 0, 119]);
}

#[test]
fn binary_headers_accept_crlf_without_consuming_raster_bytes() {
    let pgm = decode_pnm(b"P5\r\n1 1\r\n255\r\n\x0a").unwrap();
    assert_eq!(pgm.frame.data.as_slice(), &[10]);

    let ppm = decode_pnm(b"P6\r\n1 1\r\n255\r\n\x0d\x0a\x20").unwrap();
    assert_eq!(ppm.frame.data.as_slice(), &[13, 10, 32]);
}

#[test]
fn maxval_scaling_covers_low_eight_and_sixteen_bit_ranges() {
    let low = decode_pnm(b"P5\n3 1\n1\n\x00\x01\x01").unwrap();
    assert_eq!(low.frame.data.as_slice(), &[0, 255, 255]);

    let eight = decode_pnm(b"P5\n3 1\n15\n\x00\x07\x0f").unwrap();
    assert_eq!(eight.frame.data.as_slice(), &[0, 119, 255]);

    let mut sixteen = b"P6\n1 1\n65535\n".to_vec();
    sixteen.extend_from_slice(&[0x00, 0x00, 0x80, 0x00, 0xff, 0xff]);
    let sixteen = decode_pnm(&sixteen).unwrap();
    assert_eq!(sixteen.frame.data.as_slice(), &[0, 128, 255]);
}

#[test]
fn binary_sample_above_maxval_is_rejected() {
    assert!(decode_pnm(b"P5\n1 1\n15\n\x10").is_err());

    let mut sixteen = b"P5\n1 1\n256\n".to_vec();
    sixteen.extend_from_slice(&[0x01, 0x01]);
    assert!(decode_pnm(&sixteen).is_err());
}

#[test]
fn truncated_and_extra_binary_rasters_are_rejected() {
    assert!(decode_pnm(b"P6\n1 1\n255\n\x00\x01").is_err());
    assert!(decode_pnm(b"P6\n1 1\n255\n\x00\x01\x02\x03").is_err());
    assert!(decode_pnm(b"P4\n9 1\n\x00").is_err());
    assert!(decode_pnm(b"P4\n1 1\n\x00\x00").is_err());
}

#[test]
fn ascii_missing_extra_and_out_of_range_samples_are_rejected() {
    assert!(decode_pnm(b"P2\n2 1\n255\n1\n").is_err());
    assert!(decode_pnm(b"P2\n1 1\n255\n1 2\n").is_err());
    assert!(decode_pnm(b"P3\n1 1\n15\n0 0 16\n").is_err());
    assert!(decode_pnm(b"P1\n1 1\n9\n").is_err());
}

#[test]
fn invalid_tokens_dimensions_and_maxval_are_rejected() {
    assert!(decode_pnm(b"P6\nabc 1\n255\n").is_err());
    assert!(decode_pnm(b"P6\n1 0\n255\n").is_err());
    assert!(decode_pnm(b"P6\n1 1\n0\n").is_err());
    assert!(decode_pnm(b"P6\n1 1\n65536\n").is_err());
    assert!(decode_pnm(b"P6\n4294967295 4294967295\n255\n").is_err());
}

#[test]
fn probe_requires_valid_magic_and_delimiter() {
    assert_eq!(probe_pnm(b"P6\n"), 100);
    assert_eq!(probe_pnm(b"P1#comment\n"), 100);
    assert_eq!(probe_pnm(b"P7\n"), 0);
    assert_eq!(probe_pnm(b"P6X"), 0);
    assert_eq!(probe_pnm(b"P"), 0);
}

#[test]
fn pbm_ignores_unused_tail_bits_on_decode_and_zeroes_them_on_encode() {
    let image = decode_pnm(b"P4\n9 1\n\xff\xff").unwrap();
    assert_eq!(image.frame.data.as_slice(), &[0; 9]);
    let encoded = encode_pnm(PnmKind::Pbm, &image.frame).unwrap();
    assert_eq!(encoded, b"P4\n9 1\n\xff\x80");
}

#[test]
fn encoders_enforce_pixel_format_contracts() {
    let gray = VideoFrame::from_vec(1, 1, PixelFormat::Gray8, vec![0]).unwrap();
    let rgb = VideoFrame::from_vec(1, 1, PixelFormat::Rgb24, vec![0, 0, 0]).unwrap();

    assert!(encode_pnm(PnmKind::Pbm, &rgb).is_err());
    assert!(encode_pnm(PnmKind::Pgm, &rgb).is_err());
    assert!(encode_pnm(PnmKind::Ppm, &gray).is_err());
    assert!(encode_pnm(PnmKind::Pbm, &gray).is_ok());
    assert!(encode_pnm(PnmKind::Pgm, &gray).is_ok());
    assert!(encode_pnm(PnmKind::Ppm, &rgb).is_ok());
}

#[test]
fn canonical_binary_encoders_round_trip_pixels() {
    let gray = VideoFrame::from_vec(3, 1, PixelFormat::Gray8, vec![0, 127, 255]).unwrap();
    let pgm = encode_pnm(PnmKind::Pgm, &gray).unwrap();
    let decoded = decode_pnm(&pgm).unwrap();
    assert_eq!(decoded.frame, gray);

    let rgb = VideoFrame::from_vec(2, 1, PixelFormat::Rgb24, vec![1, 2, 3, 4, 5, 6]).unwrap();
    let ppm = encode_pnm(PnmKind::Ppm, &rgb).unwrap();
    let decoded = decode_pnm(&ppm).unwrap();
    assert_eq!(decoded.frame, rgb);
}
