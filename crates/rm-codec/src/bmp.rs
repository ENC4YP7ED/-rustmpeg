use rm_core::video::{PixelFormat, VideoFrame};
use rm_core::{MediaError, Result};

const FILE_HEADER_SIZE: usize = 14;
const INFO_HEADER_SIZE: usize = 40;
const PIXEL_OFFSET: usize = FILE_HEADER_SIZE + INFO_HEADER_SIZE;
const BI_RGB: u32 = 0;

#[must_use]
pub fn probe_bmp(bytes: &[u8]) -> u8 {
    if bytes.len() >= FILE_HEADER_SIZE && bytes.get(0..2) == Some(b"BM") {
        100
    } else {
        0
    }
}

pub fn decode_bmp(bytes: &[u8]) -> Result<VideoFrame> {
    if probe_bmp(bytes) == 0 {
        return Err(MediaError::invalid_data("input is not a BMP file"));
    }
    if bytes.len() < PIXEL_OFFSET {
        return Err(MediaError::invalid_data(
            "BMP input is shorter than BITMAPINFOHEADER",
        ));
    }

    let declared_file_size = read_u32_le(bytes, 2)?;
    if declared_file_size != 0 {
        let declared = usize::try_from(declared_file_size)
            .map_err(|_| MediaError::overflow("BMP file size exceeds usize"))?;
        if declared > bytes.len() {
            return Err(MediaError::invalid_data(
                "BMP declared file size exceeds available input",
            ));
        }
    }

    let pixel_offset = usize::try_from(read_u32_le(bytes, 10)?)
        .map_err(|_| MediaError::overflow("BMP pixel offset exceeds usize"))?;
    let dib_size = usize::try_from(read_u32_le(bytes, 14)?)
        .map_err(|_| MediaError::overflow("BMP DIB size exceeds usize"))?;
    if dib_size < INFO_HEADER_SIZE {
        return Err(MediaError::unsupported(
            "BMP DIB headers smaller than BITMAPINFOHEADER are not implemented",
        ));
    }
    let dib_end = FILE_HEADER_SIZE
        .checked_add(dib_size)
        .ok_or_else(|| MediaError::overflow("BMP DIB range overflow"))?;
    if dib_end > bytes.len() {
        return Err(MediaError::invalid_data("BMP DIB header is truncated"));
    }
    if pixel_offset < dib_end || pixel_offset > bytes.len() {
        return Err(MediaError::invalid_data("invalid BMP pixel offset"));
    }

    let width_signed = read_i32_le(bytes, 18)?;
    let height_signed = read_i32_le(bytes, 22)?;
    if width_signed <= 0 || height_signed == 0 || height_signed == i32::MIN {
        return Err(MediaError::invalid_data(
            "BMP dimensions must have positive width and non-zero representable height",
        ));
    }
    let width = u32::try_from(width_signed)
        .map_err(|_| MediaError::invalid_data("BMP width is not representable"))?;
    let top_down = height_signed < 0;
    let height = height_signed.unsigned_abs();

    if read_u16_le(bytes, 26)? != 1 {
        return Err(MediaError::invalid_data("BMP planes field must equal 1"));
    }
    let bits_per_pixel = read_u16_le(bytes, 28)?;
    if !matches!(bits_per_pixel, 24 | 32) {
        return Err(MediaError::unsupported(format!(
            "BMP {bits_per_pixel}-bit pixels are not implemented yet"
        )));
    }
    let compression = read_u32_le(bytes, 30)?;
    if compression != BI_RGB {
        return Err(MediaError::unsupported(format!(
            "BMP compression mode {compression} is not implemented yet"
        )));
    }

    let bytes_per_pixel = usize::from(bits_per_pixel / 8);
    let row_bytes = usize::try_from(width)
        .map_err(|_| MediaError::overflow("BMP width exceeds usize"))?
        .checked_mul(bytes_per_pixel)
        .ok_or_else(|| MediaError::overflow("BMP row byte count overflow"))?;
    let row_stride = align_four(row_bytes)?;
    let raster_size = row_stride
        .checked_mul(
            usize::try_from(height)
                .map_err(|_| MediaError::overflow("BMP height exceeds usize"))?,
        )
        .ok_or_else(|| MediaError::overflow("BMP raster size overflow"))?;
    let raster_end = pixel_offset
        .checked_add(raster_size)
        .ok_or_else(|| MediaError::overflow("BMP raster range overflow"))?;
    if raster_end > bytes.len() {
        return Err(MediaError::invalid_data("BMP raster is truncated"));
    }
    if declared_file_size != 0
        && raster_end
            > usize::try_from(declared_file_size)
                .map_err(|_| MediaError::overflow("BMP file size exceeds usize"))?
    {
        return Err(MediaError::invalid_data(
            "BMP raster exceeds declared file size",
        ));
    }

    let pixel_count = usize::try_from(u64::from(width) * u64::from(height))
        .map_err(|_| MediaError::overflow("BMP pixel count exceeds usize"))?;
    let output_len = pixel_count
        .checked_mul(3)
        .ok_or_else(|| MediaError::overflow("BMP RGB output size overflow"))?;
    let mut output = vec![0_u8; output_len];
    let output_stride = usize::try_from(width)
        .map_err(|_| MediaError::overflow("BMP width exceeds usize"))?
        .checked_mul(3)
        .ok_or_else(|| MediaError::overflow("BMP output stride overflow"))?;

    for output_y in 0..usize::try_from(height)
        .map_err(|_| MediaError::overflow("BMP height exceeds usize"))?
    {
        let source_y = if top_down {
            output_y
        } else {
            usize::try_from(height)
                .map_err(|_| MediaError::overflow("BMP height exceeds usize"))?
                - 1
                - output_y
        };
        let source_start = pixel_offset
            .checked_add(
                source_y
                    .checked_mul(row_stride)
                    .ok_or_else(|| MediaError::overflow("BMP source row offset overflow"))?,
            )
            .ok_or_else(|| MediaError::overflow("BMP source row range overflow"))?;
        let destination_start = output_y
            .checked_mul(output_stride)
            .ok_or_else(|| MediaError::overflow("BMP output row offset overflow"))?;

        for x in 0..usize::try_from(width)
            .map_err(|_| MediaError::overflow("BMP width exceeds usize"))?
        {
            let source = source_start
                .checked_add(
                    x.checked_mul(bytes_per_pixel)
                        .ok_or_else(|| MediaError::overflow("BMP source pixel offset overflow"))?,
                )
                .ok_or_else(|| MediaError::overflow("BMP source pixel range overflow"))?;
            let destination = destination_start
                .checked_add(
                    x.checked_mul(3)
                        .ok_or_else(|| MediaError::overflow("BMP output pixel offset overflow"))?,
                )
                .ok_or_else(|| MediaError::overflow("BMP output pixel range overflow"))?;
            output[destination] = bytes[source + 2];
            output[destination + 1] = bytes[source + 1];
            output[destination + 2] = bytes[source];
        }
    }

    VideoFrame::from_vec(width, height, PixelFormat::Rgb24, output)
}

