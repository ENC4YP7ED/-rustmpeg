#![forbid(unsafe_code)]

pub mod bmp;
pub mod jpeg;
pub mod png;
pub mod png_encode;
pub mod pnm;
pub mod tga;

use rm_core::{MediaError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaType {
    Video,
    Audio,
    Subtitle,
    Data,
    Attachment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CodecId {
    PcmU8,
    PcmS16Le,
    PcmS24Le,
    PcmS32Le,
    PcmF32Le,
    PcmF64Le,
    Pbm,
    Pgm,
    Ppm,
    Bmp,
    Targa,
    Png,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodecDescriptor {
    pub id: CodecId,
    pub name: &'static str,
    pub long_name: &'static str,
    pub media_type: MediaType,
    pub can_decode: bool,
    pub can_encode: bool,
}

const CODECS: [CodecDescriptor; 12] = [
    CodecDescriptor {
        id: CodecId::PcmU8,
        name: "pcm_u8",
        long_name: "PCM unsigned 8-bit",
        media_type: MediaType::Audio,
        can_decode: true,
        can_encode: true,
    },
    CodecDescriptor {
        id: CodecId::PcmS16Le,
        name: "pcm_s16le",
        long_name: "PCM signed 16-bit little-endian",
        media_type: MediaType::Audio,
        can_decode: true,
        can_encode: true,
    },
    CodecDescriptor {
        id: CodecId::PcmS24Le,
        name: "pcm_s24le",
        long_name: "PCM signed 24-bit little-endian",
        media_type: MediaType::Audio,
        can_decode: true,
        can_encode: true,
    },
    CodecDescriptor {
        id: CodecId::PcmS32Le,
        name: "pcm_s32le",
        long_name: "PCM signed 32-bit little-endian",
        media_type: MediaType::Audio,
        can_decode: true,
        can_encode: true,
    },
    CodecDescriptor {
        id: CodecId::PcmF32Le,
        name: "pcm_f32le",
        long_name: "PCM 32-bit floating point little-endian",
        media_type: MediaType::Audio,
        can_decode: true,
        can_encode: true,
    },
    CodecDescriptor {
        id: CodecId::PcmF64Le,
        name: "pcm_f64le",
        long_name: "PCM 64-bit floating point little-endian",
        media_type: MediaType::Audio,
        can_decode: true,
        can_encode: true,
    },
    CodecDescriptor {
        id: CodecId::Pbm,
        name: "pbm",
        long_name: "PBM (Portable BitMap) image",
        media_type: MediaType::Video,
        can_decode: true,
        can_encode: true,
    },
    CodecDescriptor {
        id: CodecId::Pgm,
        name: "pgm",
        long_name: "PGM (Portable GrayMap) image",
        media_type: MediaType::Video,
        can_decode: true,
        can_encode: true,
    },
    CodecDescriptor {
        id: CodecId::Ppm,
        name: "ppm",
        long_name: "PPM (Portable PixelMap) image",
        media_type: MediaType::Video,
        can_decode: true,
        can_encode: true,
    },
    CodecDescriptor {
        id: CodecId::Bmp,
        name: "bmp",
        long_name: "BMP (Windows and OS/2 bitmap)",
        media_type: MediaType::Video,
        can_decode: true,
        can_encode: true,
    },
    CodecDescriptor {
        id: CodecId::Targa,
        name: "targa",
        long_name: "Truevision Targa image",
        media_type: MediaType::Video,
        can_decode: true,
        can_encode: true,
    },
    CodecDescriptor {
        id: CodecId::Png,
        name: "png",
        long_name: "PNG (Portable Network Graphics) image",
        media_type: MediaType::Video,
        can_decode: true,
        can_encode: true,
    },
];

#[must_use]
pub const fn codecs() -> &'static [CodecDescriptor] {
    &CODECS
}

#[must_use]
pub const fn descriptor(id: CodecId) -> &'static CodecDescriptor {
    match id {
        CodecId::PcmU8 => &CODECS[0],
        CodecId::PcmS16Le => &CODECS[1],
        CodecId::PcmS24Le => &CODECS[2],
        CodecId::PcmS32Le => &CODECS[3],
        CodecId::PcmF32Le => &CODECS[4],
        CodecId::PcmF64Le => &CODECS[5],
        CodecId::Pbm => &CODECS[6],
        CodecId::Pgm => &CODECS[7],
        CodecId::Ppm => &CODECS[8],
        CodecId::Bmp => &CODECS[9],
        CodecId::Targa => &CODECS[10],
        CodecId::Png => &CODECS[11],
    }
}

#[must_use]
pub fn find_by_name(name: &str) -> Option<CodecId> {
    CODECS
        .iter()
        .find(|descriptor| descriptor.name.eq_ignore_ascii_case(name))
        .map(|descriptor| descriptor.id)
}

#[must_use]
pub const fn is_pcm(id: CodecId) -> bool {
    matches!(
        id,
        CodecId::PcmU8
            | CodecId::PcmS16Le
            | CodecId::PcmS24Le
            | CodecId::PcmS32Le
            | CodecId::PcmF32Le
            | CodecId::PcmF64Le
    )
}

#[must_use]
pub const fn pcm_from_wave_tag(format_tag: u16, bits_per_sample: u16) -> Option<CodecId> {
    match (format_tag, bits_per_sample) {
        (1, 8) => Some(CodecId::PcmU8),
        (1, 16) => Some(CodecId::PcmS16Le),
        (1, 24) => Some(CodecId::PcmS24Le),
        (1, 32) => Some(CodecId::PcmS32Le),
        (3, 32) => Some(CodecId::PcmF32Le),
        (3, 64) => Some(CodecId::PcmF64Le),
        _ => None,
    }
}

#[must_use]
pub const fn pcm_bits_per_sample(id: CodecId) -> Option<u16> {
    match id {
        CodecId::PcmU8 => Some(8),
        CodecId::PcmS16Le => Some(16),
        CodecId::PcmS24Le => Some(24),
        CodecId::PcmS32Le | CodecId::PcmF32Le => Some(32),
        CodecId::PcmF64Le => Some(64),
        CodecId::Pbm
        | CodecId::Pgm
        | CodecId::Ppm
        | CodecId::Bmp
        | CodecId::Targa
        | CodecId::Png => None,
    }
}

#[must_use]
pub const fn pcm_bytes_per_sample(id: CodecId) -> Option<usize> {
    match pcm_bits_per_sample(id) {
        Some(bits) => Some((bits / 8) as usize),
        None => None,
    }
}

#[must_use]
pub const fn pcm_wave_format_tag(id: CodecId) -> Option<u16> {
    match id {
        CodecId::PcmF32Le | CodecId::PcmF64Le => Some(3),
        CodecId::PcmU8 | CodecId::PcmS16Le | CodecId::PcmS24Le | CodecId::PcmS32Le => Some(1),
        CodecId::Pbm
        | CodecId::Pgm
        | CodecId::Ppm
        | CodecId::Bmp
        | CodecId::Targa
        | CodecId::Png => None,
    }
}

pub fn convert_pcm(input: CodecId, output: CodecId, channels: u16, data: &[u8]) -> Result<Vec<u8>> {
    if channels == 0 {
        return Err(MediaError::invalid_argument(
            "PCM channel count must be non-zero",
        ));
    }

    let input_width = pcm_bytes_per_sample(input)
        .ok_or_else(|| MediaError::invalid_argument("input codec is not PCM"))?;
    let output_width = pcm_bytes_per_sample(output)
        .ok_or_else(|| MediaError::invalid_argument("output codec is not PCM"))?;
    let input_frame_size = input_width
        .checked_mul(usize::from(channels))
        .ok_or_else(|| MediaError::overflow("PCM input frame size overflow"))?;
    if !data.len().is_multiple_of(input_frame_size) {
        return Err(MediaError::invalid_data(
            "PCM input does not contain complete interleaved sample frames",
        ));
    }
    if input == output {
        return Ok(data.to_vec());
    }

    let sample_count = data.len() / input_width;
    let output_size = sample_count
        .checked_mul(output_width)
        .ok_or_else(|| MediaError::overflow("PCM output size overflow"))?;
    let mut converted = Vec::with_capacity(output_size);

    for sample in data.chunks_exact(input_width) {
        let normalized = decode_sample(input, sample)?;
        encode_sample(output, normalized, &mut converted)?;
    }

    Ok(converted)
}

fn decode_sample(codec: CodecId, bytes: &[u8]) -> Result<f64> {
    let value = match codec {
        CodecId::PcmU8 => (f64::from(bytes[0]) - 128.0) / 128.0,
        CodecId::PcmS16Le => f64::from(i16::from_le_bytes([bytes[0], bytes[1]])) / 32_768.0,
        CodecId::PcmS24Le => {
            let extension = if bytes[2] & 0x80 != 0 { 0xFF } else { 0x00 };
            let value = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], extension]);
            f64::from(value) / 8_388_608.0
        }
        CodecId::PcmS32Le => {
            let value = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            f64::from(value) / 2_147_483_648.0
        }
        CodecId::PcmF32Le => {
            let value = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            f64::from(value)
        }
        CodecId::PcmF64Le => f64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]),
        CodecId::Pbm
        | CodecId::Pgm
        | CodecId::Ppm
        | CodecId::Bmp
        | CodecId::Targa
        | CodecId::Png => {
            return Err(MediaError::invalid_argument("codec is not PCM"));
        }
    };
    Ok(value)
}

