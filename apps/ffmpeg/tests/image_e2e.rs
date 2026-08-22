use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "rustmpeg-ffmpeg-image-{label}-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn ppm_binary(rgb: &[u8], width: u32, height: u32) -> Vec<u8> {
    assert_eq!(rgb.len(), usize::try_from(width * height * 3).unwrap());
    let mut bytes = format!("P6\n{width} {height}\n255\n").into_bytes();
    bytes.extend_from_slice(rgb);
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
fn ppm_to_pgm_resize_runs_through_real_decode_scale_encode_pipeline() {
    let dir = temp_dir("resize");
    let input = dir.join("input.ppm");
    let output = dir.join("output.pgm");
    fs::write(&input, ppm_binary(&[255, 0, 0], 1, 1)).unwrap();

    let result = ffmpeg()
        .arg("-hide_banner")
        .arg("-y")
        .arg("-i")
        .arg(&input)
        .arg("-c:v")
        .arg("pgm")
        .arg("-s:v")
        .arg("2x2")
        .arg(&output)
        .output()
        .unwrap();
    assert_success(&result);

    assert_eq!(fs::read(&output).unwrap(), b"P5\n2 2\n255\nMMMM");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn image2_sequence_searches_start_range_and_renumbers_output() {
    let dir = temp_dir("sequence");
    fs::write(dir.join("in-003.ppm"), ppm_binary(&[0, 255, 0], 1, 1)).unwrap();
    fs::write(dir.join("in-004.ppm"), ppm_binary(&[0, 0, 255], 1, 1)).unwrap();
    let input_pattern = dir.join("in-%03d.ppm");
    let output_pattern = dir.join("out-%03d.pgm");

    let result = ffmpeg()
        .arg("-hide_banner")
        .arg("-y")
        .arg("-start_number")
        .arg("0")
        .arg("-start_number_range")
        .arg("5")
        .arg("-framerate")
        .arg("10")
        .arg("-i")
        .arg(&input_pattern)
        .arg("-start_number")
        .arg("7")
        .arg("-c:v")
        .arg("pgm")
        .arg(&output_pattern)
        .output()
        .unwrap();
    assert_success(&result);

    assert_eq!(fs::read(dir.join("out-007.pgm")).unwrap(), b"P5\n1 1\n255\n\x95");
    assert_eq!(fs::read(dir.join("out-008.pgm")).unwrap(), b"P5\n1 1\n255\n\x1d");
    assert!(!dir.join("out-009.pgm").exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn image_stream_copy_is_bit_exact_even_for_ascii_netpbm() {
    let dir = temp_dir("copy");
    let input = dir.join("input.ppm");
    let output = dir.join("output.ppm");
    let source = b"P3\n# keep this exact comment\n1 1\n15\n15 7 0\n";
    fs::write(&input, source).unwrap();

    let result = ffmpeg()
        .arg("-hide_banner")
        .arg("-y")
        .arg("-i")
        .arg(&input)
        .arg("-c:v")
        .arg("copy")
        .arg(&output)
        .output()
        .unwrap();
    assert_success(&result);
    assert_eq!(fs::read(&output).unwrap(), source);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn stream_copy_rejects_codec_changing_extension() {
    let dir = temp_dir("copy-mismatch");
    let input = dir.join("input.ppm");
    let output = dir.join("output.pgm");
    fs::write(&input, ppm_binary(&[1, 2, 3], 1, 1)).unwrap();

    let result = ffmpeg()
        .arg("-hide_banner")
        .arg("-y")
        .arg("-i")
        .arg(&input)
        .arg("-c")
        .arg("copy")
        .arg(&output)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(!output.exists());
    assert!(String::from_utf8_lossy(&result.stderr).contains("cannot stream-copy"));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn frame_limit_allows_sequence_to_single_image() {
    let dir = temp_dir("limit");
    fs::write(dir.join("in-000.ppm"), ppm_binary(&[255, 255, 255], 1, 1)).unwrap();
    fs::write(dir.join("in-001.ppm"), ppm_binary(&[0, 0, 0], 1, 1)).unwrap();
    let output = dir.join("first.pgm");

    let result = ffmpeg()
        .arg("-hide_banner")
        .arg("-y")
        .arg("-frames:v")
        .arg("1")
        .arg("-i")
        .arg(dir.join("in-%03d.ppm"))
        .arg(&output)
        .output()
        .unwrap();
    assert_success(&result);
    assert_eq!(fs::read(&output).unwrap(), b"P5\n1 1\n255\n\xff");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn existing_sequence_target_blocks_before_any_output_is_written() {
    let dir = temp_dir("overwrite-preflight");
    fs::write(dir.join("in-000.ppm"), ppm_binary(&[10, 10, 10], 1, 1)).unwrap();
    fs::write(dir.join("in-001.ppm"), ppm_binary(&[20, 20, 20], 1, 1)).unwrap();
    fs::write(dir.join("out-002.pgm"), b"existing").unwrap();

    let result = ffmpeg()
        .arg("-hide_banner")
        .arg("-i")
        .arg(dir.join("in-%03d.ppm"))
        .arg(dir.join("out-%03d.pgm"))
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(!dir.join("out-001.pgm").exists());
    assert_eq!(fs::read(dir.join("out-002.pgm")).unwrap(), b"existing");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn image2_rejects_multiple_frames_without_output_pattern() {
    let dir = temp_dir("no-output-pattern");
    fs::write(dir.join("in-000.ppm"), ppm_binary(&[1, 2, 3], 1, 1)).unwrap();
    fs::write(dir.join("in-001.ppm"), ppm_binary(&[4, 5, 6], 1, 1)).unwrap();
    let output = dir.join("out.ppm");

    let result = ffmpeg()
        .arg("-hide_banner")
        .arg("-y")
        .arg("-i")
        .arg(dir.join("in-%03d.ppm"))
        .arg(&output)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(!output.exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn forced_image2_accepts_single_file() {
    let dir = temp_dir("forced");
    let input = dir.join("input.data");
    let output = dir.join("output.ppm");
    fs::write(&input, ppm_binary(&[9, 8, 7], 1, 1)).unwrap();

    let result = ffmpeg()
        .arg("-hide_banner")
        .arg("-y")
        .arg("-f")
        .arg("image2")
        .arg("-i")
        .arg(&input)
        .arg(&output)
        .output()
        .unwrap();
    assert_success(&result);
    assert_eq!(fs::read(&output).unwrap(), ppm_binary(&[9, 8, 7], 1, 1));
    fs::remove_dir_all(dir).unwrap();
}

#[allow(dead_code)]
fn _assert_path(_: &Path) {}
