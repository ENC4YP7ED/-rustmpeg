use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const ADAM7_GRAY: &[u8] = include_bytes!("../../../crates/rm-codec/testdata/pngsuite/basi0g01.png");

fn temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "rustmpeg-ffprobe-adam7-{label}-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn ffprobe() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ffprobe"))
}

#[test]
fn adam7_png_reports_decoded_video_metadata() {
    let dir = temp_dir("metadata");
    let input = dir.join("adam7.png");
    fs::write(&input, ADAM7_GRAY).unwrap();

    let output = ffprobe()
        .arg("-hide_banner")
        .arg("-show_streams")
        .arg("-show_format")
        .arg(&input)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("codec_name=png"));
    assert!(stdout.contains("codec_type=video"));
    assert!(stdout.contains("width=32"));
    assert!(stdout.contains("height=32"));
    assert!(stdout.contains("pix_fmt=gray8"));
    assert!(stdout.contains("format_name=image2"));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn malformed_adam7_png_emits_no_fake_stream_metadata() {
    let dir = temp_dir("malformed");
    let input = dir.join("broken.png");
    let mut broken = ADAM7_GRAY.to_vec();
    broken[50] ^= 1;
    fs::write(&input, broken).unwrap();

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
