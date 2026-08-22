#![forbid(unsafe_code)]

use rm_core::video::{PixelFormat, VideoFrame};
use rm_core::{MediaError, Result};

pub fn convert_pixel_format(frame: &VideoFrame, target: PixelFormat) -> Result<VideoFrame> {
    if frame.format == target {
        return Ok(frame.clone());
    }

    let pixel_count = usize::try_from(frame.pixel_count())
        .map_err(|_| MediaError::overflow("pixel conversion size exceeds usize"))?;
    let output_len = pixel_count
        .checked_mul(target.bytes_per_pixel())
        .ok_or_else(|| MediaError::overflow("pixel conversion output size overflow"))?;
    let mut output = Vec::with_capacity(output_len);

    for index in 0..pixel_count {
        let (gray, red, green, blue, alpha) = read_pixel(frame, index)?;
        match target {
            PixelFormat::Gray8 => output.push(gray),
            PixelFormat::GrayAlpha8 => output.extend_from_slice(&[gray, alpha]),
            PixelFormat::Rgb24 => output.extend_from_slice(&[red, green, blue]),
            PixelFormat::Rgba32 => output.extend_from_slice(&[red, green, blue, alpha]),
        }
    }

    VideoFrame::from_vec(frame.width, frame.height, target, output)
}

fn read_pixel(frame: &VideoFrame, index: usize) -> Result<(u8, u8, u8, u8, u8)> {
    let offset = index
        .checked_mul(frame.format.bytes_per_pixel())
        .ok_or_else(|| MediaError::overflow("pixel conversion source offset overflow"))?;
    let data = frame.data.as_slice();
    let pixel = data
        .get(offset..offset + frame.format.bytes_per_pixel())
        .ok_or_else(|| MediaError::invalid_data("pixel conversion source frame is truncated"))?;

    Ok(match frame.format {
        PixelFormat::Gray8 => {
            let gray = pixel[0];
            (gray, gray, gray, gray, 255)
        }
        PixelFormat::GrayAlpha8 => {
            let gray = pixel[0];
            (gray, gray, gray, gray, pixel[1])
        }
        PixelFormat::Rgb24 => {
            let gray = luma(pixel[0], pixel[1], pixel[2])?;
            (gray, pixel[0], pixel[1], pixel[2], 255)
        }
        PixelFormat::Rgba32 => {
            let gray = luma(pixel[0], pixel[1], pixel[2])?;
            (gray, pixel[0], pixel[1], pixel[2], pixel[3])
        }
    })
}

fn luma(red: u8, green: u8, blue: u8) -> Result<u8> {
    let value = (77 * u32::from(red) + 150 * u32::from(green) + 29 * u32::from(blue) + 128) >> 8;
    u8::try_from(value).map_err(|_| MediaError::overflow("computed luma exceeds u8"))
}

pub fn scale_nearest(frame: &VideoFrame, width: u32, height: u32) -> Result<VideoFrame> {
    if width == 0 || height == 0 {
        return Err(MediaError::invalid_argument(
            "scaled dimensions must be non-zero",
        ));
    }
    if frame.width == width && frame.height == height {
        return Ok(frame.clone());
    }

    let bytes_per_pixel = frame.format.bytes_per_pixel();
    let target_width =
        usize::try_from(width).map_err(|_| MediaError::overflow("target width exceeds usize"))?;
    let target_height =
        usize::try_from(height).map_err(|_| MediaError::overflow("target height exceeds usize"))?;
    let output_len = target_width
        .checked_mul(target_height)
        .and_then(|pixels| pixels.checked_mul(bytes_per_pixel))
        .ok_or_else(|| MediaError::overflow("scaled frame size overflow"))?;
    let mut output = vec![0_u8; output_len];

    for y in 0..target_height {
        let source_y = y
            .checked_mul(
                usize::try_from(frame.height)
                    .map_err(|_| MediaError::overflow("source height exceeds usize"))?,
            )
            .ok_or_else(|| MediaError::overflow("vertical scale coordinate overflow"))?
            / target_height;
        let source_row = frame.row(
            u32::try_from(source_y).map_err(|_| MediaError::overflow("source row exceeds u32"))?,
        )?;
        let output_row_start = y
            .checked_mul(target_width)
            .and_then(|pixels| pixels.checked_mul(bytes_per_pixel))
            .ok_or_else(|| MediaError::overflow("output row offset overflow"))?;

        for x in 0..target_width {
            let source_x = x
                .checked_mul(
                    usize::try_from(frame.width)
                        .map_err(|_| MediaError::overflow("source width exceeds usize"))?,
                )
                .ok_or_else(|| MediaError::overflow("horizontal scale coordinate overflow"))?
                / target_width;
            let source_offset = source_x
                .checked_mul(bytes_per_pixel)
                .ok_or_else(|| MediaError::overflow("source pixel offset overflow"))?;
            let output_offset = output_row_start
                .checked_add(
                    x.checked_mul(bytes_per_pixel)
                        .ok_or_else(|| MediaError::overflow("output pixel offset overflow"))?,
                )
                .ok_or_else(|| MediaError::overflow("output pixel offset overflow"))?;
            output[output_offset..output_offset + bytes_per_pixel]
                .copy_from_slice(&source_row[source_offset..source_offset + bytes_per_pixel]);
        }
    }

    VideoFrame::from_vec(width, height, frame.format, output)
}

