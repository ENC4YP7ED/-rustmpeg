use std::ffi::OsStr;
use std::path::Path;

use rm_codec::bmp::{decode_bmp, encode_bmp, probe_bmp};
use rm_codec::jpeg::{parse_jpeg, probe_jpeg};
use rm_codec::jpeg_decode::decode_jpeg;
use rm_codec::jpeg_encode::encode_jpeg;
use rm_codec::png::{decode_png, encode_png, probe_png};
use rm_codec::pnm::{PnmImage, PnmKind, decode_pnm, encode_pnm, probe_pnm};
use rm_codec::tga::{decode_tga, encode_tga, probe_tga};
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
        .max(probe_bmp(bytes))
        .max(probe_tga(bytes))
        .max(probe_png(bytes))
        .max(probe_jpeg(bytes))
}

pub fn decode_image(bytes: &[u8]) -> Result<DecodedImage> {
    let pnm_score = probe_pnm(bytes);
    let bmp_score = probe_bmp(bytes);
    let tga_score = probe_tga(bytes);
    let png_score = probe_png(bytes);
    let jpeg_score = probe_jpeg(bytes);
    let best = pnm_score
        .max(bmp_score)
        .max(tga_score)
        .max(png_score)
        .max(jpeg_score);

    if best == 0 {
        return Err(MediaError::invalid_data(
            "input is not a supported image (PBM/PGM/PPM/BMP/TGA/PNG/JPEG)",
        ));
    }

    if jpeg_score == best {
        let _ = parse_jpeg(bytes)?;
        return Ok(DecodedImage {
            codec: CodecId::Jpeg,
            frame: decode_jpeg(bytes)?,
        });
    }
    if png_score == best {
        return Ok(DecodedImage {
            codec: CodecId::Png,
            frame: decode_png(bytes)?,
        });
    }
    if bmp_score == best {
        return Ok(DecodedImage {
            codec: CodecId::Bmp,
            frame: decode_bmp(bytes)?,
        });
    }
    if pnm_score == best {
        let PnmImage { kind, frame } = decode_pnm(bytes)?;
        return Ok(DecodedImage {
            codec: codec_for_kind(kind),
            frame,
        });
    }

    Ok(DecodedImage {
        codec: CodecId::Targa,
        frame: decode_tga(bytes)?,
    })
}

pub fn encode_image(codec: CodecId, frame: &VideoFrame) -> Result<Vec<u8>> {
    match codec {
        CodecId::Pbm | CodecId::Pgm | CodecId::Ppm => {
            let kind = pnm_kind_for_codec(codec).expect("PNM codec must map to PNM kind");
            let target_format = pixel_format_for_codec(codec)?;
            let converted = convert_pixel_format(frame, target_format)?;
            encode_pnm(kind, &converted)
        }
        CodecId::Bmp => {
            let converted = convert_pixel_format(frame, PixelFormat::Rgb24)?;
            encode_bmp(&converted)
        }
        CodecId::Targa => encode_tga(frame),
        CodecId::Png => encode_png(frame),
        CodecId::Jpeg => encode_jpeg(frame),
        CodecId::PcmU8
        | CodecId::PcmS16Le
        | CodecId::PcmS24Le
        | CodecId::PcmS32Le
        | CodecId::PcmF32Le
        | CodecId::PcmF64Le => Err(MediaError::invalid_argument(
            "selected codec is not an image codec",
        )),
    }
}

pub fn prepare_frame(
    frame: &VideoFrame,
    codec: CodecId,
    size: Option<(u32, u32)>,
) -> Result<VideoFrame> {
    let target_format = match codec {
        CodecId::Targa | CodecId::Png => frame.format,
        CodecId::Jpeg => {
            if frame.format == PixelFormat::Gray8 {
                PixelFormat::Gray8
            } else {
                PixelFormat::Rgb24
            }
        }
        _ => pixel_format_for_codec(codec)?,
    };
    let converted = convert_pixel_format(frame, target_format)?;
    match size {
        Some((width, height)) => scale_nearest(&converted, width, height),
        None => Ok(converted),
    }
}

