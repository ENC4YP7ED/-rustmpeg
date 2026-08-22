use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("rustmpeg-{label}-{}-{nonce}", std::process::id()));
    fs::create_dir_all(&path).unwrap();
    path
}

fn wav_s16_mono(sample_rate: u32, samples: &[i16]) -> Vec<u8> {
    let mut data = Vec::with_capacity(samples.len() * 2);
    for sample in samples {
        data.extend_from_slice(&sample.to_le_bytes());
    }
    let block_align = 2_u16;
    let byte_rate = sample_rate * u32::from(block_align);
    let riff_size = 36_u32 + data.len() as u32;
    let mut output = Vec::new();
    output.extend_from_slice(b"RIFF");
    output.extend_from_slice(&riff_size.to_le_bytes());
    output.extend_from_slice(b"WAVEfmt ");
    output.extend_from_slice(&16_u32.to_le_bytes());
    output.extend_from_slice(&1_u16.to_le_bytes());
    output.extend_from_slice(&1_u16.to_le_bytes());
    output.extend_from_slice(&sample_rate.to_le_bytes());
    output.extend_from_slice(&byte_rate.to_le_bytes());
    output.extend_from_slice(&block_align.to_le_bytes());
    output.extend_from_slice(&16_u16.to_le_bytes());
    output.extend_from_slice(b"data");
    output.extend_from_slice(&(data.len() as u32).to_le_bytes());
    output.extend_from_slice(&data);
    output
}

#[test]
fn show_streams_and_format_report_consistent_metadata() {
    let dir = temp_dir("probe-sections");
    let input = dir.join("input.wav");
    fs::write(&input, wav_s16_mono(8_000, &[0; 8_000])).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_ffprobe"))
        .args(["-hide_banner", "-show_streams", "-show_format"])
        .arg(&input)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("[STREAM]"));
    assert!(stdout.contains("codec_name=pcm_s16le"));
    assert!(stdout.contains("sample_rate=8000"));
    assert!(stdout.contains("channels=1"));
    assert!(stdout.contains("duration_ts=8000"));
    assert!(stdout.contains("duration=1.000000"));
    assert!(stdout.contains("[FORMAT]"));
    assert!(stdout.contains("format_name=wav"));
    assert!(stdout.contains("nb_streams=1"));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn human_summary_contains_duration_codec_and_rate() {
    let dir = temp_dir("probe-human");
    let input = dir.join("input.wav");
    fs::write(&input, wav_s16_mono(44_100, &[0; 44_100])).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_ffprobe"))
        .arg(&input)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("Input #0, wav"));
    assert!(stderr.contains("Duration: 00:00:01.00"));
    assert!(stderr.contains("Audio: pcm_s16le, 44100 Hz, 1 channels"));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn capability_lists_only_advertise_real_implemented_surface() {
    let formats = Command::new(env!("CARGO_BIN_EXE_ffprobe"))
        .arg("-formats")
        .output()
        .unwrap();
    assert!(formats.status.success());
    let formats = String::from_utf8(formats.stdout).unwrap();
    assert!(formats.contains("DE wav"));
    assert!(!formats.contains(" matroska"));

    let codecs = Command::new(env!("CARGO_BIN_EXE_ffprobe"))
        .arg("-codecs")
        .output()
        .unwrap();
    assert!(codecs.status.success());
    let codecs = String::from_utf8(codecs.stdout).unwrap();
    for name in ["pcm_u8", "pcm_s16le", "pcm_s24le", "pcm_s32le", "pcm_f32le", "pcm_f64le"] {
        assert!(codecs.contains(name), "missing codec {name}");
    }
    assert!(!codecs.contains("h264"));
}

#[test]
fn malformed_and_unsupported_inputs_fail_cleanly() {
    let dir = temp_dir("probe-invalid");
    let malformed = dir.join("malformed.wav");
    let unsupported = dir.join("unsupported.bin");
    fs::write(&malformed, b"RIFF\xFF\xFF\xFF\xFFWAVE").unwrap();
    fs::write(&unsupported, b"not media").unwrap();

    for input in [&malformed, &unsupported] {
        let output = Command::new(env!("CARGO_BIN_EXE_ffprobe"))
            .arg(input)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(!output.stderr.is_empty());
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn unsupported_output_writer_and_missing_input_are_rejected() {
    let writer = Command::new(env!("CARGO_BIN_EXE_ffprobe"))
        .args(["-of", "json", "missing.wav"])
        .output()
        .unwrap();
    assert!(!writer.status.success());
    assert!(String::from_utf8_lossy(&writer.stderr).contains("not implemented"));

    let missing = Command::new(env!("CARGO_BIN_EXE_ffprobe"))
        .arg("-show_streams")
        .output()
        .unwrap();
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("missing input file"));
}
