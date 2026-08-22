use rm_core::{MediaError, Result};

use crate::{adler32, deflate::inflate};

const STORED_BLOCK_MAX: usize = u16::MAX as usize;

/// Decompresses an RFC 1950 zlib stream with an explicit uncompressed-size bound.
///
/// Preset dictionaries are deliberately rejected until dictionary negotiation is
/// implemented by a caller that can provide the exact DICTID-selected bytes.
///
/// # Errors
///
/// Returns an error for malformed zlib headers, unsupported methods/windows or
/// preset dictionaries, malformed DEFLATE payloads, output-limit violations, or
/// an Adler-32 mismatch.
pub fn decompress(bytes: &[u8], output_limit: usize) -> Result<Vec<u8>> {
    if bytes.len() < 6 {
        return Err(MediaError::invalid_data(
            "zlib stream is shorter than header plus Adler-32 trailer",
        ));
    }

    let cmf = bytes[0];
    let flg = bytes[1];
    let compression_method = cmf & 0x0F;
    let window_info = cmf >> 4;

    if compression_method != 8 {
        return Err(MediaError::unsupported(format!(
            "zlib compression method {compression_method} is not DEFLATE"
        )));
    }
    if window_info > 7 {
        return Err(MediaError::invalid_data(
            "zlib window size exceeds the RFC 1950 maximum",
        ));
    }
    if (u16::from(cmf) << 8 | u16::from(flg)) % 31 != 0 {
        return Err(MediaError::invalid_data("zlib FCHECK validation failed"));
    }
    if flg & 0x20 != 0 {
        return Err(MediaError::unsupported(
            "zlib preset dictionaries are not implemented yet",
        ));
    }

    let payload_end = bytes.len() - 4;
    let output = inflate(&bytes[2..payload_end], output_limit)?;
    let expected = u32::from_be_bytes([
        bytes[payload_end],
        bytes[payload_end + 1],
        bytes[payload_end + 2],
        bytes[payload_end + 3],
    ]);
    let actual = adler32(&output);
    if actual != expected {
        return Err(MediaError::invalid_data(format!(
            "zlib Adler-32 mismatch: expected 0x{expected:08x}, got 0x{actual:08x}"
        )));
    }
    Ok(output)
}

/// Encodes an RFC 1950 zlib stream using only RFC 1951 stored blocks.
///
/// This intentionally performs no compression. It is the deterministic baseline
/// encoder used by formats such as PNG until repository-owned match finding and
/// Huffman encoding are layered on top.
///
/// # Errors
///
/// Returns an error if the encoded output size cannot be represented by `usize`.
pub fn compress_stored(bytes: &[u8]) -> Result<Vec<u8>> {
    let block_count = if bytes.is_empty() {
        1
    } else {
        bytes.len().div_ceil(STORED_BLOCK_MAX)
    };
    let overhead = block_count
        .checked_mul(5)
        .and_then(|value| value.checked_add(6))
        .ok_or_else(|| MediaError::overflow("zlib stored-stream overhead overflow"))?;
    let capacity = bytes
        .len()
        .checked_add(overhead)
        .ok_or_else(|| MediaError::overflow("zlib stored-stream size overflow"))?;
    let mut output = Vec::with_capacity(capacity);

    // CM=8, CINFO=7 (32 KiB window), FLEVEL=0, FDICT=0, FCHECK=1.
    output.extend_from_slice(&[0x78, 0x01]);

    if bytes.is_empty() {
        write_stored_block(&mut output, &[], true)?;
    } else {
        let total_chunks = block_count;
        for (index, chunk) in bytes.chunks(STORED_BLOCK_MAX).enumerate() {
            write_stored_block(&mut output, chunk, index + 1 == total_chunks)?;
        }
    }

    output.extend_from_slice(&adler32(bytes).to_be_bytes());
    Ok(output)
}

