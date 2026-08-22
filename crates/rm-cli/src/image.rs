use std::ffi::OsStr;
use std::path::Path;

use rm_codec::pnm::{PnmImage, PnmKind, decode_pnm, encode_pnm, probe_pnm};
use rm_codec::{CodecId, MediaType, descriptor};
use rm_core::video::{PixelFormat, VideoFrame};
use rm_core::{MediaError, Result};
use rm_scale::{convert_pixel_format, scale_nearest};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedImage {
    pub codec: CodecId,
    pub frame: VideoFrame,
}

#[must_use]
pub fn probe_image(bytes: &[u8]) -> u8 {
    probe_pnm(bytes)
}

pub fn decode_image(bytes: &[u8]) -> Result<DecodedImage> {
    let PnmImage { kind, frame } = decode_pnm(bytes)?;
    Ok(DecodedImage {
        codec: codec_for_kind(kind),
        frame,
    })
}

pub fn encode_image(codec: CodecId, frame: &VideoFrame) -> Result<Vec<u8>> {
    let kind = pnm_kind_for_codec(codec)
        .ok_or_else(|| MediaError::invalid_argument("selected codec is not an image codec"))?;
    let target_format = pixel_format_for_codec(codec)?;
    let converted = convert_pixel_format(frame, target_format)?;
    encode_pnm(kind, &converted)
}

pub fn prepare_frame(
    frame: &VideoFrame,
    codec: CodecId,
    size: Option<(u32, u32)>,
) -> Result<VideoFrame> {
    let target_format = pixel_format_for_codec(codec)?;
    let converted = convert_pixel_format(frame, target_format)?;
    match size {
        Some((width, height)) => scale_nearest(&converted, width, height),
        None => Ok(converted),
    }
}

#[must_use]
pub fn is_image_codec(codec: CodecId) -> bool {
    descriptor(codec).media_type == MediaType::Video
        && matches!(codec, CodecId::Pbm | CodecId::Pgm | CodecId::Ppm)
}

#[must_use]
pub fn codec_for_path(path: &Path) -> Option<CodecId> {
    let extension = path.extension().and_then(OsStr::to_str)?;
    if extension.eq_ignore_ascii_case("pbm") {
        Some(CodecId::Pbm)
    } else if extension.eq_ignore_ascii_case("pgm") {
        Some(CodecId::Pgm)
    } else if extension.eq_ignore_ascii_case("ppm") {
        Some(CodecId::Ppm)
    } else {
        None
    }
}

#[must_use]
pub const fn codec_for_kind(kind: PnmKind) -> CodecId {
    match kind {
        PnmKind::Pbm => CodecId::Pbm,
        PnmKind::Pgm => CodecId::Pgm,
        PnmKind::Ppm => CodecId::Ppm,
    }
}

#[must_use]
pub const fn pnm_kind_for_codec(codec: CodecId) -> Option<PnmKind> {
    match codec {
        CodecId::Pbm => Some(PnmKind::Pbm),
        CodecId::Pgm => Some(PnmKind::Pgm),
        CodecId::Ppm => Some(PnmKind::Ppm),
        CodecId::PcmU8
        | CodecId::PcmS16Le
        | CodecId::PcmS24Le
        | CodecId::PcmS32Le
        | CodecId::PcmF32Le
        | CodecId::PcmF64Le => None,
    }
}

pub fn pixel_format_for_codec(codec: CodecId) -> Result<PixelFormat> {
    match codec {
        CodecId::Pbm | CodecId::Pgm => Ok(PixelFormat::Gray8),
        CodecId::Ppm => Ok(PixelFormat::Rgb24),
        CodecId::PcmU8
        | CodecId::PcmS16Le
        | CodecId::PcmS24Le
        | CodecId::PcmS32Le
        | CodecId::PcmF32Le
        | CodecId::PcmF64Le => Err(MediaError::invalid_argument(
            "audio codec has no packed image pixel format",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_extension_selects_expected_image_codec() {
        assert_eq!(codec_for_path(Path::new("x.PBM")), Some(CodecId::Pbm));
        assert_eq!(codec_for_path(Path::new("x.pgm")), Some(CodecId::Pgm));
        assert_eq!(codec_for_path(Path::new("x.ppm")), Some(CodecId::Ppm));
        assert_eq!(codec_for_path(Path::new("x.wav")), None);
    }

    #[test]
    fn pgm_encoding_converts_rgb_to_gray() {
        let frame = VideoFrame::from_vec(1, 1, PixelFormat::Rgb24, vec![255, 0, 0]).unwrap();
        let encoded = encode_image(CodecId::Pgm, &frame).unwrap();
        let decoded = decode_image(&encoded).unwrap();
        assert_eq!(decoded.codec, CodecId::Pgm);
        assert_eq!(decoded.frame.data.as_slice(), &[77]);
    }

    #[test]
    fn prepare_frame_scales_and_converts() {
        let frame = VideoFrame::from_vec(1, 1, PixelFormat::Gray8, vec![123]).unwrap();
        let output = prepare_frame(&frame, CodecId::Ppm, Some((2, 2))).unwrap();
        assert_eq!(output.format, PixelFormat::Rgb24);
        assert_eq!(output.width, 2);
        assert_eq!(output.height, 2);
        assert_eq!(output.data.as_slice(), &[123; 12]);
    }
}
