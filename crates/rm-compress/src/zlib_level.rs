use rm_core::{MediaError, Result};

use crate::{adler32, deflate_level, zlib};

/// Compresses bytes as RFC1950 zlib using a level in 0..=9.
/// Level 0 uses RFC1951 stored blocks; levels 1..=9 use the repository-owned
/// effort-scaled fixed-Huffman LZ77 encoder.
///
/// # Errors
/// Returns an error for an invalid level or compression/size failure.
pub fn compress_with_level(bytes: &[u8], level: u8) -> Result<Vec<u8>> {
    if level > 9 {
        return Err(MediaError::invalid_argument(
            "zlib compression level must be in 0..=9",
        ));
    }
    if level == 0 {
        return zlib::compress_stored(bytes);
    }
    let deflated = deflate_level::compress_fixed_level(bytes, level)?;
    let capacity = deflated
        .len()
        .checked_add(6)
        .ok_or_else(|| MediaError::overflow("zlib output size overflow"))?;
    let mut output = Vec::with_capacity(capacity);

    // CM=8/CINFO=7. FLEVEL is advisory; map effort to zlib's four FLEVEL bands.
    let flevel = match level {
        1..=2 => 0_u8,
        3..=5 => 1,
        6 => 2,
        7..=9 => 3,
        _ => unreachable!(),
    };
    let cmf = 0x78_u8;
    let base_flg = flevel << 6;
    let remainder = (u16::from(cmf) << 8 | u16::from(base_flg)) % 31;
    let fcheck = if remainder == 0 { 0 } else { 31 - remainder };
    let flg = base_flg | u8::try_from(fcheck).expect("FCHECK is below 31");
    output.extend_from_slice(&[cmf, flg]);
    output.extend_from_slice(&deflated);
    output.extend_from_slice(&adler32(bytes).to_be_bytes());
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_levels_have_valid_headers_and_round_trip() {
        let source = b"compress-level-fixture-compress-level-fixture".repeat(3000);
        for level in 0..=9 {
            let encoded = compress_with_level(&source, level).unwrap();
            assert_eq!(u16::from_be_bytes([encoded[0], encoded[1]]) % 31, 0);
            assert_eq!(
                crate::zlib::decompress(&encoded, source.len()).unwrap(),
                source
            );
        }
    }

    #[test]
    fn level_zero_is_stored_and_high_effort_is_smaller_on_repetition() {
        let source = b"AAAAABBBBBCCCCCDDDDDEEEEE".repeat(10_000);
        let zero = compress_with_level(&source, 0).unwrap();
        let nine = compress_with_level(&source, 9).unwrap();
        assert_eq!(&zero[..2], &[0x78, 0x01]);
        assert!(nine.len() < zero.len() / 8);
    }
}
