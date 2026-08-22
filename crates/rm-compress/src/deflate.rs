use rm_core::{MediaError, Result};

const MAX_CODE_BITS: usize = 15;
const CODE_LENGTH_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];
const LENGTH_BASE: [usize; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99,
    115, 131, 163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DISTANCE_BASE: [usize; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025,
    1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DISTANCE_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12,
    12, 13, 13,
];

/// Inflates a raw RFC 1951 DEFLATE stream with an explicit output bound.
///
/// # Errors
///
/// Returns an error for truncated or malformed streams, invalid Huffman trees,
/// invalid back-references, reserved block/symbol values, integer overflow, or
/// output that would exceed `output_limit`.
pub fn inflate(bytes: &[u8], output_limit: usize) -> Result<Vec<u8>> {
    let mut reader = BitReader::new(bytes);
    let mut output = Vec::new();

    loop {
        let final_block = reader.read_bits(1)? != 0;
        match reader.read_bits(2)? {
            0 => decode_stored_block(&mut reader, &mut output, output_limit)?,
            1 => {
                let (literal, distance) = fixed_tables()?;
                decode_compressed_block(
                    &mut reader,
                    &literal,
                    Some(&distance),
                    &mut output,
                    output_limit,
                )?;
            }
            2 => {
                let (literal, distance) = dynamic_tables(&mut reader)?;
                decode_compressed_block(
                    &mut reader,
                    &literal,
                    distance.as_ref(),
                    &mut output,
                    output_limit,
                )?;
            }
            _ => {
                return Err(MediaError::invalid_data(
                    "DEFLATE block uses reserved BTYPE value",
                ));
            }
        }

        if final_block {
            return Ok(output);
        }
    }
}

fn decode_stored_block(
    reader: &mut BitReader<'_>,
    output: &mut Vec<u8>,
    output_limit: usize,
) -> Result<()> {
    reader.align_to_byte();
    let len = usize::from(reader.read_u16_le_aligned()?);
    let nlen = reader.read_u16_le_aligned()?;
    if u16::try_from(len).expect("stored DEFLATE length originated as u16") ^ nlen != u16::MAX {
        return Err(MediaError::invalid_data(
            "DEFLATE stored block LEN/NLEN check failed",
        ));
    }
    ensure_output_capacity(output.len(), len, output_limit)?;
    output.extend_from_slice(reader.take_aligned(len)?);
    Ok(())
}

fn fixed_tables() -> Result<(Huffman, Huffman)> {
    let mut literal_lengths = vec![0_u8; 288];
    literal_lengths[0..=143].fill(8);
    literal_lengths[144..=255].fill(9);
    literal_lengths[256..=279].fill(7);
    literal_lengths[280..=287].fill(8);
    let distance_lengths = vec![5_u8; 32];
    Ok((
        Huffman::from_lengths(&literal_lengths)?,
        Huffman::from_lengths(&distance_lengths)?,
    ))
}

fn dynamic_tables(reader: &mut BitReader<'_>) -> Result<(Huffman, Option<Huffman>)> {
    let literal_count = usize::try_from(reader.read_bits(5)?)
        .expect("five DEFLATE bits always fit usize")
        + 257;
    let distance_count = usize::try_from(reader.read_bits(5)?)
        .expect("five DEFLATE bits always fit usize")
        + 1;
    let code_length_count = usize::try_from(reader.read_bits(4)?)
        .expect("four DEFLATE bits always fit usize")
        + 4;

    if literal_count > 286 || distance_count > 32 || code_length_count > 19 {
        return Err(MediaError::invalid_data(
            "DEFLATE dynamic table counts exceed RFC 1951 bounds",
        ));
    }

    let mut code_lengths = [0_u8; 19];
    for &symbol in &CODE_LENGTH_ORDER[..code_length_count] {
        code_lengths[symbol] = u8::try_from(reader.read_bits(3)?)
            .expect("three DEFLATE bits always fit u8");
    }
    let code_length_table = Huffman::from_lengths(&code_lengths)?;

    let total = literal_count
        .checked_add(distance_count)
        .ok_or_else(|| MediaError::overflow("DEFLATE dynamic code count overflow"))?;
    let lengths = read_dynamic_lengths(reader, &code_length_table, total)?;
    let literal_lengths = &lengths[..literal_count];
    if literal_lengths.get(256).copied().unwrap_or(0) == 0 {
        return Err(MediaError::invalid_data(
            "DEFLATE literal/length tree has no end-of-block symbol",
        ));
    }
    let literal = Huffman::from_lengths(literal_lengths)?;

    let distance_lengths = &lengths[literal_count..];
    let distance = if distance_lengths.iter().all(|&length| length == 0) {
        None
    } else {
        Some(Huffman::from_lengths(distance_lengths)?)
    };
    Ok((literal, distance))
}

fn read_dynamic_lengths(
    reader: &mut BitReader<'_>,
    table: &Huffman,
    total: usize,
) -> Result<Vec<u8>> {
    let mut lengths = Vec::with_capacity(total);
    while lengths.len() < total {
        let symbol = table.decode(reader)?;
        match symbol {
            0..=15 => lengths.push(u8::try_from(symbol).expect("code length symbol fits u8")),
            16 => {
                let previous = *lengths.last().ok_or_else(|| {
                    MediaError::invalid_data("DEFLATE repeat code 16 has no previous code length")
                })?;
                let repeat = usize::try_from(reader.read_bits(2)?)
                    .expect("two DEFLATE bits always fit usize")
                    + 3;
                extend_repeated(&mut lengths, previous, repeat, total)?;
            }
            17 => {
                let repeat = usize::try_from(reader.read_bits(3)?)
                    .expect("three DEFLATE bits always fit usize")
                    + 3;
                extend_repeated(&mut lengths, 0, repeat, total)?;
            }
            18 => {
                let repeat = usize::try_from(reader.read_bits(7)?)
                    .expect("seven DEFLATE bits always fit usize")
                    + 11;
                extend_repeated(&mut lengths, 0, repeat, total)?;
            }
            _ => {
                return Err(MediaError::invalid_data(
                    "DEFLATE code-length tree decoded an invalid symbol",
                ));
            }
        }
    }
    Ok(lengths)
}

fn extend_repeated(
    lengths: &mut Vec<u8>,
    value: u8,
    repeat: usize,
    total: usize,
) -> Result<()> {
    let new_len = lengths
        .len()
        .checked_add(repeat)
        .ok_or_else(|| MediaError::overflow("DEFLATE code-length repeat overflow"))?;
    if new_len > total {
        return Err(MediaError::invalid_data(
            "DEFLATE code-length repeat exceeds declared table size",
        ));
    }
    lengths.resize(new_len, value);
    Ok(())
}

fn decode_compressed_block(
    reader: &mut BitReader<'_>,
    literal: &Huffman,
    distance: Option<&Huffman>,
    output: &mut Vec<u8>,
    output_limit: usize,
) -> Result<()> {
    loop {
        match literal.decode(reader)? {
            symbol @ 0..=255 => {
                ensure_output_capacity(output.len(), 1, output_limit)?;
                output.push(u8::try_from(symbol).expect("literal symbol fits u8"));
            }
            256 => return Ok(()),
            symbol @ 257..=285 => {
                let length_index = usize::from(symbol - 257);
                let extra_length = reader.read_bits(LENGTH_EXTRA[length_index])?;
                let length = LENGTH_BASE[length_index]
                    .checked_add(
                        usize::try_from(extra_length)
                            .expect("DEFLATE length extra bits fit usize"),
                    )
                    .ok_or_else(|| MediaError::overflow("DEFLATE match length overflow"))?;

                let distance_table = distance.ok_or_else(|| {
                    MediaError::invalid_data(
                        "DEFLATE length symbol encountered without a distance tree",
                    )
                })?;
                let distance_symbol = distance_table.decode(reader)?;
                if distance_symbol > 29 {
                    return Err(MediaError::invalid_data(
                        "DEFLATE distance tree decoded a reserved symbol",
                    ));
                }
                let distance_index = usize::from(distance_symbol);
                let extra_distance = reader.read_bits(DISTANCE_EXTRA[distance_index])?;
                let distance = DISTANCE_BASE[distance_index]
                    .checked_add(
                        usize::try_from(extra_distance)
                            .expect("DEFLATE distance extra bits fit usize"),
                    )
                    .ok_or_else(|| MediaError::overflow("DEFLATE distance overflow"))?;
                copy_match(output, distance, length, output_limit)?;
            }
            _ => {
                return Err(MediaError::invalid_data(
                    "DEFLATE literal/length tree decoded a reserved symbol",
                ));
            }
        }
    }
}

fn copy_match(
    output: &mut Vec<u8>,
    distance: usize,
    length: usize,
    output_limit: usize,
) -> Result<()> {
    if distance == 0 || distance > output.len() {
        return Err(MediaError::invalid_data(
            "DEFLATE match distance exceeds produced output",
        ));
    }
    ensure_output_capacity(output.len(), length, output_limit)?;
    for _ in 0..length {
        let source = output.len() - distance;
        let byte = output[source];
        output.push(byte);
    }
    Ok(())
}

fn ensure_output_capacity(current: usize, additional: usize, limit: usize) -> Result<()> {
    let required = current
        .checked_add(additional)
        .ok_or_else(|| MediaError::overflow("DEFLATE output size overflow"))?;
    if required > limit {
        return Err(MediaError::unsupported(format!(
            "DEFLATE output would exceed configured limit of {limit} bytes"
        )));
    }
    Ok(())
}

#[derive(Debug)]
struct Huffman {
    codes_by_length: Vec<Vec<(u16, u16)>>,
    max_bits: usize,
}

impl Huffman {
    fn from_lengths(lengths: &[u8]) -> Result<Self> {
        let mut counts = [0_u16; MAX_CODE_BITS + 1];
        let mut max_bits = 0_usize;
        for &length in lengths {
            let length = usize::from(length);
            if length > MAX_CODE_BITS {
                return Err(MediaError::invalid_data(
                    "DEFLATE Huffman code length exceeds 15 bits",
                ));
            }
            if length != 0 {
                counts[length] = counts[length]
                    .checked_add(1)
                    .ok_or_else(|| MediaError::overflow("DEFLATE Huffman count overflow"))?;
                max_bits = max_bits.max(length);
            }
        }
        if max_bits == 0 {
            return Err(MediaError::invalid_data("DEFLATE Huffman tree is empty"));
        }

        let mut left = 1_i32;
        for &count in counts.iter().take(MAX_CODE_BITS + 1).skip(1) {
            left = (left << 1) - i32::from(count);
            if left < 0 {
                return Err(MediaError::invalid_data(
                    "DEFLATE Huffman tree is oversubscribed",
                ));
            }
        }

        let mut next_code = [0_u16; MAX_CODE_BITS + 1];
        let mut code = 0_u16;
        for bits in 1..=MAX_CODE_BITS {
            code = code
                .checked_add(counts[bits - 1])
                .ok_or_else(|| MediaError::overflow("DEFLATE canonical code overflow"))?
                << 1;
            next_code[bits] = code;
        }

        let mut codes_by_length = vec![Vec::new(); max_bits + 1];
        for (symbol, &length) in lengths.iter().enumerate() {
            let bits = usize::from(length);
            if bits == 0 {
                continue;
            }
            let symbol = u16::try_from(symbol)
                .map_err(|_| MediaError::overflow("DEFLATE symbol index exceeds u16"))?;
            let code = next_code[bits];
            next_code[bits] = next_code[bits]
                .checked_add(1)
                .ok_or_else(|| MediaError::overflow("DEFLATE canonical code overflow"))?;
            codes_by_length[bits].push((code, symbol));
        }

        Ok(Self {
            codes_by_length,
            max_bits,
        })
    }

    fn decode(&self, reader: &mut BitReader<'_>) -> Result<u16> {
        let mut code = 0_u16;
        for bits in 1..=self.max_bits {
            code = (code << 1)
                | u16::try_from(reader.read_bits(1)?).expect("one DEFLATE bit fits u16");
            if let Some((_, symbol)) = self.codes_by_length[bits]
                .iter()
                .find(|(candidate, _)| *candidate == code)
            {
                return Ok(*symbol);
            }
        }
        Err(MediaError::invalid_data(
            "DEFLATE bitstream does not match its Huffman tree",
        ))
    }
}

#[derive(Debug)]
struct BitReader<'a> {
    bytes: &'a [u8],
    byte_index: usize,
    bit_index: u8,
}

