use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const ADAM7_GRAY: &[u8] = include_bytes!("../../../crates/rm-codec/testdata/pngsuite/basi0g01.png");
const REFERENCE_GRAY: &[u8] =
    include_bytes!("../../../crates/rm-codec/testdata/pngsuite/basn0g01-reference.png");

fn temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "rustmpeg-adam7-{label}-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn ffmpeg() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ffmpeg"))
}

fn run_decode(input: &std::path::Path, output: &std::path::Path) {
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

#[test]
fn adam7_and_non_interlaced_reference_transcode_to_identical_pgm() {
    let dir = temp_dir("equivalence");
    let adam7 = dir.join("adam7.png");
    let reference = dir.join("reference.png");
    let adam7_pgm = dir.join("adam7.pgm");
    let reference_pgm = dir.join("reference.pgm");
    fs::write(&adam7, ADAM7_GRAY).unwrap();
    fs::write(&reference, REFERENCE_GRAY).unwrap();

    run_decode(&adam7, &adam7_pgm);
    run_decode(&reference, &reference_pgm);
    assert_eq!(fs::read(&adam7_pgm).unwrap(), fs::read(&reference_pgm).unwrap());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn malformed_adam7_input_fails_without_creating_output() {
    let dir = temp_dir("malformed");
    let input = dir.join("broken.png");
    let output = dir.join("out.pgm");
    let mut broken = ADAM7_GRAY.to_vec();
    broken.truncate(broken.len() - 9);
    fs::write(&input, broken).unwrap();

    let result = ffmpeg()
        .arg("-hide_banner")
        .arg("-y")
        .arg("-i")
        .arg(&input)
        .arg(&output)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(!output.exists());
    fs::remove_dir_all(dir).unwrap();
}