fn encode_sample(codec: CodecId, sample: f64, output: &mut Vec<u8>) -> Result<()> {
    match codec {
        CodecId::PcmU8 => {
            let sample = sanitize(sample).clamp(-1.0, 1.0);
            let value = if sample >= 1.0 {
                255
            } else if sample <= -1.0 {
                0
            } else {
                (sample.mul_add(128.0, 128.0).round() as i64).clamp(0, 255) as u8
            };
            output.push(value);
        }
        CodecId::PcmS16Le => {
            let value = quantize_signed(sample, 32_768.0, i64::from(i16::MIN), i64::from(i16::MAX));
            output.extend_from_slice(&(value as i16).to_le_bytes());
        }
        CodecId::PcmS24Le => {
            let value = quantize_signed(sample, 8_388_608.0, -8_388_608, 8_388_607) as i32;
            let bytes = value.to_le_bytes();
            output.extend_from_slice(&bytes[..3]);
        }
        CodecId::PcmS32Le => {
            let value = quantize_signed(
                sample,
                2_147_483_648.0,
                i64::from(i32::MIN),
                i64::from(i32::MAX),
            );
            output.extend_from_slice(&(value as i32).to_le_bytes());
        }
        CodecId::PcmF32Le => output.extend_from_slice(&(sanitize(sample) as f32).to_le_bytes()),
        CodecId::PcmF64Le => output.extend_from_slice(&sanitize(sample).to_le_bytes()),
        CodecId::Pbm
        | CodecId::Pgm
        | CodecId::Ppm
        | CodecId::Bmp
        | CodecId::Targa
        | CodecId::Png => {
            return Err(MediaError::invalid_argument("codec is not PCM"));
        }
    }
    Ok(())
}

