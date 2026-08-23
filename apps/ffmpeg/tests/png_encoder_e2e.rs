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
        "rustmpeg-png-encoder-{label}-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn ffmpeg() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ffmpeg"))
}

fn run(input: &std::path::Path, output: &std::path::Path) {
    let result = ffmpeg()
        .arg("-hide_banner")
        .arg("-y")
        .arg("-i")
        .arg(input)
        .arg(output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "stderr: {}\nstdout: {}",
        String::from_utf8_lossy(&result.stderr),
        String::from_utf8_lossy(&result.stdout)
    );
}

fn canonical_rgba_tga() -> Vec<u8> {
    let mut bytes = vec![0_u8; 18];
    bytes[2] = 2;
    bytes[12..14].copy_from_slice(&2_u16.to_le_bytes());
    bytes[14..16].copy_from_slice(&1_u16.to_le_bytes());
    bytes[16] = 32;
    bytes[17] = 0x28;
    bytes.extend_from_slice(&[
        0, 0, 255, 0,
        0, 255, 0, 128,
    ]);
    bytes
}

#[test]
fn tga_rgba_to_png_and_back_preserves_alpha_exactly() {
    let dir = temp_dir("alpha");
    let input = dir.join("source.tga");
    let png = dir.join("image.png");
    let restored = dir.join("restored.tga");
    let source = canonical_rgba_tga();
    fs::write(&input, &source).unwrap();

    run(&input, &png);
    run(&png, &restored);
    assert_eq!(fs::read(&restored).unwrap(), source);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn flat_image_uses_compressed_zlib_and_is_materially_smaller_than_raw_pixels() {
    let dir = temp_dir("compression");
    let source = dir.join("flat.pgm");
    let png = dir.join("flat.png");
    let pixels = vec![0x7F_u8; 256 * 256];
    let mut pgm = b"P5\n256 256\n255\n".to_vec();
    pgm.extend_from_slice(&pixels);
    fs::write(&source, pgm).unwrap();

    run(&source, &png);
    let encoded = fs::read(&png).unwrap();
    assert!(
        encoded.len() < pixels.len() / 8,
        "encoded size was {}",
        encoded.len()
    );

    let idat = find_first_chunk(&encoded, b"IDAT").expect("PNG must contain IDAT");
    assert!(idat.len() >= 2);
    assert_eq!(idat[0] & 0x0f, 8, "zlib must use DEFLATE");
    assert_eq!(u16::from_be_bytes([idat[0], idat[1]]) % 31, 0);
    // FFmpeg uses zlib's default compression level when unset; our level-6 default
    // maps to the same FLEVEL=2 header class (normally 0x78 0x9C).
    assert_eq!(idat[1] >> 6, 2);
    fs::remove_dir_all(dir).unwrap();
}

fn find_first_chunk<'a>(png: &'a [u8], wanted: &[u8; 4]) -> Option<&'a [u8]> {
    let mut offset = 8_usize;
    while offset.checked_add(12)? <= png.len() {
        let length = usize::try_from(u32::from_be_bytes(
            png.get(offset..offset + 4)?.try_into().ok()?,
        ))
        .ok()?;
        let kind: &[u8; 4] = png.get(offset + 4..offset + 8)?.try_into().ok()?;
        let start = offset + 8;
        let end = start.checked_add(length)?;
        let data = png.get(start..end)?;
        if kind == wanted {
            return Some(data);
        }
        offset = end.checked_add(4)?;
    }
    None
}
