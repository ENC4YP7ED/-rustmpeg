use rm_codec::png::decode_png;
use std::env;
use std::fs;
use std::path::Path;

const CASES: &[&str] = &[
    "0g01", "0g02", "0g04", "0g08", "0g16", "2c08", "2c16", "3p01", "3p02", "3p04", "3p08", "4a08",
    "4a16", "6a08", "6a16",
];

fn read(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
}

#[test]
fn adam7_basic_matrix_matches_non_interlaced_pngsuite_pixels() {
    let Some(root) = env::var_os("RUSTMPEG_PNGSUITE_DIR") else {
        eprintln!("RUSTMPEG_PNGSUITE_DIR is unset; external PngSuite matrix is exercised by CI");
        return;
    };
    let root = Path::new(&root);

    for case in CASES {
        let interlaced_path = root.join(format!("basi{case}.png"));
        let baseline_path = root.join(format!("basn{case}.png"));
        let interlaced = decode_png(&read(&interlaced_path)).unwrap_or_else(|error| {
            panic!("{} failed to decode: {error}", interlaced_path.display())
        });
        let baseline = decode_png(&read(&baseline_path)).unwrap_or_else(|error| {
            panic!("{} failed to decode: {error}", baseline_path.display())
        });

        assert_eq!(
            interlaced.width, baseline.width,
            "width mismatch for {case}"
        );
        assert_eq!(
            interlaced.height, baseline.height,
            "height mismatch for {case}"
        );
        assert_eq!(
            interlaced.format, baseline.format,
            "pixel format mismatch for {case}"
        );
        assert_eq!(
            interlaced.linesize, baseline.linesize,
            "linesize mismatch for {case}"
        );
        assert_eq!(
            interlaced.data, baseline.data,
            "decoded pixels mismatch for {case}"
        );
    }
}