#[must_use]
pub fn is_image_codec(codec: CodecId) -> bool {
    descriptor(codec).media_type == MediaType::Video
        && matches!(
            codec,
            CodecId::Pbm
                | CodecId::Pgm
                | CodecId::Ppm
                | CodecId::Bmp
                | CodecId::Targa
                | CodecId::Png
                | CodecId::Jpeg
        )
}

#[must_use]
pub fn codec_for_path(path: &Path) -> Option<CodecId> {
    let extension = path.extension().and_then(OsStr::to_str)?;
    if extension.eq_ignore_ascii_case("pbm") {
        Some(CodecId::Pbm)
    } else if extension.eq_ignore_ascii_case("pgm") {
        Some(CodecId::Pgm)
    } else if extension.eq_ignore_ascii_case("ppm") || extension.eq_ignore_ascii_case("pnm") {
        Some(CodecId::Ppm)
    } else if extension.eq_ignore_ascii_case("bmp") || extension.eq_ignore_ascii_case("dib") {
        Some(CodecId::Bmp)
    } else if extension.eq_ignore_ascii_case("tga") || extension.eq_ignore_ascii_case("targa") {
        Some(CodecId::Targa)
    } else if extension.eq_ignore_ascii_case("png") {
        Some(CodecId::Png)
    } else if extension.eq_ignore_ascii_case("jpg")
        || extension.eq_ignore_ascii_case("jpeg")
        || extension.eq_ignore_ascii_case("jpe")
    {
        Some(CodecId::Jpeg)
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
        CodecId::Bmp
        | CodecId::Targa
        | CodecId::Png
        | CodecId::Jpeg
        | CodecId::PcmU8
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
        CodecId::Ppm | CodecId::Bmp | CodecId::Targa | CodecId::Png | CodecId::Jpeg => {
            Ok(PixelFormat::Rgb24)
        }
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
        assert_eq!(codec_for_path(Path::new("x.BMP")), Some(CodecId::Bmp));
        assert_eq!(codec_for_path(Path::new("x.tga")), Some(CodecId::Targa));
        assert_eq!(codec_for_path(Path::new("x.PNG")), Some(CodecId::Png));
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
    fn bmp_round_trip_uses_rgb24() {
        let frame = VideoFrame::from_vec(1, 1, PixelFormat::Gray8, vec![91]).unwrap();
        let encoded = encode_image(CodecId::Bmp, &frame).unwrap();
        let decoded = decode_image(&encoded).unwrap();
        assert_eq!(decoded.codec, CodecId::Bmp);
        assert_eq!(decoded.frame.format, PixelFormat::Rgb24);
        assert_eq!(decoded.frame.data.as_slice(), &[91, 91, 91]);
    }

    #[test]
    fn targa_preserves_grayscale_frames() {
        let frame = VideoFrame::from_vec(2, 1, PixelFormat::Gray8, vec![12, 240]).unwrap();
        let prepared = prepare_frame(&frame, CodecId::Targa, None).unwrap();
        assert_eq!(prepared.format, PixelFormat::Gray8);
        let encoded = encode_image(CodecId::Targa, &prepared).unwrap();
        let decoded = decode_image(&encoded).unwrap();
        assert_eq!(decoded.codec, CodecId::Targa);
        assert_eq!(decoded.frame, frame);
    }

    #[test]
    fn png_preserves_grayscale_frames() {
        let frame = VideoFrame::from_vec(2, 1, PixelFormat::Gray8, vec![12, 240]).unwrap();
        let prepared = prepare_frame(&frame, CodecId::Png, None).unwrap();
        assert_eq!(prepared.format, PixelFormat::Gray8);
        let encoded = encode_image(CodecId::Png, &prepared).unwrap();
        let decoded = decode_image(&encoded).unwrap();
        assert_eq!(decoded.codec, CodecId::Png);
        assert_eq!(decoded.frame, frame);
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
