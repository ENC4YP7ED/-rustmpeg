use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const PNG_RGB_2X1: [u8; 72] = [
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 2, 0, 0, 0, 1, 8, 2, 0,
    0, 0, 123, 64, 232, 221, 0, 0, 0, 15, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 192, 240,
    159, 1, 0, 7, 255, 1, 255, 1, 127, 137, 167, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

fn temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "rustmpeg-ffprobe-png-{label}-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn ffprobe() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ffprobe"))
}

#[test]
fn png_reports_typed_video_metadata() {
    let dir = temp_dir("metadata");
    let input = dir.join("image.png");
    fs::write(&input, PNG_RGB_2X1).unwrap();

    let output = ffprobe()
        .arg("-hide_banner")
        .arg("-show_streams")
        .arg("-show_format")
        .arg(&input)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("codec_name=png"));
    assert!(stdout.contains("codec_type=video"));
    assert!(stdout.contains("width=2"));
    assert!(stdout.contains("height=1"));
    assert!(stdout.contains("pix_fmt=rgb24"));
    assert!(stdout.contains("format_name=image2"));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn codec_list_advertises_png() {
    let output = ffprobe().arg("-codecs").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("png"));
}

#[test]
fn corrupt_png_crc_fails_without_stream_metadata() {
    let dir = temp_dir("crc");
    let input = dir.join("bad.png");
    let mut bytes = PNG_RGB_2X1;
    bytes[32] ^= 1;
    fs::write(&input, bytes).unwrap();

    let output = ffprobe()
        .arg("-hide_banner")
        .arg("-show_streams")
        .arg(&input)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    fs::remove_dir_all(dir).unwrap();
}
