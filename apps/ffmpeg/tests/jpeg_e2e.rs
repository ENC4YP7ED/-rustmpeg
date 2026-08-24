use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "rustmpeg-jpeg-{label}-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn ppm(rgb: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut bytes = format!("P6\n{width} {height}\n255\n").into_bytes();
    bytes.extend_from_slice(rgb);
    bytes
}

fn pgm(gray: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut bytes = format!("P5\n{width} {height}\n255\n").into_bytes();
    bytes.extend_from_slice(gray);
    bytes
}

fn payload(bytes: &[u8]) -> &[u8] {
    let mut newlines = 0;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'\n' {
            newlines += 1;
            if newlines == 3 {
                return &bytes[i + 1..];
            }
        }
    }
    panic!("invalid Netpbm fixture")
}

fn ffmpeg() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ffmpeg"))
}
fn assert_success(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "stderr: {}\nstdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
}

fn assert_near(actual: &[u8], expected: &[u8], tolerance: u8) {
    assert_eq!(actual.len(), expected.len());
    for (i, (&a, &e)) in actual.iter().zip(expected).enumerate() {
        assert!(
            a.abs_diff(e) <= tolerance,
            "sample {i}: got {a}, expected {e}, tolerance {tolerance}"
        );
    }
}

#[test]
fn rgb_ppm_to_jpeg_and_back_is_visually_bounded() {
    let dir = temp_dir("rgb-roundtrip");
    let source = dir.join("source.ppm");
    let jpeg = dir.join("image.jpg");
    let restored = dir.join("restored.ppm");
    let mut rgb = Vec::new();
    for y in 0..16u8 {
        for x in 0..16u8 {
            rgb.extend_from_slice(&[
                x.saturating_mul(12),
                y.saturating_mul(12),
                x.wrapping_add(y).saturating_mul(6),
            ]);
        }
    }
    fs::write(&source, ppm(&rgb, 16, 16)).unwrap();
    let encoded = ffmpeg()
        .arg("-hide_banner")
        .arg("-y")
        .arg("-i")
        .arg(&source)
        .arg(&jpeg)
        .output()
        .unwrap();
    assert_success(&encoded);
    let encoded_bytes = fs::read(&jpeg).unwrap();
    assert!(encoded_bytes.starts_with(&[0xff, 0xd8]));
    assert!(encoded_bytes.ends_with(&[0xff, 0xd9]));
    let decoded = ffmpeg()
        .arg("-hide_banner")
        .arg("-y")
        .arg("-i")
        .arg(&jpeg)
        .arg(&restored)
        .output()
        .unwrap();
    assert_success(&decoded);
    assert_near(payload(&fs::read(&restored).unwrap()), &rgb, 18);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn grayscale_pgm_to_jpeg_and_back_is_visually_bounded() {
    let dir = temp_dir("gray-roundtrip");
    let source = dir.join("source.pgm");
    let jpeg = dir.join("image.jpeg");
    let restored = dir.join("restored.pgm");
    let gray: Vec<u8> = (0..=255).collect();
    fs::write(&source, pgm(&gray, 16, 16)).unwrap();
    assert_success(
        &ffmpeg()
            .arg("-hide_banner")
            .arg("-y")
            .arg("-i")
            .arg(&source)
            .arg(&jpeg)
            .output()
            .unwrap(),
    );
    assert_success(
        &ffmpeg()
            .arg("-hide_banner")
            .arg("-y")
            .arg("-i")
            .arg(&jpeg)
            .arg(&restored)
            .output()
            .unwrap(),
    );
    assert_near(payload(&fs::read(&restored).unwrap()), &gray, 5);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn jpeg_stream_copy_is_bit_exact() {
    let dir = temp_dir("copy");
    let source = dir.join("source.ppm");
    let input = dir.join("input.jpg");
    let output = dir.join("output.jpg");
    fs::write(&source, ppm(&[32, 64, 96].repeat(64), 8, 8)).unwrap();
    assert_success(
        &ffmpeg()
            .arg("-hide_banner")
            .arg("-y")
            .arg("-i")
            .arg(&source)
            .arg(&input)
            .output()
            .unwrap(),
    );
    let original = fs::read(&input).unwrap();
    assert_success(
        &ffmpeg()
            .arg("-hide_banner")
            .arg("-y")
            .arg("-i")
            .arg(&input)
            .arg("-c:v")
            .arg("copy")
            .arg(&output)
            .output()
            .unwrap(),
    );
    assert_eq!(fs::read(&output).unwrap(), original);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn malformed_jpeg_fails_without_output() {
    let dir = temp_dir("malformed");
    let input = dir.join("bad.jpg");
    let output = dir.join("out.ppm");
    fs::write(&input, [0xff, 0xd8, 0xff, 0xdb, 0x00, 0x43, 0x00, 1, 2, 3]).unwrap();
    let result = ffmpeg()
        .arg("-hide_banner")
        .arg("-y")
        .arg("-i")
        .arg(&input)
        .arg(&output)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(!output.exists());
    fs::remove_dir_all(dir).unwrap();
}
