use rm_core::{MediaError, Result};

const WINDOW: usize = 32_768;
const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 258;
const HASH_SIZE: usize = 1 << 16;
const NONE: usize = usize::MAX;

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

/// Encodes one final fixed-Huffman RFC1951 block with effort-scaled hash-chain search.
/// `level` is 1..=9; larger values examine more candidate matches.
///
/// # Errors
/// Returns an error when `level` is outside 1..=9 or on integer overflow.
pub fn compress_fixed_level(bytes: &[u8], level: u8) -> Result<Vec<u8>> {
    if !(1..=9).contains(&level) {
        return Err(MediaError::invalid_argument(
            "DEFLATE level must be in 1..=9",
        ));
    }
    let search_limit = match level {
        1 => 1,
        2 => 2,
        3 => 4,
        4 => 8,
        5 => 16,
        6 => 32,
        7 => 64,
        8 => 128,
        9 => 256,
        _ => unreachable!(),
    };

    let mut writer = BitWriter::default();
    writer.write_bits_lsb(1, 1);
    writer.write_bits_lsb(1, 2); // fixed Huffman

    let mut heads = vec![NONE; HASH_SIZE];
    let mut previous = vec![NONE; bytes.len()];
    let mut position = 0_usize;
    while position < bytes.len() {
        let best = if position + MIN_MATCH <= bytes.len() {
            find_best(bytes, position, &heads, &previous, search_limit)
        } else {
            None
        };
        if let Some((distance, length)) = best {
            write_match(&mut writer, length, distance)?;
            for consumed in 0..length {
                insert(bytes, position + consumed, &mut heads, &mut previous);
            }
            position = position
                .checked_add(length)
                .ok_or_else(|| MediaError::overflow("DEFLATE position overflow"))?;
        } else {
            write_fixed_symbol(&mut writer, u16::from(bytes[position]));
            insert(bytes, position, &mut heads, &mut previous);
            position += 1;
        }
    }
    write_fixed_symbol(&mut writer, 256);
    Ok(writer.finish())
}

fn find_best(
    bytes: &[u8],
    position: usize,
    heads: &[usize],
    previous: &[usize],
    search_limit: usize,
) -> Option<(usize, usize)> {
    let mut candidate = heads[hash3(bytes, position)];
    let mut searched = 0_usize;
    let mut best_length = 0_usize;
    let mut best_distance = 0_usize;
    let max_length = MAX_MATCH.min(bytes.len() - position);

    while candidate != NONE && candidate < position && searched < search_limit {
        let distance = position - candidate;
        if distance > WINDOW {
            break;
        }
        if bytes[candidate] == bytes[position] {
            let mut length = 1_usize;
            while length < max_length && bytes[candidate + length] == bytes[position + length] {
                length += 1;
            }
            if length >= MIN_MATCH && length > best_length {
                best_length = length;
                best_distance = distance;
                if length == max_length {
                    break;
                }
            }
        }
        candidate = previous[candidate];
        searched += 1;
    }
    (best_length >= MIN_MATCH).then_some((best_distance, best_length))
}

fn insert(bytes: &[u8], position: usize, heads: &mut [usize], previous: &mut [usize]) {
    if position + MIN_MATCH <= bytes.len() {
        let hash = hash3(bytes, position);
        previous[position] = heads[hash];
        heads[hash] = position;
    }
}

fn hash3(bytes: &[u8], position: usize) -> usize {
    let value = (u32::from(bytes[position]) << 16)
        ^ (u32::from(bytes[position + 1]) << 8)
        ^ u32::from(bytes[position + 2]);
    usize::try_from(value.wrapping_mul(0x1E35_A7BD) >> 16).expect("16-bit hash fits usize")
}

fn write_match(writer: &mut BitWriter, length: usize, distance: usize) -> Result<()> {
    let li = range_index(length, &LENGTH_BASE, &LENGTH_EXTRA)
        .ok_or_else(|| MediaError::invalid_argument("DEFLATE match length out of range"))?;
    write_fixed_symbol(
        writer,
        u16::try_from(257 + li).map_err(|_| MediaError::overflow("length symbol overflow"))?,
    );
    if LENGTH_EXTRA[li] != 0 {
        writer.write_bits_lsb(
            u32::try_from(length - LENGTH_BASE[li])
                .map_err(|_| MediaError::overflow("length extra overflow"))?,
            LENGTH_EXTRA[li],
        );
    }
    let di = range_index(distance, &DISTANCE_BASE, &DISTANCE_EXTRA)
        .ok_or_else(|| MediaError::invalid_argument("DEFLATE distance out of range"))?;
    writer.write_huffman(
        u16::try_from(di).map_err(|_| MediaError::overflow("distance symbol overflow"))?,
        5,
    );
    if DISTANCE_EXTRA[di] != 0 {
        writer.write_bits_lsb(
            u32::try_from(distance - DISTANCE_BASE[di])
                .map_err(|_| MediaError::overflow("distance extra overflow"))?,
            DISTANCE_EXTRA[di],
        );
    }
    Ok(())
}

fn range_index<const N: usize>(value: usize, base: &[usize; N], extra: &[u8; N]) -> Option<usize> {
    base.iter().enumerate().find_map(|(i, &start)| {
        let count = if extra[i] == 0 {
            1
        } else {
            1_usize << extra[i]
        };
        let end = start.checked_add(count - 1)?;
        (value >= start && value <= end).then_some(i)
    })
}

fn write_fixed_symbol(writer: &mut BitWriter, symbol: u16) {
    match symbol {
        0..=143 => writer.write_huffman(0x30 + symbol, 8),
        144..=255 => writer.write_huffman(0x190 + (symbol - 144), 9),
        256..=279 => writer.write_huffman(symbol - 256, 7),
        280..=287 => writer.write_huffman(0xC0 + (symbol - 280), 8),
        _ => unreachable!(),
    }
}

#[derive(Default)]
struct BitWriter {
    bytes: Vec<u8>,
    current: u8,
    used: u8,
}
impl BitWriter {
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
    fn every_effort_level_round_trips() {
        let source = b"abcdefghabcdefghabcdefgh-0123456789-abcdefghabcdefgh".repeat(500);
        for level in 1..=9 {
            let encoded = compress_fixed_level(&source, level).unwrap();
            assert_eq!(
                crate::deflate::inflate(&encoded, source.len()).unwrap(),
                source
            );
        }
    }

    #[test]
    fn higher_effort_never_regresses_reference_fixture_size() {
        let mut source = Vec::new();
        for i in 0..5000_u32 {
            source.extend_from_slice(
                format!("row-{i:04}-common-common-common-row-{i:04}\n").as_bytes(),
            );
        }
        let low = compress_fixed_level(&source, 1).unwrap();
        let high = compress_fixed_level(&source, 9).unwrap();
        assert!(high.len() <= low.len());
    }
}
