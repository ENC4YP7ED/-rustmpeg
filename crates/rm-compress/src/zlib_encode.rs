use rm_core::{MediaError, Result};

use crate::{adler32, deflate_encode};

/// Encodes an RFC1950 zlib stream backed by the repository-owned fixed-Huffman
/// RFC1951 encoder.
///
/// # Errors
///
/// Returns an error if DEFLATE encoding fails or output-size arithmetic overflows.
pub fn compress(bytes: &[u8]) -> Result<Vec<u8>> {
    let deflated = deflate_encode::compress_fixed(bytes)?;
    let capacity = deflated
        .len()
        .checked_add(6)
        .ok_or_else(|| MediaError::overflow("compressed zlib output size overflow"))?;
    let mut output = Vec::with_capacity(capacity);

    // CM=8, CINFO=7 (32KiB window). FLEVEL=1 reflects the current fast
    // single-candidate greedy compressor. FCHECK=30 makes the header divisible by 31.
    output.extend_from_slice(&[0x78, 0x5E]);
    output.extend_from_slice(&deflated);
    output.extend_from_slice(&adler32(bytes).to_be_bytes());
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zlib_header_is_valid_and_round_trips() {
        for input in [
            Vec::new(),
            b"hello hello hello hello".to_vec(),
            vec![0xA5; 100_000],
        ] {
            let encoded = compress(&input).unwrap();
            assert_eq!(u16::from_be_bytes([encoded[0], encoded[1]]) % 31, 0);
            assert_eq!(encoded[0] & 0x0f, 8);
            assert_eq!(
                crate::zlib::decompress(&encoded, input.len()).unwrap(),
                input
            );
        }
    }

    #[test]
    fn repetitive_zlib_is_smaller_than_stored_baseline() {
        let input = b"scanline scanline scanline scanline".repeat(4_000);
        let compressed = compress(&input).unwrap();
        let stored = crate::zlib::compress_stored(&input).unwrap();
        assert!(compressed.len() < stored.len() / 4);
    }
}
