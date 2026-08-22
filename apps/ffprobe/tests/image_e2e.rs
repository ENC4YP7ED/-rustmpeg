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
        "rustmpeg-ffprobe-image-{label}-{}-{nonce}",
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

fn ffprobe() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ffprobe"))
}

#[test]
fn single_ppm_reports_video_stream_and_image2_format() {
    let dir = temp_dir("single");
    let input = dir.join("input.ppm");
    fs::write(&input, ppm(&[255, 0, 0, 0, 255, 0], 2, 1)).unwrap();

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
    assert!(stdout.contains("codec_name=ppm"));
    assert!(stdout.contains("codec_type=video"));
    assert!(stdout.contains("width=2"));
    assert!(stdout.contains("height=1"));
    assert!(stdout.contains("pix_fmt=rgb24"));
    assert!(stdout.contains("r_frame_rate=25/1"));
    assert!(stdout.contains("nb_frames=1"));
    assert!(stdout.contains("duration=0.040000"));
    assert!(stdout.contains("format_name=image2"));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn sequence_probe_uses_start_range_rate_and_frame_count() {
    let dir = temp_dir("sequence");
    fs::write(dir.join("f-003.ppm"), ppm(&[0, 0, 0], 1, 1)).unwrap();
    fs::write(dir.join("f-004.ppm"), ppm(&[255, 255, 255], 1, 1)).unwrap();

    let output = ffprobe()
        .arg("-hide_banner")
        .arg("-f")
        .arg("image2")
        .arg("-start_number")
        .arg("0")
        .arg("-start_number_range")
        .arg("5")
        .arg("-framerate")
        .arg("10")
        .arg("-show_streams")
        .arg(dir.join("f-%03d.ppm"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("nb_frames=2"));
    assert!(stdout.contains("duration_ts=2"));
    assert!(stdout.contains("duration=0.200000"));
    assert!(stdout.contains("r_frame_rate=10/1"));
    assert!(stdout.contains("time_base=1/10"));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn capability_lists_advertise_only_implemented_image_surface() {
    let formats = ffprobe().arg("-formats").output().unwrap();
    assert!(formats.status.success());
    let formats = String::from_utf8(formats.stdout).unwrap();
    assert!(formats.contains("image2"));

    let codecs = ffprobe().arg("-codecs").output().unwrap();
    assert!(codecs.status.success());
    let codecs = String::from_utf8(codecs.stdout).unwrap();
    for codec in ["pbm", "pgm", "ppm"] {
        assert!(codecs.contains(codec), "missing image codec {codec}");
    }
    assert!(!codecs.contains("png"));
    assert!(!codecs.contains("mjpeg"));
}

#[test]
fn malformed_netpbm_fails_without_fake_metadata() {
    let dir = temp_dir("malformed");
    let input = dir.join("bad.ppm");
    fs::write(&input, b"P6\n2 2\n255\n\x00").unwrap();

    let output = ffprobe().arg("-show_streams").arg(&input).output().unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn missing_sequence_in_start_range_fails_cleanly() {
    let dir = temp_dir("missing");
    let output = ffprobe()
        .arg("-f")
        .arg("image2")
        .arg("-start_number")
        .arg("10")
        .arg("-start_number_range")
        .arg("3")
        .arg(dir.join("f-%03d.ppm"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no image2 frame found"));
    fs::remove_dir_all(dir).unwrap();
}