pub fn convert_and_scale(
    frame: &VideoFrame,
    target_format: PixelFormat,
    width: u32,
    height: u32,
) -> Result<VideoFrame> {
    let converted = convert_pixel_format(frame, target_format)?;
    scale_nearest(&converted, width, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gray_to_rgb_replicates_channels() {
        let frame = VideoFrame::from_vec(2, 1, PixelFormat::Gray8, vec![0, 200]).unwrap();
        let rgb = convert_pixel_format(&frame, PixelFormat::Rgb24).unwrap();
        assert_eq!(rgb.data.as_slice(), &[0, 0, 0, 200, 200, 200]);
    }

    #[test]
    fn rgb_to_gray_uses_deterministic_integer_luma() {
        let frame = VideoFrame::from_vec(
            3,
            1,
            PixelFormat::Rgb24,
            vec![255, 0, 0, 0, 255, 0, 0, 0, 255],
        )
        .unwrap();
        let gray = convert_pixel_format(&frame, PixelFormat::Gray8).unwrap();
        assert_eq!(gray.data.as_slice(), &[77, 149, 29]);
    }

    #[test]
    fn alpha_is_preserved_when_target_supports_it() {
        let frame = VideoFrame::from_vec(
            2,
            1,
            PixelFormat::Rgba32,
            vec![255, 0, 0, 17, 0, 255, 0, 231],
        )
        .unwrap();
        let gray_alpha = convert_pixel_format(&frame, PixelFormat::GrayAlpha8).unwrap();
        assert_eq!(gray_alpha.data.as_slice(), &[77, 17, 149, 231]);
        let round_trip = convert_pixel_format(&gray_alpha, PixelFormat::Rgba32).unwrap();
        assert_eq!(
            round_trip.data.as_slice(),
            &[77, 77, 77, 17, 149, 149, 149, 231]
        );
    }

    #[test]
    fn opaque_alpha_is_inserted_for_non_alpha_sources() {
        let frame = VideoFrame::from_vec(1, 1, PixelFormat::Rgb24, vec![1, 2, 3]).unwrap();
        let rgba = convert_pixel_format(&frame, PixelFormat::Rgba32).unwrap();
        assert_eq!(rgba.data.as_slice(), &[1, 2, 3, 255]);
    }

    #[test]
    fn nearest_scale_preserves_rgba_pixels() {
        let frame =
            VideoFrame::from_vec(2, 1, PixelFormat::Rgba32, vec![1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
        let scaled = scale_nearest(&frame, 4, 1).unwrap();
        assert_eq!(
            scaled.data.as_slice(),
            &[1, 2, 3, 4, 1, 2, 3, 4, 5, 6, 7, 8, 5, 6, 7, 8]
        );
    }

    #[test]
    fn nearest_scale_preserves_corner_mapping() {
        let frame = VideoFrame::from_vec(2, 2, PixelFormat::Gray8, vec![1, 2, 3, 4]).unwrap();
        let scaled = scale_nearest(&frame, 4, 4).unwrap();
        assert_eq!(
            scaled.data.as_slice(),
            &[1, 1, 2, 2, 1, 1, 2, 2, 3, 3, 4, 4, 3, 3, 4, 4]
        );
    }

    #[test]
    fn zero_sized_scale_is_rejected() {
        let frame = VideoFrame::from_vec(1, 1, PixelFormat::Gray8, vec![0]).unwrap();
        assert!(scale_nearest(&frame, 0, 1).is_err());
    }
}
