#![forbid(unsafe_code)]

use rm_core::video::{PixelFormat, VideoFrame};
use rm_core::{MediaError, Result};

pub fn convert_pixel_format(frame: &VideoFrame, target: PixelFormat) -> Result<VideoFrame> {
    if frame.format == target {
        return Ok(frame.clone());
    }

    match (frame.format, target) {
        (PixelFormat::Gray8, PixelFormat::Rgb24) => gray_to_rgb(frame),
        (PixelFormat::Rgb24, PixelFormat::Gray8) => rgb_to_gray(frame),
        _ => Err(MediaError::unsupported(format!(
            "pixel conversion {} -> {} is not implemented",
            frame.format.name(),
            target.name()
        ))),
    }
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

fn gray_to_rgb(frame: &VideoFrame) -> Result<VideoFrame> {
    let capacity = frame
        .data
        .len()
        .checked_mul(3)
        .ok_or_else(|| MediaError::overflow("RGB conversion size overflow"))?;
    let mut output = Vec::with_capacity(capacity);
    for &value in frame.data.as_slice() {
        output.extend_from_slice(&[value, value, value]);
    }
    VideoFrame::from_vec(frame.width, frame.height, PixelFormat::Rgb24, output)
}

fn rgb_to_gray(frame: &VideoFrame) -> Result<VideoFrame> {
    let mut output = Vec::with_capacity(
        usize::try_from(frame.pixel_count())
            .map_err(|_| MediaError::overflow("gray conversion size exceeds usize"))?,
    );
    for pixel in frame.data.as_slice().chunks_exact(3) {
        let red = u32::from(pixel[0]);
        let green = u32::from(pixel[1]);
        let blue = u32::from(pixel[2]);
        let luma = (77 * red + 150 * green + 29 * blue + 128) >> 8;
        output.push(
            u8::try_from(luma).map_err(|_| MediaError::overflow("computed luma exceeds u8"))?,
        );
    }
    VideoFrame::from_vec(frame.width, frame.height, PixelFormat::Gray8, output)
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
