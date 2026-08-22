use rm_core::video::{PixelFormat, VideoFrame};
use rm_core::{MediaError, Result};

const MAX_PIXELS: u64 = 268_435_456;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PnmKind {
    Pbm,
    Pgm,
    Ppm,
}

impl PnmKind {
    #[must_use]
    pub const fn codec_name(self) -> &'static str {
        match self {
            Self::Pbm => "pbm",
            Self::Pgm => "pgm",
            Self::Ppm => "ppm",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PnmImage {
    pub kind: PnmKind,
    pub frame: VideoFrame,
}

#[must_use]
pub fn probe_pnm(bytes: &[u8]) -> u8 {
    if bytes.len() < 3 || bytes[0] != b'P' || !(b'1'..=b'6').contains(&bytes[1]) {
        return 0;
    }
    if !bytes[2].is_ascii_whitespace() && bytes[2] != b'#' {
        return 0;
    }
    100
}

pub fn decode_pnm(bytes: &[u8]) -> Result<PnmImage> {
    if probe_pnm(bytes) == 0 {
        return Err(MediaError::invalid_data("input is not a supported Netpbm image"));
    }

    let magic = bytes[1];
    let mut tokens = Tokenizer::new(bytes, 2);
    let width = parse_dimension(tokens.next_token()?, "width")?;
    let height = parse_dimension(tokens.next_token()?, "height")?;
    validate_dimensions(width, height)?;

    match magic {
        b'1' => decode_ascii_pbm(&mut tokens, width, height),
        b'2' => {
            let maxval = parse_maxval(tokens.next_token()?)?;
            decode_ascii_samples(&mut tokens, PnmKind::Pgm, width, height, maxval)
        }
        b'3' => {
            let maxval = parse_maxval(tokens.next_token()?)?;
            decode_ascii_samples(&mut tokens, PnmKind::Ppm, width, height, maxval)
        }
        b'4' => decode_binary_pbm(tokens.remaining(), width, height),
        b'5' => {
            let maxval = parse_maxval(tokens.next_token()?)?;
            decode_binary_samples(tokens.remaining(), PnmKind::Pgm, width, height, maxval)
        }
        b'6' => {
            let maxval = parse_maxval(tokens.next_token()?)?;
            decode_binary_samples(tokens.remaining(), PnmKind::Ppm, width, height, maxval)
        }
        _ => Err(MediaError::invalid_data("unsupported Netpbm magic")),
    }
}

pub fn encode_pnm(kind: PnmKind, frame: &VideoFrame) -> Result<Vec<u8>> {
    match kind {
        PnmKind::Pbm => encode_pbm(frame),
        PnmKind::Pgm => encode_pgm(frame),
        PnmKind::Ppm => encode_ppm(frame),
    }
}

fn decode_ascii_pbm(tokens: &mut Tokenizer<'_>, width: u32, height: u32) -> Result<PnmImage> {
    let count = pixel_count_usize(width, height)?;
    let mut pixels = Vec::with_capacity(count);
    for _ in 0..count {
        let token = tokens.next_token()?;
        let value = parse_u32(token, "PBM sample")?;
        pixels.push(match value {
            0 => 255,
            1 => 0,
            _ => return Err(MediaError::invalid_data("PBM samples must be 0 or 1")),
        });
    }
    tokens.ensure_ascii_exhausted()?;
    Ok(PnmImage {
        kind: PnmKind::Pbm,
        frame: VideoFrame::from_vec(width, height, PixelFormat::Gray8, pixels)?,
    })
}

fn decode_ascii_samples(
    tokens: &mut Tokenizer<'_>,
    kind: PnmKind,
    width: u32,
    height: u32,
    maxval: u16,
) -> Result<PnmImage> {
    let channels = match kind {
        PnmKind::Pgm => 1,
        PnmKind::Ppm => 3,
        PnmKind::Pbm => unreachable!(),
    };
    let samples = pixel_count_usize(width, height)?
        .checked_mul(channels)
        .ok_or_else(|| MediaError::overflow("Netpbm sample count overflow"))?;
    let mut pixels = Vec::with_capacity(samples);
    for _ in 0..samples {
        let value = parse_u32(tokens.next_token()?, "Netpbm sample")?;
        if value > u32::from(maxval) {
            return Err(MediaError::invalid_data("Netpbm sample exceeds maxval"));
        }
        pixels.push(scale_to_u8(value, maxval));
    }
    tokens.ensure_ascii_exhausted()?;
    let format = if kind == PnmKind::Pgm {
        PixelFormat::Gray8
    } else {
        PixelFormat::Rgb24
    };
    Ok(PnmImage {
        kind,
        frame: VideoFrame::from_vec(width, height, format, pixels)?,
    })
}

fn decode_binary_pbm(data: &[u8], width: u32, height: u32) -> Result<PnmImage> {
    let width_usize = usize::try_from(width)
        .map_err(|_| MediaError::overflow("PBM width exceeds usize"))?;
    let height_usize = usize::try_from(height)
        .map_err(|_| MediaError::overflow("PBM height exceeds usize"))?;
    let row_bytes = width_usize
        .checked_add(7)
        .ok_or_else(|| MediaError::overflow("PBM row size overflow"))?
        / 8;
    let expected = row_bytes
        .checked_mul(height_usize)
        .ok_or_else(|| MediaError::overflow("PBM raster size overflow"))?;
    if data.len() != expected {
        return Err(MediaError::invalid_data(format!(
            "PBM raster has {} bytes but {expected} are required",
            data.len()
        )));
    }

    let mut pixels = Vec::with_capacity(pixel_count_usize(width, height)?);
    for row in data.chunks_exact(row_bytes) {
        for x in 0..width_usize {
            let byte = row[x / 8];
            let bit = (byte >> (7 - (x % 8))) & 1;
            pixels.push(if bit == 1 { 0 } else { 255 });
        }
    }

    Ok(PnmImage {
        kind: PnmKind::Pbm,
        frame: VideoFrame::from_vec(width, height, PixelFormat::Gray8, pixels)?,
    })
}

fn decode_binary_samples(
    data: &[u8],
    kind: PnmKind,
    width: u32,
    height: u32,
    maxval: u16,
) -> Result<PnmImage> {
    let channels = if kind == PnmKind::Pgm { 1_usize } else { 3 };
    let samples = pixel_count_usize(width, height)?
        .checked_mul(channels)
        .ok_or_else(|| MediaError::overflow("Netpbm sample count overflow"))?;
    let bytes_per_sample = if maxval < 256 { 1_usize } else { 2 };
    let expected = samples
        .checked_mul(bytes_per_sample)
        .ok_or_else(|| MediaError::overflow("Netpbm raster size overflow"))?;
    if data.len() != expected {
        return Err(MediaError::invalid_data(format!(
            "Netpbm raster has {} bytes but {expected} are required",
            data.len()
        )));
    }

    let mut pixels = Vec::with_capacity(samples);
    if bytes_per_sample == 1 {
        for &value in data {
            let value = u32::from(value);
            if value > u32::from(maxval) {
                return Err(MediaError::invalid_data("Netpbm sample exceeds maxval"));
            }
            pixels.push(scale_to_u8(value, maxval));
        }
    } else {
        for sample in data.chunks_exact(2) {
            let value = u32::from(u16::from_be_bytes([sample[0], sample[1]]));
            if value > u32::from(maxval) {
                return Err(MediaError::invalid_data("Netpbm sample exceeds maxval"));
            }
            pixels.push(scale_to_u8(value, maxval));
        }
    }

    let format = if kind == PnmKind::Pgm {
        PixelFormat::Gray8
    } else {
        PixelFormat::Rgb24
    };
    Ok(PnmImage {
        kind,
        frame: VideoFrame::from_vec(width, height, format, pixels)?,
    })
}

fn encode_pbm(frame: &VideoFrame) -> Result<Vec<u8>> {
    require_format(frame, PixelFormat::Gray8, "PBM")?;
    let width = usize::try_from(frame.width)
        .map_err(|_| MediaError::overflow("PBM width exceeds usize"))?;
    let row_bytes = width
        .checked_add(7)
        .ok_or_else(|| MediaError::overflow("PBM row size overflow"))?
        / 8;
    let raster_size = row_bytes
        .checked_mul(usize::try_from(frame.height).map_err(|_| {
            MediaError::overflow("PBM height exceeds usize")
        })?)
        .ok_or_else(|| MediaError::overflow("PBM raster size overflow"))?;
    let mut output = Vec::with_capacity(32 + raster_size);
    output.extend_from_slice(format!("P4\n{} {}\n", frame.width, frame.height).as_bytes());

    for y in 0..frame.height {
        let row = frame.row(y)?;
        for group in 0..row_bytes {
            let mut byte = 0_u8;
            for bit in 0..8 {
                let x = group * 8 + bit;
                if x < width && row[x] < 128 {
                    byte |= 1 << (7 - bit);
                }
            }
            output.push(byte);
        }
    }
    Ok(output)
}

fn encode_pgm(frame: &VideoFrame) -> Result<Vec<u8>> {
    require_format(frame, PixelFormat::Gray8, "PGM")?;
    let mut output = Vec::with_capacity(frame.data.len().saturating_add(32));
    output.extend_from_slice(format!("P5\n{} {}\n255\n", frame.width, frame.height).as_bytes());
    output.extend_from_slice(frame.data.as_slice());
    Ok(output)
}

fn encode_ppm(frame: &VideoFrame) -> Result<Vec<u8>> {
    require_format(frame, PixelFormat::Rgb24, "PPM")?;
    let mut output = Vec::with_capacity(frame.data.len().saturating_add(32));
    output.extend_from_slice(format!("P6\n{} {}\n255\n", frame.width, frame.height).as_bytes());
    output.extend_from_slice(frame.data.as_slice());
    Ok(output)
}

fn require_format(frame: &VideoFrame, expected: PixelFormat, name: &str) -> Result<()> {
    if frame.format != expected {
        return Err(MediaError::invalid_argument(format!(
            "{name} encoder requires {} input, got {}",
            expected.name(),
            frame.format.name()
        )));
    }
    Ok(())
}

fn parse_dimension(token: &[u8], name: &str) -> Result<u32> {
    let value = parse_u32(token, name)?;
    if value == 0 {
        return Err(MediaError::invalid_data(format!(
            "Netpbm {name} must be non-zero"
        )));
    }
    Ok(value)
}

fn parse_maxval(token: &[u8]) -> Result<u16> {
    let value = parse_u32(token, "maxval")?;
    if value == 0 || value > 65_535 {
        return Err(MediaError::invalid_data(
            "Netpbm maxval must be in the range 1..=65535",
        ));
    }
    u16::try_from(value).map_err(|_| MediaError::overflow("Netpbm maxval exceeds u16"))
}

fn parse_u32(token: &[u8], name: &str) -> Result<u32> {
    if token.is_empty() || !token.iter().all(u8::is_ascii_digit) {
        return Err(MediaError::invalid_data(format!(
            "invalid Netpbm {name} token"
        )));
    }
    let mut value = 0_u32;
    for &digit in token {
        value = value
            .checked_mul(10)
            .and_then(|current| current.checked_add(u32::from(digit - b'0')))
            .ok_or_else(|| MediaError::overflow(format!("Netpbm {name} overflow")))?;
    }
    Ok(value)
}

fn validate_dimensions(width: u32, height: u32) -> Result<()> {
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or_else(|| MediaError::overflow("Netpbm pixel count overflow"))?;
    if pixels > MAX_PIXELS {
        return Err(MediaError::unsupported(format!(
            "Netpbm image has {pixels} pixels; limit is {MAX_PIXELS}"
        )));
    }
    Ok(())
}

fn pixel_count_usize(width: u32, height: u32) -> Result<usize> {
    usize::try_from(u64::from(width) * u64::from(height))
        .map_err(|_| MediaError::overflow("Netpbm pixel count exceeds usize"))
}

fn scale_to_u8(value: u32, maxval: u16) -> u8 {
    let maxval = u32::from(maxval);
    let scaled = (value * 255 + maxval / 2) / maxval;
    u8::try_from(scaled).expect("scaled Netpbm sample is always in u8 range")
}

struct Tokenizer<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Tokenizer<'a> {
    const fn new(bytes: &'a [u8], position: usize) -> Self {
        Self { bytes, position }
    }

    fn next_token(&mut self) -> Result<&'a [u8]> {
        self.skip_prefix();
        if self.position >= self.bytes.len() {
            return Err(MediaError::eof("unexpected end of Netpbm header"));
        }
        let start = self.position;
        while self.position < self.bytes.len() {
            let byte = self.bytes[self.position];
            if byte.is_ascii_whitespace() || byte == b'#' {
                break;
            }
            self.position += 1;
        }
        if start == self.position {
            return Err(MediaError::invalid_data("empty Netpbm token"));
        }
        let token = &self.bytes[start..self.position];
        if self.position < self.bytes.len() {
            if self.bytes[self.position] == b'#' {
                self.skip_comment();
            } else {
                let delimiter = self.bytes[self.position];
                self.position += 1;
                if delimiter == b'\r' && self.bytes.get(self.position) == Some(&b'\n') {
                    self.position += 1;
                }
            }
        }
        Ok(token)
    }