fn sanitize(sample: f64) -> f64 {
    if sample.is_finite() { sample } else { 0.0 }
}

fn quantize_signed(sample: f64, scale: f64, minimum: i64, maximum: i64) -> i64 {
    let sample = sanitize(sample).clamp(-1.0, 1.0);
    let quantized = if sample <= -1.0 {
        minimum
    } else if sample >= 1.0 {
        maximum
    } else {
        (sample * scale).round() as i64
    };
    quantized.clamp(minimum, maximum)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wave_pcm_mapping_matches_sample_widths() {
        assert_eq!(pcm_from_wave_tag(1, 16), Some(CodecId::PcmS16Le));
        assert_eq!(pcm_from_wave_tag(3, 32), Some(CodecId::PcmF32Le));
        assert_eq!(pcm_from_wave_tag(1, 20), None);
    }

    #[test]
    fn registry_contains_audio_and_video_codecs() {
        for codec in [CodecId::Ppm, CodecId::Bmp, CodecId::Targa, CodecId::Png] {
            assert_eq!(descriptor(codec).media_type, MediaType::Video);
        }
        assert_eq!(descriptor(CodecId::PcmS16Le).media_type, MediaType::Audio);
        assert_eq!(find_by_name("ppm"), Some(CodecId::Ppm));
        assert_eq!(find_by_name("bmp"), Some(CodecId::Bmp));
        assert_eq!(find_by_name("targa"), Some(CodecId::Targa));
        assert_eq!(find_by_name("png"), Some(CodecId::Png));
    }

    #[test]
    fn pcm_metadata_rejects_image_codecs() {
        for codec in [
            CodecId::Pgm,
            CodecId::Ppm,
            CodecId::Bmp,
            CodecId::Targa,
            CodecId::Png,
        ] {
            assert_eq!(pcm_bits_per_sample(codec), None);
            assert_eq!(pcm_bytes_per_sample(codec), None);
            assert_eq!(pcm_wave_format_tag(codec), None);
        }
    }

    #[test]
    fn identical_pcm_conversion_is_bit_exact() {
        let source = [0x00, 0x80, 0xFF, 0x7F];
        assert_eq!(
            convert_pcm(CodecId::PcmS16Le, CodecId::PcmS16Le, 1, &source).unwrap(),
            source
        );
    }

    #[test]
    fn non_pcm_conversion_is_rejected() {
        assert!(convert_pcm(CodecId::Png, CodecId::PcmU8, 1, &[]).is_err());
        assert!(convert_pcm(CodecId::PcmU8, CodecId::Targa, 1, &[]).is_err());
    }

    #[test]
    fn signed_sixteen_converts_to_unsigned_eight() {
        let mut source = Vec::new();
        source.extend_from_slice(&i16::MIN.to_le_bytes());
        source.extend_from_slice(&0_i16.to_le_bytes());
        source.extend_from_slice(&i16::MAX.to_le_bytes());
        let converted = convert_pcm(CodecId::PcmS16Le, CodecId::PcmU8, 1, &source).unwrap();
        assert_eq!(converted, [0, 128, 255]);
    }

    #[test]
    fn float_to_integer_clips_and_sanitizes() {
        let mut source = Vec::new();
        source.extend_from_slice(&(-2.0_f32).to_le_bytes());
        source.extend_from_slice(&f32::NAN.to_le_bytes());
        source.extend_from_slice(&(2.0_f32).to_le_bytes());
        let converted = convert_pcm(CodecId::PcmF32Le, CodecId::PcmS16Le, 1, &source).unwrap();
        let values: Vec<i16> = converted
            .chunks_exact(2)
            .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]))
            .collect();
        assert_eq!(values, [i16::MIN, 0, i16::MAX]);
    }
}
