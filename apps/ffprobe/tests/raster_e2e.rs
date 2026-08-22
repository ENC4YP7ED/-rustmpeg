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
        "rustmpeg-ffprobe-raster-{label}-{}-{nonce}",
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

fn ffmpeg() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ffmpeg"))
}

fn ffprobe() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ffprobe"))
}

fn make_image(extension: &str, pixels: &[u8], width: u32, height: u32) -> (PathBuf, PathBuf) {
    let dir = temp_dir(extension);
    let input = dir.join("source.ppm");
    let output = dir.join(format!("image.{extension}"));
    fs::write(&input, ppm(pixels, width, height)).unwrap();
    let result = ffmpeg()
        .arg("-hide_banner")
        .arg("-y")
        .arg("-i")
        .arg(&input)
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    (dir, output)
}

#[test]
fn bmp_reports_typed_video_metadata() {
    let (dir, input) = make_image("bmp", &[255, 0, 0, 0, 255, 0], 2, 1);
    let output = ffprobe()
        .arg("-hide_banner")
        .arg("-show_streams")
        .arg("-show_format")
        .arg(&input)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("codec_name=bmp"));
    assert!(stdout.contains("codec_type=video"));
    assert!(stdout.contains("width=2"));
    assert!(stdout.contains("height=1"));
    assert!(stdout.contains("pix_fmt=rgb24"));
    assert!(stdout.contains("format_name=image2"));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn targa_reports_typed_video_metadata() {
    let (dir, input) = make_image("tga", &[0, 0, 255, 255, 255, 255], 2, 1);
    let output = ffprobe()
        .arg("-hide_banner")
        .arg("-show_streams")
        .arg("-show_format")
        .arg(&input)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("codec_name=targa"));
    assert!(stdout.contains("codec_type=video"));
    assert!(stdout.contains("width=2"));
    assert!(stdout.contains("height=1"));
    assert!(stdout.contains("pix_fmt=rgb24"));
    assert!(stdout.contains("format_name=image2"));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn capability_list_includes_only_the_newly_implemented_rasters() {
    let output = ffprobe().arg("-codecs").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    for codec in ["pbm", "pgm", "ppm", "bmp", "targa"] {
        assert!(stdout.contains(codec), "missing codec {codec}");
    }
    assert!(!stdout.contains("png"));
    assert!(!stdout.contains("mjpeg"));
}

#[test]
fn malformed_bmp_and_tga_fail_without_stream_metadata() {
    let dir = temp_dir("malformed");
    let bmp = dir.join("bad.bmp");
    let tga = dir.join("bad.tga");
    fs::write(&bmp, b"BM\0\0\0\0").unwrap();
    fs::write(&tga, [0_u8; 18]).unwrap();

    for input in [&bmp, &tga] {
        let output = ffprobe()
            .arg("-hide_banner")
            .arg("-show_streams")
            .arg(input)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
    fs::remove_dir_all(dir).unwrap();
}
