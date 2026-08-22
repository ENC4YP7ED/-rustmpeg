use crate::{Buffer, MediaError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PixelFormat {
    Gray8,
    GrayAlpha8,
    Rgb24,
    Rgba32,
}

impl PixelFormat {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Gray8 => "gray",
            Self::GrayAlpha8 => "ya8",
            Self::Rgb24 => "rgb24",
            Self::Rgba32 => "rgba",
        }
    }

    #[must_use]
    pub const fn bytes_per_pixel(self) -> usize {
        match self {
            Self::Gray8 => 1,
            Self::GrayAlpha8 => 2,
            Self::Rgb24 => 3,
            Self::Rgba32 => 4,
        }
    }

    #[must_use]
    pub fn packed_row_bytes(self, width: u32) -> Option<usize> {
        usize::try_from(width)
            .ok()?
            .checked_mul(self.bytes_per_pixel())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    pub format: PixelFormat,
    pub data: Buffer,
    pub linesize: usize,
    pub pts: Option<i64>,
}

impl VideoFrame {
    pub fn packed(width: u32, height: u32, format: PixelFormat, data: Buffer) -> Result<Self> {
        if width == 0 || height == 0 {
            return Err(MediaError::invalid_argument(
                "video frame dimensions must be non-zero",
            ));
        }

        let width = usize::try_from(width)
            .map_err(|_| MediaError::overflow("video frame width exceeds usize"))?;
        let height = usize::try_from(height)
            .map_err(|_| MediaError::overflow("video frame height exceeds usize"))?;
        let linesize = width
            .checked_mul(format.bytes_per_pixel())
            .ok_or_else(|| MediaError::overflow("video frame linesize overflow"))?;
        let expected = linesize
            .checked_mul(height)
            .ok_or_else(|| MediaError::overflow("video frame buffer size overflow"))?;

        if data.len() != expected {
            return Err(MediaError::invalid_data(format!(
                "video frame buffer has {} bytes but {expected} are required",
                data.len()
            )));
        }

        Ok(Self {
            width: u32::try_from(width)
                .map_err(|_| MediaError::overflow("video frame width exceeds u32"))?,
            height: u32::try_from(height)
                .map_err(|_| MediaError::overflow("video frame height exceeds u32"))?,
            format,
            data,
            linesize,
            pts: None,
        })
    }

    pub fn from_vec(width: u32, height: u32, format: PixelFormat, data: Vec<u8>) -> Result<Self> {
        Self::packed(width, height, format, Buffer::from_vec(data))
    }

    #[must_use]
    pub fn pixel_count(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }

    pub fn row(&self, y: u32) -> Result<&[u8]> {
        if y >= self.height {
            return Err(MediaError::invalid_argument(
                "video frame row is out of bounds",
            ));
        }
        let start = usize::try_from(y)
            .map_err(|_| MediaError::overflow("video frame row index exceeds usize"))?
            .checked_mul(self.linesize)
            .ok_or_else(|| MediaError::overflow("video frame row offset overflow"))?;
        Ok(&self.data.as_slice()[start..start + self.linesize])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_rgb_frame_validates_exact_storage() {
        let frame = VideoFrame::from_vec(2, 2, PixelFormat::Rgb24, vec![0; 12]).unwrap();
        assert_eq!(frame.linesize, 6);
        assert_eq!(frame.pixel_count(), 4);
        assert_eq!(frame.row(1).unwrap(), &[0; 6]);
    }

    #[test]
    fn packed_alpha_frames_validate_exact_storage() {
        let gray_alpha =
            VideoFrame::from_vec(2, 1, PixelFormat::GrayAlpha8, vec![7, 8, 9, 10]).unwrap();
        assert_eq!(gray_alpha.linesize, 4);
        assert_eq!(gray_alpha.row(0).unwrap(), &[7, 8, 9, 10]);

        let rgba =
            VideoFrame::from_vec(2, 1, PixelFormat::Rgba32, vec![1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
        assert_eq!(rgba.linesize, 8);
        assert_eq!(rgba.row(0).unwrap(), &[1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn packed_row_size_is_checked() {
        assert_eq!(PixelFormat::Gray8.packed_row_bytes(4), Some(4));
        assert_eq!(PixelFormat::GrayAlpha8.packed_row_bytes(4), Some(8));
        assert_eq!(PixelFormat::Rgb24.packed_row_bytes(4), Some(12));
        assert_eq!(PixelFormat::Rgba32.packed_row_bytes(4), Some(16));
    }

    #[test]
    fn malformed_storage_is_rejected() {
        assert!(VideoFrame::from_vec(2, 2, PixelFormat::Rgb24, vec![0; 11]).is_err());
        assert!(VideoFrame::from_vec(0, 2, PixelFormat::Gray8, Vec::new()).is_err());
        assert!(VideoFrame::from_vec(1, 1, PixelFormat::Rgba32, vec![0; 3]).is_err());
    }

    #[test]
    fn row_bounds_are_checked() {
        let frame = VideoFrame::from_vec(1, 1, PixelFormat::Gray8, vec![7]).unwrap();
        assert_eq!(frame.row(0).unwrap(), &[7]);
        assert!(frame.row(1).is_err());
    }
}