fn write_stored_block(output: &mut Vec<u8>, bytes: &[u8], final_block: bool) -> Result<()> {
    let len = u16::try_from(bytes.len()).map_err(|_| {
        MediaError::invalid_argument("DEFLATE stored block exceeds 65535 bytes")
    })?;
    output.push(u8::from(final_block));
    output.extend_from_slice(&len.to_le_bytes());
    output.extend_from_slice(&(!len).to_le_bytes());
    output.extend_from_slice(bytes);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_fixed_huffman_zlib_vector_decodes() {
        let stream = [
            0x78, 0x9C, 0xCB, 0x48, 0xCD, 0xC9, 0xC9, 0x57, 0xC8, 0x40, 0x27, 0x15, 0x01,
            0x70, 0xD5, 0x08, 0xD2,
        ];
        assert_eq!(
            decompress(&stream, 1024).unwrap(),
            b"hello hello hello hello!"
        );
    }

    #[test]
    fn dynamic_huffman_zlib_wrapper_matches_verified_raw_payload() {
        let mut raw = vec![
            0xED, 0xC7, 0x31, 0x01, 0x00, 0x20, 0x0C, 0x03, 0x30, 0xAD, 0xA5, 0xC3, 0xBF,
            0x05, 0x26, 0x00, 0x09, 0xC9, 0x97, 0x64, 0x9D, 0xD5, 0x76, 0xE6, 0x46,
        ];
        raw.extend_from_slice(&[0x55; 28]);
        raw.extend_from_slice(&[0xF5, 0xD7, 0x07]);

        let expected = b"aaaaabbbbcccdde".repeat(1_000);
        assert_eq!(crate::deflate::inflate(&raw, 20_000).unwrap(), expected);
        assert_eq!(adler32(&expected), 0x4ECB_8303);

        let mut stream = Vec::with_capacity(2 + raw.len() + 4);
        stream.extend_from_slice(&[0x78, 0xDA]);
        stream.extend_from_slice(&raw);
        stream.extend_from_slice(&adler32(&expected).to_be_bytes());

        assert_eq!(decompress(&stream, 20_000).unwrap(), expected);
    }

    #[test]
    fn stored_encoder_round_trips_empty_small_and_multiblock_payloads() {
        for source in [
            Vec::new(),
            b"stored zlib baseline".to_vec(),
            (0..150_000_u32)
                .map(|value| (value.wrapping_mul(37) & 0xFF) as u8)
                .collect(),
        ] {
            let encoded = compress_stored(&source).unwrap();
            assert_eq!(&encoded[..2], &[0x78, 0x01]);
            assert_eq!(decompress(&encoded, source.len()).unwrap(), source);
        }
    }

    #[test]
    fn stored_encoder_splits_at_rfc1951_limit_and_marks_only_last_block_final() {
        let source = vec![0xA5; STORED_BLOCK_MAX + 1];
        let encoded = compress_stored(&source).unwrap();

        assert_eq!(encoded[2] & 1, 0);
        let second_header = 2 + 5 + STORED_BLOCK_MAX;
        assert_eq!(encoded[second_header] & 1, 1);
        assert_eq!(decompress(&encoded, source.len()).unwrap(), source);
    }

    #[test]
    fn bad_header_check_bits_are_rejected() {
        let stream = [0x78, 0x9D, 0x03, 0x00, 0x00, 0x00];
        assert!(decompress(&stream, 16).is_err());
    }

    #[test]
    fn preset_dictionary_flag_is_rejected_explicitly() {
        let stream = [0x78, 0x20, 0x00, 0x00, 0x00, 0x01];
        assert!(decompress(&stream, 16).is_err());
    }

    #[test]
    fn corrupt_adler_trailer_is_rejected() {
        let mut stream = [
            0x78, 0x9C, 0xCB, 0x48, 0xCD, 0xC9, 0xC9, 0x57, 0xC8, 0x40, 0x27, 0x15, 0x01,
            0x70, 0xD5, 0x08, 0xD2,
        ];
        *stream.last_mut().unwrap() ^= 1;
        assert!(decompress(&stream, 1024).is_err());
    }
}
