use rm_core::{MediaError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PngPrediction {
    None,
    Sub,
    Up,
    Average,
    Paeth,
    Mixed,
}

impl PngPrediction {
    #[must_use]
    pub const fn filter_type(self) -> Option<u8> {
        match self {
            Self::None => Some(0),
            Self::Sub => Some(1),
            Self::Up => Some(2),
            Self::Average => Some(3),
            Self::Paeth => Some(4),
            Self::Mixed => None,
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        if value.eq_ignore_ascii_case("none") || value == "0" {
            Ok(Self::None)
        } else if value.eq_ignore_ascii_case("sub") || value == "1" {
            Ok(Self::Sub)
        } else if value.eq_ignore_ascii_case("up") || value == "2" {
            Ok(Self::Up)
        } else if value.eq_ignore_ascii_case("avg") || value.eq_ignore_ascii_case("average") || value == "3" {
            Ok(Self::Average)
        } else if value.eq_ignore_ascii_case("paeth") || value == "4" {
            Ok(Self::Paeth)
        } else if value.eq_ignore_ascii_case("mixed") || value == "5" {
            Ok(Self::Mixed)
        } else {
            Err(MediaError::invalid_argument(format!(
                "invalid PNG prediction method '{value}'"
            )))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PngEncodeOptions {
    pub prediction: PngPrediction,
    pub compression_level: u8,
    pub dpi: Option<u32>,
    pub dpm: Option<u32>,
    pub interlaced: bool,
    pub sample_aspect_ratio: (u32, u32),
}

impl Default for PngEncodeOptions {
    fn default() -> Self {
        Self {
            // FFmpeg n9.0.1 pngenc.c defaults `pred` to PNG_FILTER_VALUE_PAETH.
            prediction: PngPrediction::Paeth,
            // zlib's Z_DEFAULT_COMPRESSION is level 6.
            compression_level: 6,
            dpi: None,
            dpm: None,
            interlaced: false,
            // AVCodecContext's unspecified SAR is conventionally 0/1.
            sample_aspect_ratio: (0, 1),
        }
    }
}

impl PngEncodeOptions {
    pub fn validate(self) -> Result<Self> {
        if self.compression_level > 9 {
            return Err(MediaError::invalid_argument(
                "PNG compression_level must be in 0..=9",
            ));
        }
        if self.dpi.is_some() && self.dpm.is_some() {
            return Err(MediaError::invalid_argument(
                "only one of PNG dpi or dpm may be set",
            ));
        }
        if self.dpi.is_some_and(|value| value > 0x1_0000)
            || self.dpm.is_some_and(|value| value > 0x1_0000)
        {
            return Err(MediaError::invalid_argument(
                "PNG dpi/dpm must be in 0..=65536",
            ));
        }
        Ok(self)
    }

    pub fn dots_per_meter(self) -> Result<Option<u32>> {
        self.validate()?;
        if let Some(dpm) = self.dpm {
            return Ok(Some(dpm));
        }
        if let Some(dpi) = self.dpi {
            return dpi
                .checked_mul(10_000)
                .map(|value| Some(value / 254))
                .ok_or_else(|| MediaError::overflow("PNG dpi to dpm conversion overflow"));
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_ffmpeg_9_0_1_png_encoder() {
        let options = PngEncodeOptions::default();
        assert_eq!(options.prediction, PngPrediction::Paeth);
        assert_eq!(options.compression_level, 6);
        assert!(!options.interlaced);
        assert_eq!(options.sample_aspect_ratio, (0, 1));
    }

    #[test]
    fn dpi_conversion_matches_ffmpeg_integer_formula() {
        let options = PngEncodeOptions {
            dpi: Some(300),
            ..PngEncodeOptions::default()
        };
        assert_eq!(options.dots_per_meter().unwrap(), Some(11_811));
    }

    #[test]
    fn conflicting_density_and_out_of_range_values_are_rejected() {
        assert!(
            PngEncodeOptions {
                dpi: Some(72),
                dpm: Some(2_835),
                ..PngEncodeOptions::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            PngEncodeOptions {
                compression_level: 10,
                ..PngEncodeOptions::default()
            }
            .validate()
            .is_err()
        );
    }
}
