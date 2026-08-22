use rm_core::{MediaError, Result};

use crate::{adler32, deflate::inflate};

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
    fn standard_dynamic_huffman_zlib_vector_decodes() {
        let stream = [
            0x78, 0xDA, 0xED, 0xC7, 0x31, 0x01, 0x00, 0x20, 0x0C, 0x03, 0x30, 0xAD, 0xA5,
            0xC3, 0xBF, 0x05, 0x26, 0x00, 0x09, 0xC9, 0x97, 0x64, 0x9D, 0xD5, 0x76, 0xE6,
            0x46, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55,
            0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55,
            0x55, 0xF5, 0xD7, 0x07, 0x4E, 0xCB, 0x83, 0x03,
        ];
        assert_eq!(
            decompress(&stream, 20_000).unwrap(),
            b"aaaaabbbbcccdde".repeat(1_000)
        );
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