pub fn encode_bmp(frame: &VideoFrame) -> Result<Vec<u8>> {
    if frame.format != PixelFormat::Rgb24 {
        return Err(MediaError::invalid_argument(format!(
            "BMP encoder requires rgb24 input, got {}",
            frame.format.name()
        )));
    }
    if frame.width > i32::MAX as u32 || frame.height > i32::MAX as u32 {
        return Err(MediaError::unsupported(
            "BMP dimensions exceed BITMAPINFOHEADER signed 32-bit limits",
        ));
    }

    let width = usize::try_from(frame.width)
        .map_err(|_| MediaError::overflow("BMP width exceeds usize"))?;
    let height = usize::try_from(frame.height)
        .map_err(|_| MediaError::overflow("BMP height exceeds usize"))?;
    let row_bytes = width
        .checked_mul(3)
        .ok_or_else(|| MediaError::overflow("BMP row byte count overflow"))?;
    let row_stride = align_four(row_bytes)?;
    let image_size = row_stride
        .checked_mul(height)
        .ok_or_else(|| MediaError::overflow("BMP raster size overflow"))?;
    let file_size = PIXEL_OFFSET
        .checked_add(image_size)
        .ok_or_else(|| MediaError::overflow("BMP file size overflow"))?;
    let file_size_u32 = u32::try_from(file_size)
        .map_err(|_| MediaError::unsupported("BMP output exceeds 4 GiB file-size field"))?;
    let image_size_u32 = u32::try_from(image_size)
        .map_err(|_| MediaError::unsupported("BMP raster exceeds 4 GiB image-size field"))?;

    let mut output = Vec::with_capacity(file_size);
    output.extend_from_slice(b"BM");
    output.extend_from_slice(&file_size_u32.to_le_bytes());
    output.extend_from_slice(&0_u16.to_le_bytes());
    output.extend_from_slice(&0_u16.to_le_bytes());
    output.extend_from_slice(&(PIXEL_OFFSET as u32).to_le_bytes());

    output.extend_from_slice(&(INFO_HEADER_SIZE as u32).to_le_bytes());
    output.extend_from_slice(&(frame.width as i32).to_le_bytes());
    output.extend_from_slice(&(frame.height as i32).to_le_bytes());
    output.extend_from_slice(&1_u16.to_le_bytes());
    output.extend_from_slice(&24_u16.to_le_bytes());
    output.extend_from_slice(&BI_RGB.to_le_bytes());
    output.extend_from_slice(&image_size_u32.to_le_bytes());
    output.extend_from_slice(&0_i32.to_le_bytes());
    output.extend_from_slice(&0_i32.to_le_bytes());
    output.extend_from_slice(&0_u32.to_le_bytes());
    output.extend_from_slice(&0_u32.to_le_bytes());

    let padding = row_stride - row_bytes;
    for y in (0..frame.height).rev() {
        let row = frame.row(y)?;
        for pixel in row.chunks_exact(3) {
            output.extend_from_slice(&[pixel[2], pixel[1], pixel[0]]);
        }
        output.resize(output.len() + padding, 0);
    }

    debug_assert_eq!(output.len(), file_size);
    Ok(output)
}

