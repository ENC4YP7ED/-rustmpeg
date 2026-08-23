use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let path = std::env::temp_dir().join(format!("rustmpeg-ffprobe-jpeg-{label}-{}-{nonce}", std::process::id()));
    fs::create_dir_all(&path).unwrap();
    path
}
fn ffprobe() -> Command { Command::new(env!("CARGO_BIN_EXE_ffprobe")) }
fn ppm(rgb: &[u8], w: u32, h: u32) -> Vec<u8> { let mut v=format!("P6\n{w} {h}\n255\n").into_bytes(); v.extend_from_slice(rgb); v }

#[test]
fn jpeg_reports_typed_video_metadata() {
    let dir=temp_dir("metadata");
    let input=dir.join("source.ppm"); let jpeg=dir.join("image.jpg");
    fs::write(&input, ppm(&[80,120,160].repeat(64),8,8)).unwrap();
    let ffmpeg=std::env::var_os("CARGO_BIN_EXE_ffmpeg").map(PathBuf::from);
    // Cargo does not guarantee sibling binary env vars for this integration target, so build a tiny
    // standards-compliant JPEG fixture using rm-codec is intentionally avoided here. This probe test
    // instead validates a static JPEG emitted by our canonical encoder structure.
    let bytes = include_bytes!("../../../crates/rm-codec/testdata/jpeg_constant_gray.jpg");
    fs::write(&jpeg, bytes).unwrap();
    let output=ffprobe().arg("-hide_banner").arg("-show_streams").arg("-show_format").arg(&jpeg).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let stdout=String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("codec_name=mjpeg"));
    assert!(stdout.contains("codec_type=video"));
    assert!(stdout.contains("format_name=image2"));
    let _=ffmpeg;
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn codec_list_advertises_jpeg() {
    let output=ffprobe().arg("-codecs").output().unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8(output.stdout).unwrap().contains("mjpeg"));
}

#[test]
fn malformed_jpeg_emits_no_fake_stream_metadata() {
    let dir=temp_dir("malformed"); let input=dir.join("bad.jpg");
    fs::write(&input,[0xff,0xd8,0xff,0xc0,0,2,0xff,0xd9]).unwrap();
    let output=ffprobe().arg("-show_streams").arg(&input).output().unwrap();
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("[STREAM]"));
    fs::remove_dir_all(dir).unwrap();
}