impl<'a> BitReader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            byte_index: 0,
            bit_index: 0,
        }
    }

    fn read_bits(&mut self, count: u8) -> Result<u32> {
        if count > 24 {
            return Err(MediaError::invalid_argument(
                "DEFLATE bit reader supports at most 24 bits per read",
            ));
        }
        let mut value = 0_u32;
        for shift in 0..count {
            let byte = *self.bytes.get(self.byte_index).ok_or_else(|| {
                MediaError::eof("unexpected end of DEFLATE bitstream")
            })?;
            value |= u32::from((byte >> self.bit_index) & 1) << shift;
            self.bit_index += 1;
            if self.bit_index == 8 {
                self.bit_index = 0;
                self.byte_index += 1;
            }
        }
        Ok(value)
    }

    fn align_to_byte(&mut self) {
        if self.bit_index != 0 {
            self.bit_index = 0;
            self.byte_index += 1;
        }
    }

    fn read_u16_le_aligned(&mut self) -> Result<u16> {
        if self.bit_index != 0 {
            return Err(MediaError::invalid_data(
                "unaligned DEFLATE stored-block integer read",
            ));
        }
        let bytes = self.take_aligned(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn take_aligned(&mut self, len: usize) -> Result<&'a [u8]> {
        if self.bit_index != 0 {
            return Err(MediaError::invalid_data(
                "unaligned DEFLATE stored-block byte read",
            ));
        }
        let end = self
            .byte_index
            .checked_add(len)
            .ok_or_else(|| MediaError::overflow("DEFLATE byte range overflow"))?;
        let bytes = self.bytes.get(self.byte_index..end).ok_or_else(|| {
            MediaError::eof("unexpected end of DEFLATE stored block")
        })?;
        self.byte_index = end;
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_block_decodes_exact_payload() {
        let stream = [
            0x01, 0x14, 0x00, 0xEB, 0xFF, 0x73, 0x74, 0x6F, 0x72, 0x65, 0x64, 0x20, 0x62,
            0x6C, 0x6F, 0x63, 0x6B, 0x20, 0x70, 0x61, 0x79, 0x6C, 0x6F, 0x61, 0x64,
        ];
        assert_eq!(inflate(&stream, 1024).unwrap(), b"stored block payload");
    }

    #[test]
    fn fixed_huffman_stream_decodes_matches_and_literals() {
        let stream = [
            0xCB, 0x48, 0xCD, 0xC9, 0xC9, 0x57, 0xC8, 0x40, 0x27, 0x15, 0x01,
        ];
        assert_eq!(
            inflate(&stream, 1024).unwrap(),
            b"hello hello hello hello!"
        );
    }

    #[test]
    fn dynamic_huffman_stream_decodes_long_repetitive_payload() {
        let stream = [
            0xED, 0xC7, 0x31, 0x01, 0x00, 0x20, 0x0C, 0x03, 0x30, 0xAD, 0xA5, 0xC3, 0xBF,
            0x05, 0x26, 0x00, 0x09, 0xC9, 0x97, 0x64, 0x9D, 0xD5, 0x76, 0xE6, 0x46, 0x55,
            0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55,
            0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55,
            0x55, 0xF5, 0xD7, 0x07,
        ];
        let expected = b"aaaaabbbbcccdde".repeat(1_000);
        assert_eq!(inflate(&stream, 20_000).unwrap(), expected);
    }

    #[test]
    fn stored_block_rejects_bad_complement() {
        let stream = [0x01, 0x01, 0x00, 0x00, 0x00, b'x'];
        assert!(inflate(&stream, 32).is_err());
    }

    #[test]
    fn output_limit_stops_decompression_bomb_expansion() {
        let stream = [
            0xCB, 0x48, 0xCD, 0xC9, 0xC9, 0x57, 0xC8, 0x40, 0x27, 0x15, 0x01,
        ];
        assert!(inflate(&stream, 8).is_err());
    }

    #[test]
    fn reserved_block_type_is_rejected() {
        assert!(inflate(&[0x07], 16).is_err());
    }
}
