use rm_core::{MediaError, Result};

const WINDOW: usize = 32_768;
const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 258;
const HASH_SIZE: usize = 1 << 16;

const LENGTH_BASE: [usize; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DISTANCE_BASE: [usize; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DISTANCE_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// Encodes one raw RFC1951 final block using fixed Huffman codes and greedy LZ77 matches.
///
/// The matcher intentionally uses one recent candidate per 3-byte hash. It is deterministic,
/// bounded-memory, and substantially smaller than stored blocks for repetitive media data while
/// remaining simple enough to serve as the reference compressor before more advanced parsing.
///
/// # Errors
///
/// Returns an error on internal integer overflow or if a match cannot be represented by RFC1951.
pub fn compress_fixed(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut writer = BitWriter::new();
    writer.write_bits_lsb(1, 1); // BFINAL
    writer.write_bits_lsb(1, 2); // BTYPE=01 fixed Huffman

    let mut last = vec![usize::MAX; HASH_SIZE];
    let mut position = 0_usize;
    while position < bytes.len() {
        let matched = if position + MIN_MATCH <= bytes.len() {
            find_match(bytes, position, &last)
        } else {
            None
        };

        if let Some((distance, length)) = matched {
            write_length_distance(&mut writer, length, distance)?;
            for consumed in 0..length {
                insert_position(bytes, position + consumed, &mut last);
            }
            position = position
                .checked_add(length)
                .ok_or_else(|| MediaError::overflow("DEFLATE input position overflow"))?;
        } else {
            write_fixed_symbol(&mut writer, u16::from(bytes[position]));
            insert_position(bytes, position, &mut last);
            position += 1;
        }
    }

    write_fixed_symbol(&mut writer, 256);
    Ok(writer.finish())
}

fn find_match(bytes: &[u8], position: usize, last: &[usize]) -> Option<(usize, usize)> {
    let candidate = last[hash3(bytes, position)];
    if candidate == usize::MAX || candidate >= position {
        return None;
    }
    let distance = position - candidate;
    if distance > WINDOW {
        return None;
    }

    let max = MAX_MATCH.min(bytes.len() - position);
    let mut length = 0_usize;
    while length < max && bytes[candidate + length] == bytes[position + length] {
        length += 1;
    }
    (length >= MIN_MATCH).then_some((distance, length))
}

fn insert_position(bytes: &[u8], position: usize, last: &mut [usize]) {
    if position + MIN_MATCH <= bytes.len() {
        last[hash3(bytes, position)] = position;
    }
}

fn hash3(bytes: &[u8], position: usize) -> usize {
    let value = (u32::from(bytes[position]) << 16)
        ^ (u32::from(bytes[position + 1]) << 8)
        ^ u32::from(bytes[position + 2]);
    usize::try_from(value.wrapping_mul(0x1E35_A7BD) >> 16).expect("16-bit hash fits usize")
}

fn write_length_distance(writer: &mut BitWriter, length: usize, distance: usize) -> Result<()> {
    let length_index = range_index(length, &LENGTH_BASE, &LENGTH_EXTRA)
        .ok_or_else(|| MediaError::invalid_argument("DEFLATE match length is out of range"))?;
    let length_symbol = u16::try_from(257 + length_index)
        .map_err(|_| MediaError::overflow("DEFLATE length symbol overflow"))?;
    write_fixed_symbol(writer, length_symbol);
    let length_extra_bits = LENGTH_EXTRA[length_index];
    if length_extra_bits != 0 {
        let extra = length - LENGTH_BASE[length_index];
        writer.write_bits_lsb(
            u32::try_from(extra).map_err(|_| MediaError::overflow("length extra overflow"))?,
            length_extra_bits,
        );
    }

    let distance_index = range_index(distance, &DISTANCE_BASE, &DISTANCE_EXTRA)
        .ok_or_else(|| MediaError::invalid_argument("DEFLATE match distance is out of range"))?;
    writer.write_huffman(
        u16::try_from(distance_index)
            .map_err(|_| MediaError::overflow("distance symbol overflow"))?,
        5,
    );
    let distance_extra_bits = DISTANCE_EXTRA[distance_index];
    if distance_extra_bits != 0 {
        let extra = distance - DISTANCE_BASE[distance_index];
        writer.write_bits_lsb(
            u32::try_from(extra).map_err(|_| MediaError::overflow("distance extra overflow"))?,
            distance_extra_bits,
        );
    }
    Ok(())
}

fn range_index<const N: usize>(value: usize, base: &[usize; N], extra: &[u8; N]) -> Option<usize> {
    base.iter().enumerate().find_map(|(index, &start)| {
        let count = if extra[index] == 0 {
            1
        } else {
            1_usize << extra[index]
        };
        let end = start.checked_add(count - 1)?;
        (value >= start && value <= end).then_some(index)
    })
}

fn write_fixed_symbol(writer: &mut BitWriter, symbol: u16) {
    match symbol {
        0..=143 => writer.write_huffman(0x30 + symbol, 8),
        144..=255 => writer.write_huffman(0x190 + (symbol - 144), 9),
        256..=279 => writer.write_huffman(symbol - 256, 7),
        280..=287 => writer.write_huffman(0xC0 + (symbol - 280), 8),
        _ => unreachable!("fixed literal/length symbol is outside RFC1951 range"),
    }
}

#[derive(Debug, Default)]
struct BitWriter {
    bytes: Vec<u8>,
    current: u8,
    used: u8,
}

impl BitWriter {
    fn new() -> Self {
        Self::default()
    }

    fn write_bit(&mut self, bit: bool) {
        if bit {
            self.current |= 1 << self.used;
        }
        self.used += 1;
        if self.used == 8 {
            self.bytes.push(self.current);
            self.current = 0;
            self.used = 0;
        }
    }

    fn write_bits_lsb(&mut self, value: u32, count: u8) {
        for bit in 0..count {
            self.write_bit((value >> bit) & 1 != 0);
        }
    }

    fn write_huffman(&mut self, code: u16, length: u8) {
        for bit in (0..length).rev() {
            self.write_bit((code >> bit) & 1 != 0);
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.used != 0 {
            self.bytes.push(self.current);
        }
        self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_literal_only_inputs_round_trip() {
        for input in [b"".as_slice(), b"a", b"abcdefg", &[0, 1, 2, 3, 4, 5, 6, 7]] {
            let compressed = compress_fixed(input).unwrap();
            assert_eq!(crate::deflate::inflate(&compressed, input.len()).unwrap(), input);
        }
    }

    #[test]
    fn overlapping_and_long_matches_round_trip() {
        for input in [
            vec![b'a'; 20_000],
            b"abcabcabcabcabcabcabcabcabcabc".repeat(500),
            (0..100_000_u32).map(|value| (value & 0xff) as u8).collect(),
        ] {
            let compressed = compress_fixed(&input).unwrap();
            assert_eq!(crate::deflate::inflate(&compressed, input.len()).unwrap(), input);
        }
    }

    #[test]
    fn repetitive_payload_is_materially_smaller_than_stored_data() {
        let input = b"rgba-row-rgba-row-rgba-row-".repeat(4_000);
        let compressed = compress_fixed(&input).unwrap();
        assert!(compressed.len() < input.len() / 4);
    }

    #[test]
    fn distance_window_boundary_is_respected() {
        let mut input = (0..32_768_u32).map(|value| (value.wrapping_mul(73) & 0xff) as u8).collect::<Vec<_>>();
        input.extend_from_slice(&input.clone());
        let compressed = compress_fixed(&input).unwrap();
        assert_eq!(crate::deflate::inflate(&compressed, input.len()).unwrap(), input);
    }
}