    fn remaining(&self) -> &'a [u8] {
        &self.bytes[self.position..]
    }

    fn ensure_ascii_exhausted(&mut self) -> Result<()> {
        self.skip_prefix();
        if self.position != self.bytes.len() {
            return Err(MediaError::invalid_data(
                "extra tokens found after Netpbm raster",
            ));
        }
        Ok(())
    }

    fn skip_prefix(&mut self) {
        loop {
            while self.position < self.bytes.len()
                && self.bytes[self.position].is_ascii_whitespace()
            {
                self.position += 1;
            }
            if self.position < self.bytes.len() && self.bytes[self.position] == b'#' {
                self.skip_comment();
                continue;
            }
            break;
        }
    }

    fn skip_comment(&mut self) {
        while self.position < self.bytes.len() && self.bytes[self.position] != b'\n' {
            self.position += 1;
        }
        if self.position < self.bytes.len() {
            self.position += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_ppm_comments_and_scaling_decode() {
        let image = decode_pnm(b"P3\n# comment\n2 1\n15\n15 0 0  0 15 7\n").unwrap();
        assert_eq!(image.kind, PnmKind::Ppm);
        assert_eq!(image.frame.format, PixelFormat::Rgb24);
        assert_eq!(image.frame.data.as_slice(), &[255, 0, 0, 0, 255, 119]);
    }

    #[test]
    fn binary_sixteen_bit_pgm_scales_big_endian_samples() {
        let mut bytes = b"P5\n2 1\n65535\n".to_vec();
        bytes.extend_from_slice(&[0x00, 0x00, 0xFF, 0xFF]);
        let image = decode_pnm(&bytes).unwrap();
        assert_eq!(image.frame.data.as_slice(), &[0, 255]);
    }

    #[test]
    fn pbm_binary_unpack_and_reencode_round_trip() {
        let image = decode_pnm(b"P4\n9 1\n\xAA\x80").unwrap();
        assert_eq!(
            image.frame.data.as_slice(),
            &[0, 255, 0, 255, 0, 255, 0, 255, 0]
        );
        assert_eq!(encode_pnm(PnmKind::Pbm, &image.frame).unwrap(), b"P4\n9 1\n\xAA\x80");
    }

    #[test]
    fn ppm_binary_round_trip_is_bit_exact_at_maxval_255() {
        let source = b"P6\n2 1\n255\n\x00\x7F\xFF\xFF\x00\x20";
        let image = decode_pnm(source).unwrap();
        assert_eq!(encode_pnm(PnmKind::Ppm, &image.frame).unwrap(), source);
    }

    #[test]
    fn malformed_dimensions_samples_and_rasters_are_rejected() {
        assert!(decode_pnm(b"P6\n0 1\n255\n").is_err());
        assert!(decode_pnm(b"P2\n1 1\n255\n256\n").is_err());
        assert!(decode_pnm(b"P6\n1 1\n255\n\x00\x00").is_err());
        assert!(decode_pnm(b"P1\n1 1\n2\n").is_err());
    }
}
