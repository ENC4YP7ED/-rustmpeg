use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path =
        std::env::temp_dir().join(format!("rustmpeg-{label}-{}-{nonce}", std::process::id()));
    fs::create_dir_all(&path).unwrap();
    path
}

fn wav(format_tag: u16, channels: u16, sample_rate: u32, bits: u16, data: &[u8]) -> Vec<u8> {
    let block_align = channels * (bits / 8);
    let byte_rate = sample_rate * u32::from(block_align);
    let padded = data.len() + (data.len() & 1);
    let riff_size = 36 + padded;
    let mut output = Vec::with_capacity(riff_size + 8);
    output.extend_from_slice(b"RIFF");
    output.extend_from_slice(&(riff_size as u32).to_le_bytes());
    output.extend_from_slice(b"WAVEfmt ");
    output.extend_from_slice(&16_u32.to_le_bytes());
    output.extend_from_slice(&format_tag.to_le_bytes());
    output.extend_from_slice(&channels.to_le_bytes());
    output.extend_from_slice(&sample_rate.to_le_bytes());
    output.extend_from_slice(&byte_rate.to_le_bytes());
    output.extend_from_slice(&block_align.to_le_bytes());
    output.extend_from_slice(&bits.to_le_bytes());
    output.extend_from_slice(b"data");
    output.extend_from_slice(&(data.len() as u32).to_le_bytes());
    output.extend_from_slice(data);
    if data.len() & 1 != 0 {
        output.push(0);
    }
    output
}

fn run(args: &[&Path], literals: &[&str]) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ffmpeg"));
    let mut path_index = 0;
    for literal in literals {
        if *literal == "{path}" {
            command.arg(args[path_index]);
            path_index += 1;
        } else {
            command.arg(literal);
        }
    }
    command.output().unwrap()
}

#[test]
fn stream_copy_produces_valid_canonical_wave_with_identical_pcm() {
    let dir = temp_dir("copy");
    let input = dir.join("input.wav");
    let output = dir.join("output.wav");
    let pcm = [0x00, 0x80, 0xFF, 0x7F, 0x34, 0x12, 0xCC, 0xED];
    fs::write(&input, wav(1, 2, 48_000, 16, &pcm)).unwrap();

    let result = run(
        &[&input, &output],
        &["-hide_banner", "-y", "-i", "{path}", "-c", "copy", "{path}"],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );

    let bytes = fs::read(&output).unwrap();
    assert_eq!(&bytes[0..4], b"RIFF");
    assert_eq!(&bytes[8..12], b"WAVE");
    assert_eq!(u16::from_le_bytes([bytes[20], bytes[21]]), 1);
    assert_eq!(u16::from_le_bytes([bytes[34], bytes[35]]), 16);
    assert_eq!(&bytes[44..44 + pcm.len()], &pcm);
    assert!(String::from_utf8_lossy(&result.stderr).contains("(copy)"));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn pcm_s16_to_u8_transcoding_has_expected_endpoint_samples() {
    let dir = temp_dir("s16-u8");
    let input = dir.join("input.wav");
    let output = dir.join("output.wav");
    let mut pcm = Vec::new();
    pcm.extend_from_slice(&i16::MIN.to_le_bytes());
    pcm.extend_from_slice(&0_i16.to_le_bytes());
    pcm.extend_from_slice(&i16::MAX.to_le_bytes());
    fs::write(&input, wav(1, 1, 8_000, 16, &pcm)).unwrap();

    let result = run(
        &[&input, &output],
        &[
            "-hide_banner",
            "-y",
            "-i",
            "{path}",
            "-c:a",
            "pcm_u8",
            "{path}",
        ],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let bytes = fs::read(&output).unwrap();
    assert_eq!(u16::from_le_bytes([bytes[34], bytes[35]]), 8);
    assert_eq!(&bytes[44..47], &[0, 128, 255]);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn default_wave_encoder_converts_float32_to_signed16() {
    let dir = temp_dir("default-codec");
    let input = dir.join("input.wav");
    let output = dir.join("output.wav");
    let mut pcm = Vec::new();
    pcm.extend_from_slice(&(-1.0_f32).to_le_bytes());
    pcm.extend_from_slice(&0.0_f32.to_le_bytes());
    pcm.extend_from_slice(&1.0_f32.to_le_bytes());
    fs::write(&input, wav(3, 1, 48_000, 32, &pcm)).unwrap();

    let result = run(
        &[&input, &output],
        &["-hide_banner", "-y", "-i", "{path}", "{path}"],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let bytes = fs::read(&output).unwrap();
    assert_eq!(u16::from_le_bytes([bytes[20], bytes[21]]), 1);
    assert_eq!(u16::from_le_bytes([bytes[34], bytes[35]]), 16);
    let actual: Vec<i16> = bytes[44..50]
        .chunks_exact(2)
        .map(|sample| i16::from_le_bytes([sample[0], sample[1]]))
        .collect();
    assert_eq!(actual, [i16::MIN, 0, i16::MAX]);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn existing_output_requires_explicit_overwrite_policy() {
    let dir = temp_dir("overwrite");
    let input = dir.join("input.wav");
    let output = dir.join("output.wav");
    fs::write(&input, wav(1, 1, 8_000, 16, &[0, 0])).unwrap();
    fs::write(&output, b"existing").unwrap();

    let result = run(
        &[&input, &output],
        &["-hide_banner", "-i", "{path}", "-c", "copy", "{path}"],
    );
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("use -y to overwrite"));
    assert_eq!(fs::read(&output).unwrap(), b"existing");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn unknown_codec_and_unsupported_output_are_rejected() {
    let dir = temp_dir("reject");
    let input = dir.join("input.wav");
    let wave_output = dir.join("output.wav");
    let mp4_output = dir.join("output.mp4");
    fs::write(&input, wav(1, 1, 8_000, 16, &[0, 0])).unwrap();

    let codec_result = run(
        &[&input, &wave_output],
        &["-hide_banner", "-i", "{path}", "-c:a", "aac", "{path}"],
    );
    assert!(!codec_result.status.success());
    assert!(String::from_utf8_lossy(&codec_result.stderr).contains("not implemented"));
    assert!(!wave_output.exists());

    let format_result = run(
        &[&input, &mp4_output],
        &["-hide_banner", "-i", "{path}", "-c", "copy", "{path}"],
    );
    assert!(!format_result.status.success());
    assert!(String::from_utf8_lossy(&format_result.stderr).contains("cannot infer output format"));
    assert!(!mp4_output.exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn malformed_input_never_creates_output() {
    let dir = temp_dir("malformed");
    let input = dir.join("broken.wav");
    let output = dir.join("output.wav");
    fs::write(&input, b"RIFF\xFF\xFF\xFF\xFFWAVE").unwrap();

    let result = run(
        &[&input, &output],
        &["-hide_banner", "-y", "-i", "{path}", "-c", "copy", "{path}"],
    );
    assert!(!result.status.success());
    assert!(!output.exists());
    fs::remove_dir_all(dir).unwrap();
}
