#![forbid(unsafe_code)]

const ADLER_MODULUS: u32 = 65_521;
const CRC32_POLYNOMIAL: u32 = 0xEDB8_8320;

/// Computes the Adler-32 checksum used by the zlib wrapper format.
#[must_use]
pub fn adler32(bytes: &[u8]) -> u32 {
    let mut a = 1_u32;
    let mut b = 0_u32;

    for chunk in bytes.chunks(5_552) {
        for &byte in chunk {
            a += u32::from(byte);
            b += a;
        }
        a %= ADLER_MODULUS;
        b %= ADLER_MODULUS;
    }

    (b << 16) | a
}

/// Computes the reflected IEEE CRC-32 checksum used by PNG chunks.
#[must_use]
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = 0_u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (CRC32_POLYNOMIAL & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_standard_check_vectors() {
        assert_eq!(crc32(b""), 0x0000_0000);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b"Wikipedia"), 0xADAA_C02E);
    }

    #[test]
    fn adler32_matches_standard_check_vectors() {
        assert_eq!(adler32(b""), 0x0000_0001);
        assert_eq!(adler32(b"123456789"), 0x091E_01DE);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn chunked_adler_reduction_matches_large_reference_vector() {
        let bytes = vec![0xFF; 100_000];
        assert_eq!(adler32(&bytes), 0x149A_302C);
    }
}
