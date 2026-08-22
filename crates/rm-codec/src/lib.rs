#![forbid(unsafe_code)]

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

const PCM_CODECS: [CodecDescriptor; 6] = [
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
];

#[must_use]
pub const fn codecs() -> &'static [CodecDescriptor] {
    &PCM_CODECS
}

#[must_use]
pub fn descriptor(id: CodecId) -> &'static CodecDescriptor {
    PCM_CODECS
        .iter()
        .find(|descriptor| descriptor.id == id)
        .expect("every CodecId must have a descriptor")
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
pub const fn pcm_bits_per_sample(id: CodecId) -> u16 {
    match id {
        CodecId::PcmU8 => 8,
        CodecId::PcmS16Le => 16,
        CodecId::PcmS24Le => 24,
        CodecId::PcmS32Le | CodecId::PcmF32Le => 32,
        CodecId::PcmF64Le => 64,
    }
}

#[must_use]
pub const fn pcm_wave_format_tag(id: CodecId) -> u16 {
    match id {
        CodecId::PcmF32Le | CodecId::PcmF64Le => 3,
        CodecId::PcmU8 | CodecId::PcmS16Le | CodecId::PcmS24Le | CodecId::PcmS32Le => 1,
    }
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
    fn every_codec_has_a_stable_ffmpeg_style_name() {
        for codec in codecs() {
            assert!(!codec.name.is_empty());
            assert!(codec.can_decode);
            assert!(codec.can_encode);
        }
    }
}
