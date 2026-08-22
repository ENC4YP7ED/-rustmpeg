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
        "rustmpeg-png-{label}-{}-{nonce}",
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

#[test]
fn ppm_to_png_and_back_preserves_rgb_pixels() {
    let dir = temp_dir("rgb-roundtrip");
    let source = dir.join("source.ppm");
    let png = dir.join("image.png");
    let restored = dir.join("restored.ppm");
    let expected = ppm(&[255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255], 2, 2);
    fs::write(&source, &expected).unwrap();

    let encoded = ffmpeg()
        .arg("-hide_banner")
        .arg("-y")
        .arg("-i")
        .arg(&source)
        .arg(&png)
        .output()
        .unwrap();
    assert_success(&encoded);
    assert_eq!(&fs::read(&png).unwrap()[..8], b"\x89PNG\r\n\x1a\n");

    let decoded = ffmpeg()
        .arg("-hide_banner")
        .arg("-y")
        .arg("-i")
        .arg(&png)
        .arg(&restored)
        .output()
        .unwrap();
    assert_success(&decoded);
    assert_eq!(fs::read(&restored).unwrap(), expected);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn pgm_to_png_and_back_preserves_grayscale_pixels() {
    let dir = temp_dir("gray-roundtrip");
    let source = dir.join("source.pgm");
    let png = dir.join("image.png");
    let restored = dir.join("restored.pgm");
    let expected = pgm(&[0, 64, 128, 255], 2, 2);
    fs::write(&source, &expected).unwrap();

    let encoded = ffmpeg()
        .arg("-hide_banner")
        .arg("-y")
        .arg("-i")
        .arg(&source)
        .arg(&png)
        .output()
        .unwrap();
    assert_success(&encoded);

    let decoded = ffmpeg()
        .arg("-hide_banner")
        .arg("-y")
        .arg("-i")
        .arg(&png)
        .arg(&restored)
        .output()
        .unwrap();
    assert_success(&decoded);
    assert_eq!(fs::read(&restored).unwrap(), expected);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn png_stream_copy_is_bit_exact() {
    let dir = temp_dir("copy");
    let source = dir.join("source.ppm");
    let input = dir.join("input.png");
    let output = dir.join("output.png");
    fs::write(&source, ppm(&[1, 2, 3, 4, 5, 6], 2, 1)).unwrap();

    let encoded = ffmpeg()
        .arg("-hide_banner")
        .arg("-y")
        .arg("-i")
        .arg(&source)
        .arg(&input)
        .output()
        .unwrap();
    assert_success(&encoded);
    let original = fs::read(&input).unwrap();

    let copied = ffmpeg()
        .arg("-hide_banner")
        .arg("-y")
        .arg("-i")
        .arg(&input)
        .arg("-c:v")
        .arg("copy")
        .arg(&output)
        .output()
        .unwrap();
    assert_success(&copied);
    assert_eq!(fs::read(&output).unwrap(), original);
    fs::remove_dir_all(dir).unwrap();
}
