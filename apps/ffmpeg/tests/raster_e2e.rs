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
        "rustmpeg-raster-{label}-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn ppm(rgb: &[u8], width: u32, height: u32) -> Vec<u8> {
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
fn ppm_to_bmp_and_back_preserves_pixels_and_emits_canonical_header() {
    let dir = temp_dir("bmp-roundtrip");
    let source = dir.join("source.ppm");
    let bmp = dir.join("image.bmp");
    let restored = dir.join("restored.ppm");
    let pixels = [255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255];
    let expected = ppm(&pixels, 2, 2);
    fs::write(&source, &expected).unwrap();

    let encoded = ffmpeg()
        .arg("-hide_banner")
        .arg("-y")
        .arg("-i")
        .arg(&source)
        .arg(&bmp)
        .output()
        .unwrap();
    assert_success(&encoded);

    let bmp_bytes = fs::read(&bmp).unwrap();
    assert_eq!(&bmp_bytes[0..2], b"BM");
    assert_eq!(
        u32::from_le_bytes(bmp_bytes[10..14].try_into().unwrap()),
        54
    );
    assert_eq!(
        u16::from_le_bytes(bmp_bytes[28..30].try_into().unwrap()),
        24
    );

    let decoded = ffmpeg()
        .arg("-hide_banner")
        .arg("-y")
        .arg("-i")
        .arg(&bmp)
        .arg(&restored)
        .output()
        .unwrap();
    assert_success(&decoded);
    assert_eq!(fs::read(&restored).unwrap(), expected);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn ppm_to_tga_and_back_preserves_pixels_and_top_left_origin() {
    let dir = temp_dir("tga-roundtrip");
    let source = dir.join("source.ppm");
    let tga = dir.join("image.tga");
    let restored = dir.join("restored.ppm");
    let pixels = [255, 128, 0, 10, 20, 30, 40, 50, 60];
    let expected = ppm(&pixels, 3, 1);
    fs::write(&source, &expected).unwrap();

    let encoded = ffmpeg()
        .arg("-hide_banner")
        .arg("-y")
        .arg("-i")
        .arg(&source)
        .arg(&tga)
        .output()
        .unwrap();
    assert_success(&encoded);

    let tga_bytes = fs::read(&tga).unwrap();
    assert_eq!(tga_bytes[2], 2);
    assert_eq!(tga_bytes[16], 24);
    assert_eq!(tga_bytes[17] & 0x20, 0x20);

    let decoded = ffmpeg()
        .arg("-hide_banner")
        .arg("-y")
        .arg("-i")
        .arg(&tga)
        .arg(&restored)
        .output()
        .unwrap();
    assert_success(&decoded);
    assert_eq!(fs::read(&restored).unwrap(), expected);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn tga_stream_copy_is_bit_exact() {
    let dir = temp_dir("tga-copy");
    let source_ppm = dir.join("source.ppm");
    let input = dir.join("input.tga");
    let output = dir.join("output.tga");
    fs::write(&source_ppm, ppm(&[1, 2, 3, 4, 5, 6], 2, 1)).unwrap();

    let encoded = ffmpeg()
        .arg("-hide_banner")
        .arg("-y")
        .arg("-i")
        .arg(&source_ppm)
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

#[test]
fn bmp_sequence_can_transcode_to_tga_sequence() {
    let dir = temp_dir("sequence");
    for (index, pixel) in [[255, 0, 0], [0, 255, 0]].into_iter().enumerate() {
        let ppm_path = dir.join(format!("source-{index}.ppm"));
        let bmp_path = dir.join(format!("in-{index:03}.bmp"));
        fs::write(&ppm_path, ppm(&pixel, 1, 1)).unwrap();
        let encoded = ffmpeg()
            .arg("-hide_banner")
            .arg("-y")
            .arg("-i")
            .arg(&ppm_path)
            .arg(&bmp_path)
            .output()
            .unwrap();
        assert_success(&encoded);
    }

    let result = ffmpeg()
        .arg("-hide_banner")
        .arg("-y")
        .arg("-i")
        .arg(dir.join("in-%03d.bmp"))
        .arg(dir.join("out-%03d.tga"))
        .output()
        .unwrap();
    assert_success(&result);
    assert!(dir.join("out-001.tga").exists());
    assert!(dir.join("out-002.tga").exists());
    assert!(!dir.join("out-003.tga").exists());
    fs::remove_dir_all(dir).unwrap();
}
