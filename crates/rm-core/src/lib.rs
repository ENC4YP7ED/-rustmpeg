#![forbid(unsafe_code)]

use std::fmt;
use std::sync::Arc;

pub const RUSTMPEG_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const FFMPEG_COMPAT_VERSION: &str = "9.0.1";

pub type Result<T> = std::result::Result<T, MediaError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    EndOfFile,
    InvalidArgument,
    InvalidData,
    Overflow,
    Unsupported,
    Io,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaError {
    kind: ErrorKind,
    message: String,
}

impl MediaError {
    #[must_use]
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        self.kind
    }

    #[must_use]
    pub fn eof(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::EndOfFile, message)
    }

    #[must_use]
    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::InvalidArgument, message)
    }

    #[must_use]
    pub fn invalid_data(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::InvalidData, message)
    }

    #[must_use]
    pub fn overflow(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Overflow, message)
    }

    #[must_use]
    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Unsupported, message)
    }
}

impl fmt::Display for MediaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for MediaError {}

impl From<std::io::Error> for MediaError {
    fn from(value: std::io::Error) -> Self {
        Self::new(ErrorKind::Io, value.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rational {
    num: i64,
    den: i64,
}

impl Rational {
    pub const ZERO: Self = Self { num: 0, den: 1 };
    pub const ONE: Self = Self { num: 1, den: 1 };
    pub const MILLISECOND: Self = Self { num: 1, den: 1_000 };
    pub const MICROSECOND: Self = Self {
        num: 1,
        den: 1_000_000,
    };

    pub fn new(num: i64, den: i64) -> Result<Self> {
        if den == 0 {
            return Err(MediaError::invalid_argument(
                "rational denominator must not be zero",
            ));
        }

        let mut num = i128::from(num);
        let mut den = i128::from(den);
        if den < 0 {
            num = -num;
            den = -den;
        }

        let gcd = gcd_u128(abs_i128(num), den as u128) as i128;
        num /= gcd;
        den /= gcd;

        let num = i64::try_from(num)
            .map_err(|_| MediaError::overflow("normalized rational numerator exceeds i64"))?;
        let den = i64::try_from(den)
            .map_err(|_| MediaError::overflow("normalized rational denominator exceeds i64"))?;

        Ok(Self { num, den })
    }

    #[must_use]
    pub const fn numerator(self) -> i64 {
        self.num
    }

    #[must_use]
    pub const fn denominator(self) -> i64 {
        self.den
    }

    #[must_use]
    pub fn as_f64(self) -> f64 {
        self.num as f64 / self.den as f64
    }

    pub fn reciprocal(self) -> Result<Self> {
        Self::new(self.den, self.num)
    }
}

fn abs_i128(value: i128) -> u128 {
    if value < 0 {
        (-value) as u128
    } else {
        value as u128
    }
}

fn gcd_u128(mut lhs: u128, mut rhs: u128) -> u128 {
    while rhs != 0 {
        let remainder = lhs % rhs;
        lhs = rhs;
        rhs = remainder;
    }
    lhs.max(1)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rounding {
    TowardZero,
    Down,
    Up,
    Nearest,
}

pub fn rescale(value: i64, source: Rational, destination: Rational, rounding: Rounding) -> Result<i64> {
    if destination.num == 0 {
        return Err(MediaError::invalid_argument(
            "destination time base numerator must not be zero",
        ));
    }

    let numerator = i128::from(value)
        .checked_mul(i128::from(source.num))
        .and_then(|v| v.checked_mul(i128::from(destination.den)))
        .ok_or_else(|| MediaError::overflow("timestamp rescale numerator overflow"))?;
    let denominator = i128::from(source.den)
        .checked_mul(i128::from(destination.num))
        .ok_or_else(|| MediaError::overflow("timestamp rescale denominator overflow"))?;

    if denominator == 0 {
        return Err(MediaError::invalid_argument("invalid zero rescale denominator"));
    }

    let quotient = div_round(numerator, denominator, rounding);
    i64::try_from(quotient).map_err(|_| MediaError::overflow("rescaled timestamp exceeds i64"))
}

fn div_round(numerator: i128, denominator: i128, rounding: Rounding) -> i128 {
    let quotient = numerator / denominator;
    let remainder = numerator % denominator;
    if remainder == 0 {
        return quotient;
    }

    let same_sign = (numerator < 0) == (denominator < 0);
    match rounding {
        Rounding::TowardZero => quotient,
        Rounding::Down => {
            if same_sign {
                quotient
            } else {
                quotient - 1
            }
        }
        Rounding::Up => {
            if same_sign {
                quotient + 1
            } else {
                quotient
            }
        }
        Rounding::Nearest => {
            let twice_remainder = abs_i128(remainder).saturating_mul(2);
            if twice_remainder >= abs_i128(denominator) {
                if same_sign {
                    quotient + 1
                } else {
                    quotient - 1
                }
            } else {
                quotient
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timestamp {
    pub value: i64,
    pub time_base: Rational,
}

impl Timestamp {
    pub fn rescale(self, time_base: Rational, rounding: Rounding) -> Result<Self> {
        Ok(Self {
            value: rescale(self.value, self.time_base, time_base, rounding)?,
            time_base,
        })
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Buffer {
    storage: Arc<[u8]>,
    offset: usize,
    len: usize,
}

impl Buffer {
    #[must_use]
    pub fn from_vec(bytes: Vec<u8>) -> Self {
        let len = bytes.len();
        Self {
            storage: Arc::from(bytes),
            offset: 0,
            len,
        }
    }

    #[must_use]
    pub fn copy_from_slice(bytes: &[u8]) -> Self {
        Self::from_vec(bytes.to_vec())
    }

    pub fn slice(&self, offset: usize, len: usize) -> Result<Self> {
        let end = offset
            .checked_add(len)
            .ok_or_else(|| MediaError::overflow("buffer slice overflow"))?;
        if end > self.len {
            return Err(MediaError::invalid_argument("buffer slice is out of bounds"));
        }

        Ok(Self {
            storage: Arc::clone(&self.storage),
            offset: self.offset + offset,
            len,
        })
    }

    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.storage[self.offset..self.offset + self.len]
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl fmt::Debug for Buffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Buffer")
            .field("len", &self.len)
            .field("shared", &(Arc::strong_count(&self.storage) > 1))
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PacketFlags(u32);

impl PacketFlags {
    pub const KEY: u32 = 1 << 0;
    pub const CORRUPT: u32 = 1 << 1;
    pub const DISCARD: u32 = 1 << 2;

    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }

    #[must_use]
    pub const fn contains(self, flag: u32) -> bool {
        self.0 & flag != 0
    }

    pub fn set(&mut self, flag: u32, enabled: bool) {
        if enabled {
            self.0 |= flag;
        } else {
            self.0 &= !flag;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    pub data: Buffer,
    pub stream_index: usize,
    pub pts: Option<i64>,
    pub dts: Option<i64>,
    pub duration: i64,
    pub time_base: Rational,
    pub flags: PacketFlags,
}

impl Packet {
    #[must_use]
    pub fn new(data: Buffer, stream_index: usize, time_base: Rational) -> Self {
        Self {
            data,
            stream_index,
            pts: None,
            dts: None,
            duration: 0,
            time_base,
            flags: PacketFlags::default(),
        }
    }
}

#[must_use]
pub fn build_banner(program: &str) -> String {
    format!(
        "{program} version rustmpeg-{RUSTMPEG_VERSION} (FFmpeg {FFMPEG_COMPAT_VERSION} compatibility target)"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rational_normalizes_sign_and_gcd() {
        assert_eq!(Rational::new(30, -60).unwrap(), Rational::new(-1, 2).unwrap());
    }

    #[test]
    fn rescale_rounds_negative_values_correctly() {
        let source = Rational::new(1, 3).unwrap();
        let destination = Rational::ONE;
        assert_eq!(rescale(-2, source, destination, Rounding::Down).unwrap(), -1);
        assert_eq!(rescale(-2, source, destination, Rounding::Up).unwrap(), 0);
        assert_eq!(rescale(-2, source, destination, Rounding::Nearest).unwrap(), -1);
    }

    #[test]
    fn buffer_slicing_shares_storage() {
        let buffer = Buffer::from_vec(vec![1, 2, 3, 4]);
        let slice = buffer.slice(1, 2).unwrap();
        assert_eq!(slice.as_slice(), &[2, 3]);
        assert_eq!(buffer.as_slice(), &[1, 2, 3, 4]);
    }
}
