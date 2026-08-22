#![forbid(unsafe_code)]

use rm_core::{MediaError, Result};

#[derive(Debug, Clone)]
pub struct ByteReader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> ByteReader<'a> {
    #[must_use]
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    #[must_use]
    pub const fn position(&self) -> usize {
        self.position
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.bytes.len()
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    #[must_use]
    pub fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.position)
    }

    pub fn seek(&mut self, position: usize) -> Result<()> {
        if position > self.bytes.len() {
            return Err(MediaError::invalid_argument("byte reader seek is out of bounds"));
        }
        self.position = position;
        Ok(())
    }

    pub fn skip(&mut self, len: usize) -> Result<()> {
        let position = self
            .position
            .checked_add(len)
            .ok_or_else(|| MediaError::overflow("byte reader position overflow"))?;
        self.seek(position)
    }

    pub fn take(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self
            .position
            .checked_add(len)
            .ok_or_else(|| MediaError::overflow("byte reader position overflow"))?;
        let bytes = self
            .bytes
            .get(self.position..end)
            .ok_or_else(|| MediaError::eof("unexpected end of input"))?;
        self.position = end;
        Ok(bytes)
    }

    pub fn peek(&self, len: usize) -> Result<&'a [u8]> {
        let end = self
            .position
            .checked_add(len)
            .ok_or_else(|| MediaError::overflow("byte reader position overflow"))?;
        self.bytes
            .get(self.position..end)
            .ok_or_else(|| MediaError::eof("unexpected end of input"))
    }

    pub fn read_array<const N: usize>(&mut self) -> Result<[u8; N]> {
        let bytes = self.take(N)?;
        let mut value = [0_u8; N];
        value.copy_from_slice(bytes);
        Ok(value)
    }

    pub fn read_u8(&mut self) -> Result<u8> {
        Ok(self.read_array::<1>()?[0])
    }

    pub fn read_u16_le(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.read_array()?))
    }

    pub fn read_u16_be(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(self.read_array()?))
    }

    pub fn read_i16_le(&mut self) -> Result<i16> {
        Ok(i16::from_le_bytes(self.read_array()?))
    }

    pub fn read_u24_le(&mut self) -> Result<u32> {
        let bytes = self.read_array::<3>()?;
        Ok(u32::from(bytes[0]) | (u32::from(bytes[1]) << 8) | (u32::from(bytes[2]) << 16))
    }

    pub fn read_u24_be(&mut self) -> Result<u32> {
        let bytes = self.read_array::<3>()?;
        Ok((u32::from(bytes[0]) << 16) | (u32::from(bytes[1]) << 8) | u32::from(bytes[2]))
    }

    pub fn read_u32_le(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.read_array()?))
    }

    pub fn read_u32_be(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(self.read_array()?))
    }

    pub fn read_i32_le(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.read_array()?))
    }

    pub fn read_u64_le(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.read_array()?))
    }

    pub fn read_u64_be(&mut self) -> Result<u64> {
        Ok(u64::from_be_bytes(self.read_array()?))
    }
}

#[derive(Debug, Default, Clone)]
pub struct ByteWriter {
    bytes: Vec<u8>,
}

impl ByteWriter {
    #[must_use]
    pub const fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(capacity),
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub fn write(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }

    pub fn write_u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    pub fn write_u16_le(&mut self, value: u16) {
        self.write(&value.to_le_bytes());
    }

    pub fn write_u16_be(&mut self, value: u16) {
        self.write(&value.to_be_bytes());
    }

    pub fn write_u32_le(&mut self, value: u32) {
        self.write(&value.to_le_bytes());
    }

    pub fn write_u32_be(&mut self, value: u32) {
        self.write(&value.to_be_bytes());
    }

    pub fn write_u64_le(&mut self, value: u64) {
        self.write(&value.to_le_bytes());
    }

    pub fn patch_u32_le(&mut self, position: usize, value: u32) -> Result<()> {
        let end = position
            .checked_add(4)
            .ok_or_else(|| MediaError::overflow("byte writer patch position overflow"))?;
        let target = self
            .bytes
            .get_mut(position..end)
            .ok_or_else(|| MediaError::invalid_argument("byte writer patch is out of bounds"))?;
        target.copy_from_slice(&value.to_le_bytes());
        Ok(())
    }

    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.bytes
    }

    #[must_use]
    pub fn into_inner(self) -> Vec<u8> {
        self.bytes
    }
}

#[derive(Debug, Clone)]
pub struct BitReader<'a> {
    bytes: &'a [u8],
    bit_position: usize,
}

impl<'a> BitReader<'a> {
    #[must_use]
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            bit_position: 0,
        }
    }

    #[must_use]
    pub const fn bit_position(&self) -> usize {
        self.bit_position
    }

    #[must_use]
    pub fn bits_remaining(&self) -> usize {
        self.bytes
            .len()
            .saturating_mul(8)
            .saturating_sub(self.bit_position)
    }

    pub fn read_bit(&mut self) -> Result<bool> {
        if self.bit_position >= self.bytes.len().saturating_mul(8) {
            return Err(MediaError::eof("unexpected end of bitstream"));
        }

        let byte = self.bytes[self.bit_position / 8];
        let shift = 7 - (self.bit_position % 8);
        self.bit_position += 1;
        Ok(((byte >> shift) & 1) != 0)
    }

    pub fn read_bits(&mut self, count: u8) -> Result<u64> {
        if count > 64 {
            return Err(MediaError::invalid_argument("cannot read more than 64 bits"));
        }
        if usize::from(count) > self.bits_remaining() {
            return Err(MediaError::eof("unexpected end of bitstream"));
        }

        let mut value = 0_u64;
        for _ in 0..count {
            value = (value << 1) | u64::from(self.read_bit()?);
        }
        Ok(value)
    }

    pub fn skip_bits(&mut self, count: usize) -> Result<()> {
        if count > self.bits_remaining() {
            return Err(MediaError::eof("unexpected end of bitstream"));
        }
        self.bit_position += count;
        Ok(())
    }

    pub fn align_to_byte(&mut self) -> Result<()> {
        let remainder = self.bit_position % 8;
        if remainder != 0 {
            self.skip_bits(8 - remainder)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_reader_tracks_position_and_endianness() {
        let mut reader = ByteReader::new(&[0x34, 0x12, 0xAA, 0xBB]);
        assert_eq!(reader.read_u16_le().unwrap(), 0x1234);
        assert_eq!(reader.read_u16_be().unwrap(), 0xAABB);
        assert_eq!(reader.remaining(), 0);
    }

    #[test]
    fn bit_reader_is_msb_first() {
        let mut reader = BitReader::new(&[0b1011_0010]);
        assert_eq!(reader.read_bits(3).unwrap(), 0b101);
        assert_eq!(reader.read_bits(5).unwrap(), 0b1_0010);
    }

    #[test]
    fn writer_can_patch_container_lengths() {
        let mut writer = ByteWriter::new();
        writer.write(b"RIFF");
        writer.write_u32_le(0);
        writer.patch_u32_le(4, 36).unwrap();
        assert_eq!(&writer.as_slice()[4..8], &36_u32.to_le_bytes());
    }
}