fn align_four(value: usize) -> Result<usize> {
    value
        .checked_add(3)
        .map(|value| value & !3)
        .ok_or_else(|| MediaError::overflow("BMP row alignment overflow"))
}

fn read_u16_le(bytes: &[u8], offset: usize) -> Result<u16> {
    let value = bytes
        .get(offset..offset + 2)
        .ok_or_else(|| MediaError::eof("unexpected end of BMP header"))?;
    Ok(u16::from_le_bytes([value[0], value[1]]))
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Result<u32> {
    let value = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| MediaError::eof("unexpected end of BMP header"))?;
    Ok(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
}

fn read_i32_le(bytes: &[u8], offset: usize) -> Result<i32> {
    let value = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| MediaError::eof("unexpected end of BMP header"))?;
    Ok(i32::from_le_bytes([value[0], value[1], value[2], value[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame_2x2() -> VideoFrame {
        VideoFrame::from_vec(
            2,
            2,
            PixelFormat::Rgb24,
            vec![
                255, 0, 0, 0, 255, 0, // top: red, green
                0, 0, 255, 255, 255, 255, // bottom: blue, white
            ],
        )
        .unwrap()
    }

    #[test]
    fn canonical_bmp_round_trip_preserves_rgb_and_orientation() {
        let frame = frame_2x2();
        let encoded = encode_bmp(&frame).unwrap();
        assert_eq!(probe_bmp(&encoded), 100);
        let decoded = decode_bmp(&encoded).unwrap();
        assert_eq!(decoded, frame);
        assert_eq!(encoded.len(), 70);
    }

    #[test]
    fn top_down_24_bit_bmp_decodes_without_vertical_flip() {
        let mut encoded = encode_bmp(&frame_2x2()).unwrap();
        encoded[22..26].copy_from_slice(&(-2_i32).to_le_bytes());
        let raster = encoded[54..].to_vec();
        let stride = 8;
        encoded[54..54 + stride].copy_from_slice(&raster[stride..stride * 2]);
        encoded[54 + stride..54 + stride * 2].copy_from_slice(&raster[..stride]);
        assert_eq!(decode_bmp(&encoded).unwrap(), frame_2x2());
    }

    #[test]
    fn uncompressed_32_bit_bgra_is_decoded_to_rgb24() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"BM");
        bytes.extend_from_slice(&58_u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 4]);
        bytes.extend_from_slice(&54_u32.to_le_bytes());
        bytes.extend_from_slice(&40_u32.to_le_bytes());
        bytes.extend_from_slice(&1_i32.to_le_bytes());
        bytes.extend_from_slice(&1_i32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&32_u16.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&4_u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 16]);
        bytes.extend_from_slice(&[3, 2, 1, 99]);
        let frame = decode_bmp(&bytes).unwrap();
        assert_eq!(frame.data.as_slice(), &[1, 2, 3]);
    }

    #[test]
    fn malformed_offsets_sizes_and_compression_are_rejected() {
        let valid = encode_bmp(&frame_2x2()).unwrap();

        let mut truncated = valid.clone();
        truncated.pop();
        assert!(decode_bmp(&truncated).is_err());

        let mut bad_offset = valid.clone();
        bad_offset[10..14].copy_from_slice(&10_u32.to_le_bytes());
        assert!(decode_bmp(&bad_offset).is_err());

        let mut compressed = valid.clone();
        compressed[30..34].copy_from_slice(&1_u32.to_le_bytes());
        assert!(decode_bmp(&compressed).is_err());

        let mut bad_planes = valid.clone();
        bad_planes[26..28].copy_from_slice(&2_u16.to_le_bytes());
        assert!(decode_bmp(&bad_planes).is_err());
    }

    #[test]
    fn encoder_rejects_non_rgb_frames() {
        let gray = VideoFrame::from_vec(1, 1, PixelFormat::Gray8, vec![0]).unwrap();
        assert!(encode_bmp(&gray).is_err());
    }
}
