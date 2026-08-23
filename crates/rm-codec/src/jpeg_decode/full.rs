#![allow(clippy::needless_range_loop)]

use rm_core::video::{PixelFormat, VideoFrame};
use rm_core::{MediaError, Result};

const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34,
    27, 20, 13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37,
    44, 51, 58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

#[derive(Clone)]
struct Huffman {
    counts: [u8; 16],
    symbols: Vec<u8>,
}

#[derive(Clone)]
struct Component {
    id: u8,
    h: u8,
    v: u8,
    tq: u8,
    padded_blocks_x: usize,
    padded_blocks_y: usize,
    actual_blocks_x: usize,
    actual_blocks_y: usize,
    coeffs: Vec<[i32; 64]>,
    dc_seen: bool,
    ac_seen: [bool; 64],
}

#[derive(Clone, Copy)]
struct ScanComponent {
    ci: usize,
    dc_table: usize,
    ac_table: usize,
}

struct Frame {
    width: usize,
    height: usize,
    progressive: bool,
    max_h: u8,
    max_v: u8,
    mcus_x: usize,
    mcus_y: usize,
    components: Vec<Component>,
}

struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    bit: u8,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0, bit: 0 }
    }

    fn read_bit(&mut self) -> Result<u8> {
        let byte = *self
            .data
            .get(self.pos)
            .ok_or_else(|| MediaError::eof("truncated JPEG entropy data"))?;
        let out = (byte >> (7 - self.bit)) & 1;
        self.bit += 1;
        if self.bit == 8 {
            self.bit = 0;
            self.pos += 1;
        }
        Ok(out)
    }

    fn read_bits(&mut self, count: u8) -> Result<u32> {
        let mut value = 0u32;
        for _ in 0..count {
            value = (value << 1) | u32::from(self.read_bit()?);
        }
        Ok(value)
    }
}

fn decode_huff(reader: &mut BitReader<'_>, table: &Huffman) -> Result<u8> {
    let mut code = 0u32;
    let mut first = 0u32;
    let mut offset = 0usize;
    for len in 1..=16 {
        code = (code << 1) | u32::from(reader.read_bit()?);
        let count = u32::from(table.counts[len - 1]);
        if code >= first && code < first + count {
            let index = offset
                .checked_add(usize::try_from(code - first).unwrap())
                .ok_or_else(|| MediaError::overflow("JPEG Huffman index overflow"))?;
            return table
                .symbols
                .get(index)
                .copied()
                .ok_or_else(|| MediaError::invalid_data("JPEG Huffman symbol index out of range"));
        }
        offset += usize::try_from(count).unwrap();
        first = (first + count) << 1;
    }
    Err(MediaError::invalid_data("invalid JPEG Huffman code"))
}

fn receive_extend(reader: &mut BitReader<'_>, count: u8) -> Result<i32> {
    if count == 0 {
        return Ok(0);
    }
    if count > 16 {
        return Err(MediaError::invalid_data("JPEG coefficient magnitude is too large"));
    }
    let value = reader.read_bits(count)? as i32;
    let threshold = 1i32 << (count - 1);
    if value < threshold {
        Ok(value - ((1i32 << count) - 1))
    } else {
        Ok(value)
    }
}

struct Entropy {
    segments: Vec<Vec<u8>>,
    restarts: Vec<u8>,
}

fn entropy_end(bytes: &[u8], start: usize) -> Result<usize> {
    let mut pos = start;
    while pos < bytes.len() {
        if bytes[pos] != 0xff {
            pos += 1;
            continue;
        }
        let mut next = pos + 1;
        while bytes.get(next) == Some(&0xff) {
            next += 1;
        }
        let code = *bytes
            .get(next)
            .ok_or_else(|| MediaError::eof("truncated JPEG entropy marker"))?;
        if code == 0x00 || (0xd0..=0xd7).contains(&code) {
            pos = next + 1;
            continue;
        }
        return Ok(pos);
    }
    Err(MediaError::eof("JPEG scan has no following marker"))
}

fn split_entropy(raw: &[u8]) -> Result<Entropy> {
    let mut segments = vec![Vec::with_capacity(raw.len())];
    let mut restarts = Vec::new();
    let mut pos = 0usize;
    while pos < raw.len() {
        if raw[pos] != 0xff {
            segments.last_mut().unwrap().push(raw[pos]);
            pos += 1;
            continue;
        }
        let mut next = pos + 1;
        while raw.get(next) == Some(&0xff) {
            next += 1;
        }
        let code = *raw
            .get(next)
            .ok_or_else(|| MediaError::eof("truncated JPEG entropy marker"))?;
        match code {
            0x00 => {
                segments.last_mut().unwrap().push(0xff);
                pos = next + 1;
            }
            0xd0..=0xd7 => {
                restarts.push(code);
                segments.push(Vec::new());
                pos = next + 1;
            }
            _ => return Err(MediaError::invalid_data("unexpected marker in JPEG scan payload")),
        }
    }
    Ok(Entropy { segments, restarts })
}

fn parse_dqt(data: &[u8], tables: &mut [Option<[u16; 64]>; 4]) -> Result<()> {
    let mut pos = 0usize;
    while pos < data.len() {
        let spec = *data.get(pos).ok_or_else(|| MediaError::eof("truncated JPEG DQT"))?;
        pos += 1;
        let precision = spec >> 4;
        let id = usize::from(spec & 0x0f);
        if precision > 1 || id > 3 {
            return Err(MediaError::invalid_data("invalid JPEG DQT table specifier"));
        }
        let mut table = [0u16; 64];
        for zz in 0..64 {
            let value = if precision == 0 {
                let value = *data.get(pos).ok_or_else(|| MediaError::eof("truncated JPEG DQT"))?;
                pos += 1;
                u16::from(value)
            } else {
                let pair = data
                    .get(pos..pos + 2)
                    .ok_or_else(|| MediaError::eof("truncated JPEG DQT"))?;
                pos += 2;
                u16::from_be_bytes([pair[0], pair[1]])
            };
            if value == 0 {
                return Err(MediaError::invalid_data("JPEG quantizer cannot contain zero"));
            }
            table[ZIGZAG[zz]] = value;
        }
        tables[id] = Some(table);
    }
    Ok(())
}

fn parse_dht(data: &[u8], dc: &mut [Option<Huffman>; 4], ac: &mut [Option<Huffman>; 4]) -> Result<()> {
    let mut pos = 0usize;
    while pos < data.len() {
        let spec = *data.get(pos).ok_or_else(|| MediaError::eof("truncated JPEG DHT"))?;
        pos += 1;
        let class = spec >> 4;
        let id = usize::from(spec & 0x0f);
        if class > 1 || id > 3 {
            return Err(MediaError::invalid_data("invalid JPEG DHT table specifier"));
        }
        let counts_slice = data
            .get(pos..pos + 16)
            .ok_or_else(|| MediaError::eof("truncated JPEG DHT counts"))?;
        pos += 16;
        let mut counts = [0u8; 16];
        counts.copy_from_slice(counts_slice);
        let symbol_count: usize = counts.iter().map(|&value| usize::from(value)).sum();
        if symbol_count == 0 || symbol_count > 256 {
            return Err(MediaError::invalid_data("invalid JPEG Huffman symbol count"));
        }
        let symbols = data
            .get(pos..pos + symbol_count)
            .ok_or_else(|| MediaError::eof("truncated JPEG DHT symbols"))?
            .to_vec();
        pos += symbol_count;
        let table = Huffman { counts, symbols };
        if class == 0 {
            dc[id] = Some(table);
        } else {
            ac[id] = Some(table);
        }
    }
    Ok(())
}

fn parse_frame(marker: u8, data: &[u8]) -> Result<Frame> {
    if data.len() < 6 || data[0] != 8 {
        return Err(MediaError::unsupported("only 8-bit Huffman JPEG frames are implemented"));
    }
    let height = usize::from(u16::from_be_bytes([data[1], data[2]]));
    let width = usize::from(u16::from_be_bytes([data[3], data[4]]));
    let count = usize::from(data[5]);
    if width == 0 || height == 0 || !(count == 1 || count == 3) {
        return Err(MediaError::unsupported("JPEG frame must contain 1 or 3 components"));
    }
    if data.len() != 6 + count * 3 {
        return Err(MediaError::invalid_data("JPEG SOF component table length mismatch"));
    }
    let mut specs = Vec::with_capacity(count);
    let mut max_h = 0u8;
    let mut max_v = 0u8;
    for item in data[6..].chunks_exact(3) {
        let h = item[1] >> 4;
        let v = item[1] & 0x0f;
        if h == 0 || v == 0 || h > 4 || v > 4 || item[2] > 3 {
            return Err(MediaError::invalid_data("invalid JPEG component sampling/table selector"));
        }
        if specs.iter().any(|&(id, _, _, _)| id == item[0]) {
            return Err(MediaError::invalid_data("duplicate JPEG component id"));
        }
        max_h = max_h.max(h);
        max_v = max_v.max(v);
        specs.push((item[0], h, v, item[2]));
    }
    let mcu_w = 8usize * usize::from(max_h);
    let mcu_h = 8usize * usize::from(max_v);
    let mcus_x = width.div_ceil(mcu_w);
    let mcus_y = height.div_ceil(mcu_h);
    let mut components = Vec::with_capacity(count);
    for (id, h, v, tq) in specs {
        let padded_blocks_x = mcus_x * usize::from(h);
        let padded_blocks_y = mcus_y * usize::from(v);
        let actual_blocks_x = (width * usize::from(h)).div_ceil(8 * usize::from(max_h));
        let actual_blocks_y = (height * usize::from(v)).div_ceil(8 * usize::from(max_v));
        let block_count = padded_blocks_x
            .checked_mul(padded_blocks_y)
            .ok_or_else(|| MediaError::overflow("JPEG coefficient storage overflow"))?;
        components.push(Component {
            id,
            h,
            v,
            tq,
            padded_blocks_x,
            padded_blocks_y,
            actual_blocks_x,
            actual_blocks_y,
            coeffs: vec![[0i32; 64]; block_count],
            dc_seen: false,
            ac_seen: [false; 64],
        });
    }
    Ok(Frame {
        width,
        height,
        progressive: marker == 0xc2,
        max_h,
        max_v,
        mcus_x,
        mcus_y,
        components,
    })
}

fn parse_scan_header(frame: &Frame, data: &[u8]) -> Result<(Vec<ScanComponent>, u8, u8, u8, u8)> {
    let count = usize::from(*data.first().ok_or_else(|| MediaError::eof("empty JPEG SOS"))?);
    if count == 0 || count > frame.components.len() || data.len() != 1 + count * 2 + 3 {
        return Err(MediaError::invalid_data("invalid JPEG SOS component table"));
    }
    let mut scan = Vec::with_capacity(count);
    for i in 0..count {
        let id = data[1 + i * 2];
        let selectors = data[2 + i * 2];
        let ci = frame
            .components
            .iter()
            .position(|component| component.id == id)
            .ok_or_else(|| MediaError::invalid_data("JPEG SOS references unknown component"))?;
        if scan.iter().any(|entry: &ScanComponent| entry.ci == ci) {
            return Err(MediaError::invalid_data("duplicate JPEG scan component"));
        }
        let dc_table = usize::from(selectors >> 4);
        let ac_table = usize::from(selectors & 0x0f);
        if dc_table > 3 || ac_table > 3 {
            return Err(MediaError::invalid_data("invalid JPEG scan Huffman selector"));
        }
        scan.push(ScanComponent { ci, dc_table, ac_table });
    }
    let ss = data[1 + count * 2];
    let se = data[2 + count * 2];
    let ah_al = data[3 + count * 2];
    let ah = ah_al >> 4;
    let al = ah_al & 0x0f;
    if ss > se || se > 63 || ah > 13 || al > 13 {
        return Err(MediaError::invalid_data("invalid JPEG spectral/refinement parameters"));
    }
    if ss != 0 && scan.len() != 1 {
        return Err(MediaError::invalid_data("JPEG AC scans must contain one component"));
    }
    if !frame.progressive && (ss != 0 || se != 63 || ah != 0 || al != 0) {
        return Err(MediaError::unsupported("non-progressive JPEG scan is not sequential"));
    }
    if frame.progressive && ah != 0 && ah != al + 1 {
        return Err(MediaError::invalid_data("invalid JPEG progressive successive approximation"));
    }
    Ok((scan, ss, se, ah, al))
}

fn block_index(frame: &Frame, scan: &[ScanComponent], mcu: usize, sub: usize) -> Result<(usize, usize)> {
    if scan.len() == 1 {
        let ci = scan[0].ci;
        let component = &frame.components[ci];
        let x = mcu % component.actual_blocks_x;
        let y = mcu / component.actual_blocks_x;
        if y >= component.actual_blocks_y || sub != 0 {
            return Err(MediaError::invalid_data("JPEG non-interleaved block index out of range"));
        }
        return Ok((ci, y * component.padded_blocks_x + x));
    }
    let ci = scan[sub].ci;
    let component = &frame.components[ci];
    let mx = mcu % frame.mcus_x;
    let my = mcu / frame.mcus_x;
    if my >= frame.mcus_y {
        return Err(MediaError::invalid_data("JPEG MCU index out of range"));
    }
    Ok((ci, my * component.padded_blocks_x + mx))
}

fn scan_mcus(frame: &Frame, scan: &[ScanComponent]) -> Result<usize> {
    if scan.len() == 1 {
        let component = &frame.components[scan[0].ci];
        component
            .actual_blocks_x
            .checked_mul(component.actual_blocks_y)
            .ok_or_else(|| MediaError::overflow("JPEG scan block count overflow"))
    } else {
        frame
            .mcus_x
            .checked_mul(frame.mcus_y)
            .ok_or_else(|| MediaError::overflow("JPEG MCU count overflow"))
    }
}

fn validate_restarts(entropy: &Entropy, mcu_count: usize, interval: u16) -> Result<()> {
    let expected = if interval == 0 || mcu_count == 0 {
        0
    } else {
        (mcu_count - 1) / usize::from(interval)
    };
    if entropy.restarts.len() != expected || entropy.segments.len() != expected + 1 {
        return Err(MediaError::invalid_data("JPEG restart marker count mismatch"));
    }
    for (index, &marker) in entropy.restarts.iter().enumerate() {
        if marker != 0xd0 + u8::try_from(index % 8).unwrap() {
            return Err(MediaError::invalid_data("JPEG restart marker sequence mismatch"));
        }
    }
    Ok(())
}

fn decode_sequential_block(
    reader: &mut BitReader<'_>,
    block: &mut [i32; 64],
    predictor: &mut i32,
    dc: &Huffman,
    ac: &Huffman,
) -> Result<()> {
    let size = decode_huff(reader, dc)?;
    if size > 11 {
        return Err(MediaError::invalid_data("invalid JPEG DC magnitude"));
    }
    *predictor += receive_extend(reader, size)?;
    block[0] = *predictor;
    let mut k = 1usize;
    while k < 64 {
        let rs = decode_huff(reader, ac)?;
        let run = usize::from(rs >> 4);
        let size = rs & 0x0f;
        if size == 0 {
            if run == 15 {
                k = k
                    .checked_add(16)
                    .ok_or_else(|| MediaError::overflow("JPEG AC run overflow"))?;
                if k > 64 {
                    return Err(MediaError::invalid_data("JPEG AC run exceeds block"));
                }
                continue;
            }
            break;
        }
        if size > 10 {
            return Err(MediaError::invalid_data("invalid baseline JPEG AC magnitude"));
        }
        k += run;
        if k >= 64 {
            return Err(MediaError::invalid_data("JPEG AC run exceeds block"));
        }
        block[ZIGZAG[k]] = receive_extend(reader, size)?;
        k += 1;
    }
    Ok(())
}

fn refine_nonzero(reader: &mut BitReader<'_>, coefficient: &mut i32, bit: i32) -> Result<()> {
    if reader.read_bit()? != 0 && (*coefficient & bit) == 0 {
        if *coefficient > 0 {
            *coefficient += bit;
        } else {
            *coefficient -= bit;
        }
    }
    Ok(())
}

fn decode_progressive_block(
    reader: &mut BitReader<'_>,
    block: &mut [i32; 64],
    predictor: &mut i32,
    dc_table: Option<&Huffman>,
    ac_table: Option<&Huffman>,
    ss: u8,
    se: u8,
    ah: u8,
    al: u8,
    eob_run: &mut u32,
) -> Result<()> {
    let bit = 1i32 << al;
    if ss == 0 {
        if ah == 0 {
            let table = dc_table.ok_or_else(|| MediaError::invalid_data("missing progressive JPEG DC table"))?;
            let size = decode_huff(reader, table)?;
            if size > 11 {
                return Err(MediaError::invalid_data("invalid progressive JPEG DC magnitude"));
            }
            *predictor += receive_extend(reader, size)?;
            block[0] = *predictor << al;
        } else if reader.read_bit()? != 0 {
            block[0] |= bit;
        }
        return Ok(());
    }

    let table = ac_table.ok_or_else(|| MediaError::invalid_data("missing progressive JPEG AC table"))?;
    let start = usize::from(ss);
    let end = usize::from(se);
    if ah == 0 {
        if *eob_run != 0 {
            *eob_run -= 1;
            return Ok(());
        }
        let mut k = start;
        while k <= end {
            let rs = decode_huff(reader, table)?;
            let run = usize::from(rs >> 4);
            let size = rs & 0x0f;
            if size == 0 {
                if run == 15 {
                    k += 16;
                    continue;
                }
                *eob_run = (1u32 << run)
                    .checked_add(if run == 0 { 0 } else { reader.read_bits(run as u8)? })
                    .ok_or_else(|| MediaError::overflow("JPEG EOB run overflow"))?;
                *eob_run -= 1;
                break;
            }
            if size > 10 {
                return Err(MediaError::invalid_data("invalid progressive JPEG AC magnitude"));
            }
            k += run;
            if k > end {
                return Err(MediaError::invalid_data("progressive JPEG AC run exceeds spectral band"));
            }
            block[ZIGZAG[k]] = receive_extend(reader, size)? << al;
            k += 1;
        }
        return Ok(());
    }

    if *eob_run != 0 {
        *eob_run -= 1;
        for k in start..=end {
            let coefficient = &mut block[ZIGZAG[k]];
            if *coefficient != 0 {
                refine_nonzero(reader, coefficient, bit)?;
            }
        }
        return Ok(());
    }

    let mut k = start;
    while k <= end {
        let rs = decode_huff(reader, table)?;
        let mut run = usize::from(rs >> 4);
        let size = rs & 0x0f;
        let mut new_value = None;
        if size == 0 {
            if run != 15 {
                *eob_run = (1u32 << run)
                    .checked_add(if run == 0 { 0 } else { reader.read_bits(run as u8)? })
                    .ok_or_else(|| MediaError::overflow("JPEG EOB run overflow"))?;
                *eob_run -= 1;
                run = usize::MAX;
            }
        } else {
            if size != 1 {
                return Err(MediaError::invalid_data("progressive AC refinement introduces non-unit coefficient"));
            }
            new_value = Some(if reader.read_bit()? != 0 { bit } else { -bit });
        }

        while k <= end {
            let coefficient = &mut block[ZIGZAG[k]];
            if *coefficient != 0 {
                refine_nonzero(reader, coefficient, bit)?;
            } else if run == 0 {
                if let Some(value) = new_value.take() {
                    *coefficient = value;
                }
                k += 1;
                break;
            } else if run != usize::MAX {
                run -= 1;
            }
            k += 1;
        }
        if run == usize::MAX {
            for rest in k..=end {
                let coefficient = &mut block[ZIGZAG[rest]];
                if *coefficient != 0 {
                    refine_nonzero(reader, coefficient, bit)?;
                }
            }
            break;
        }
    }
    Ok(())
}

fn decode_scan(
    frame: &mut Frame,
    scan: &[ScanComponent],
    ss: u8,
    se: u8,
    ah: u8,
    al: u8,
    entropy: Entropy,
    restart_interval: u16,
    dc_tables: &[Option<Huffman>; 4],
    ac_tables: &[Option<Huffman>; 4],
) -> Result<()> {
    let mcu_count = scan_mcus(frame, scan)?;
    validate_restarts(&entropy, mcu_count, restart_interval)?;
    let mut predictors = vec![0i32; frame.components.len()];
    let mut eob_run = 0u32;
    let mut segment_index = 0usize;
    let mut reader = BitReader::new(&entropy.segments[0]);

    for mcu in 0..mcu_count {
        if mcu != 0 && restart_interval != 0 && mcu.is_multiple_of(usize::from(restart_interval)) {
            segment_index += 1;
            predictors.fill(0);
            eob_run = 0;
            reader = BitReader::new(&entropy.segments[segment_index]);
        }
        if scan.len() == 1 {
            let entry = scan[0];
            let (ci, bi) = block_index(frame, scan, mcu, 0)?;
            let dc = dc_tables[entry.dc_table].as_ref();
            let ac = ac_tables[entry.ac_table].as_ref();
            let block = &mut frame.components[ci].coeffs[bi];
            if frame.progressive {
                decode_progressive_block(
                    &mut reader,
                    block,
                    &mut predictors[ci],
                    dc,
                    ac,
                    ss,
                    se,
                    ah,
                    al,
                    &mut eob_run,
                )?;
            } else {
                decode_sequential_block(
                    &mut reader,
                    block,
                    &mut predictors[ci],
                    dc.ok_or_else(|| MediaError::invalid_data("missing JPEG DC table"))?,
                    ac.ok_or_else(|| MediaError::invalid_data("missing JPEG AC table"))?,
                )?;
            }
        } else {
            if frame.progressive && !(ss == 0 && se == 0) {
                return Err(MediaError::invalid_data("progressive interleaved scan may contain DC only"));
            }
            let mut entry_index = 0usize;
            for entry in scan {
                let ci = entry.ci;
                let h = usize::from(frame.components[ci].h);
                let v = usize::from(frame.components[ci].v);
                let mx = mcu % frame.mcus_x;
                let my = mcu / frame.mcus_x;
                for by in 0..v {
                    for bx in 0..h {
                        let bi = (my * v + by) * frame.components[ci].padded_blocks_x + (mx * h + bx);
                        let block = &mut frame.components[ci].coeffs[bi];
                        let dc = dc_tables[entry.dc_table].as_ref();
                        let ac = ac_tables[entry.ac_table].as_ref();
                        if frame.progressive {
                            decode_progressive_block(
                                &mut reader,
                                block,
                                &mut predictors[ci],
                                dc,
                                ac,
                                ss,
                                se,
                                ah,
                                al,
                                &mut eob_run,
                            )?;
                        } else {
                            decode_sequential_block(
                                &mut reader,
                                block,
                                &mut predictors[ci],
                                dc.ok_or_else(|| MediaError::invalid_data("missing JPEG DC table"))?,
                                ac.ok_or_else(|| MediaError::invalid_data("missing JPEG AC table"))?,
                            )?;
                        }
                    }
                }
                entry_index += 1;
            }
            let _ = entry_index;
        }
    }

    for entry in scan {
        let component = &mut frame.components[entry.ci];
        if frame.progressive {
            if ss == 0 {
                if ah == 0 && component.dc_seen {
                    return Err(MediaError::invalid_data("duplicate progressive JPEG DC first scan"));
                }
                if ah == 0 {
                    component.dc_seen = true;
                } else if !component.dc_seen {
                    return Err(MediaError::invalid_data("progressive JPEG DC refinement precedes first scan"));
                }
            } else {
                for k in usize::from(ss)..=usize::from(se) {
                    if ah == 0 && component.ac_seen[k] {
                        return Err(MediaError::invalid_data("overlapping progressive JPEG AC first scans"));
                    }
                    if ah != 0 && !component.ac_seen[k] {
                        return Err(MediaError::invalid_data("progressive JPEG AC refinement precedes first scan"));
                    }
                }
                if ah == 0 {
                    for k in usize::from(ss)..=usize::from(se) {
                        component.ac_seen[k] = true;
                    }
                }
            }
        }
    }
    Ok(())
}

fn idct(coefficients: &[i32; 64]) -> [u8; 64] {
    let mut out = [0u8; 64];
    let pi = std::f64::consts::PI;
    for y in 0..8 {
        for x in 0..8 {
            let mut sum = 0.0;
            for v in 0..8 {
                for u in 0..8 {
                    let cu = if u == 0 { 1.0 / 2.0f64.sqrt() } else { 1.0 };
                    let cv = if v == 0 { 1.0 / 2.0f64.sqrt() } else { 1.0 };
                    sum += cu
                        * cv
                        * f64::from(coefficients[v * 8 + u])
                        * (((2 * x + 1) as f64 * u as f64 * pi) / 16.0).cos()
                        * (((2 * y + 1) as f64 * v as f64 * pi) / 16.0).cos();
                }
            }
            out[y * 8 + x] = (sum / 4.0 + 128.0).round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

fn render(frame: Frame, qtables: &[Option<[u16; 64]>; 4]) -> Result<VideoFrame> {
    let mut planes = Vec::with_capacity(frame.components.len());
    for component in &frame.components {
        let qtable = qtables[usize::from(component.tq)]
            .as_ref()
            .ok_or_else(|| MediaError::invalid_data("missing JPEG quantization table"))?;
        let width = component.padded_blocks_x * 8;
        let height = component.padded_blocks_y * 8;
        let mut plane = vec![0u8; width * height];
        for by in 0..component.padded_blocks_y {
            for bx in 0..component.padded_blocks_x {
                let source = &component.coeffs[by * component.padded_blocks_x + bx];
                let mut dequantized = [0i32; 64];
                for k in 0..64 {
                    dequantized[k] = source[k]
                        .checked_mul(i32::from(qtable[k]))
                        .ok_or_else(|| MediaError::overflow("JPEG dequantization overflow"))?;
                }
                let block = idct(&dequantized);
                for y in 0..8 {
                    let dst = (by * 8 + y) * width + bx * 8;
                    plane[dst..dst + 8].copy_from_slice(&block[y * 8..y * 8 + 8]);
                }
            }
        }
        planes.push((plane, width, height));
    }

    if frame.components.len() == 1 {
        let (plane, stride, _) = &planes[0];
        let mut output = vec![0u8; frame.width * frame.height];
        for y in 0..frame.height {
            output[y * frame.width..(y + 1) * frame.width]
                .copy_from_slice(&plane[y * *stride..y * *stride + frame.width]);
        }
        return VideoFrame::from_vec(
            u32::try_from(frame.width).unwrap(),
            u32::try_from(frame.height).unwrap(),
            PixelFormat::Gray8,
            output,
        );
    }

    let mut output = Vec::with_capacity(frame.width * frame.height * 3);
    for y in 0..frame.height {
        for x in 0..frame.width {
            let mut sample = [0u8; 3];
            for ci in 0..3 {
                let component = &frame.components[ci];
                let sx = x * usize::from(component.h) / usize::from(frame.max_h);
                let sy = y * usize::from(component.v) / usize::from(frame.max_v);
                sample[ci] = planes[ci].0[sy * planes[ci].1 + sx];
            }
            let yy = f64::from(sample[0]);
            let cb = f64::from(sample[1]) - 128.0;
            let cr = f64::from(sample[2]) - 128.0;
            output.push((yy + 1.402 * cr).round().clamp(0.0, 255.0) as u8);
            output.push((yy - 0.344_136 * cb - 0.714_136 * cr).round().clamp(0.0, 255.0) as u8);
            output.push((yy + 1.772 * cb).round().clamp(0.0, 255.0) as u8);
        }
    }
    VideoFrame::from_vec(
        u32::try_from(frame.width).unwrap(),
        u32::try_from(frame.height).unwrap(),
        PixelFormat::Rgb24,
        output,
    )
}

pub fn decode_jpeg(bytes: &[u8]) -> Result<VideoFrame> {
    if bytes.len() < 4 || bytes[..2] != [0xff, 0xd8] {
        return Err(MediaError::invalid_data("missing JPEG SOI marker"));
    }
    let mut pos = 2usize;
    let mut frame = None::<Frame>;
    let mut qtables: [Option<[u16; 64]>; 4] = [None, None, None, None];
    let mut dc_tables: [Option<Huffman>; 4] = [None, None, None, None];
    let mut ac_tables: [Option<Huffman>; 4] = [None, None, None, None];
    let mut restart_interval = 0u16;
    let mut saw_scan = false;
    let mut saw_eoi = false;

    while pos < bytes.len() {
        if bytes.get(pos) != Some(&0xff) {
            return Err(MediaError::invalid_data("expected JPEG marker prefix"));
        }
        while bytes.get(pos) == Some(&0xff) {
            pos += 1;
        }
        let marker = *bytes.get(pos).ok_or_else(|| MediaError::eof("truncated JPEG marker"))?;
        pos += 1;
        match marker {
            0xd9 => {
                saw_eoi = true;
                break;
            }
            0xd8 | 0x01 | 0xd0..=0xd7 => {
                return Err(MediaError::invalid_data("unexpected standalone JPEG marker"));
            }
            _ => {}
        }
        let length_bytes = bytes
            .get(pos..pos + 2)
            .ok_or_else(|| MediaError::eof("truncated JPEG segment length"))?;
        let length = usize::from(u16::from_be_bytes([length_bytes[0], length_bytes[1]]));
        if length < 2 {
            return Err(MediaError::invalid_data("invalid JPEG segment length"));
        }
        let end = pos
            .checked_add(length)
            .ok_or_else(|| MediaError::overflow("JPEG segment range overflow"))?;
        let payload = bytes
            .get(pos + 2..end)
            .ok_or_else(|| MediaError::eof("truncated JPEG segment"))?;
        pos = end;
        match marker {
            0xdb => parse_dqt(payload, &mut qtables)?,
            0xc4 => parse_dht(payload, &mut dc_tables, &mut ac_tables)?,
            0xdd => {
                if payload.len() != 2 {
                    return Err(MediaError::invalid_data("JPEG DRI payload must be 2 bytes"));
                }
                restart_interval = u16::from_be_bytes([payload[0], payload[1]]);
            }
            0xc0 | 0xc2 => {
                if frame.is_some() {
                    return Err(MediaError::invalid_data("multiple JPEG frame headers"));
                }
                frame = Some(parse_frame(marker, payload)?);
            }
            0xda => {
                let current = frame
                    .as_mut()
                    .ok_or_else(|| MediaError::invalid_data("JPEG SOS before frame header"))?;
                let (scan, ss, se, ah, al) = parse_scan_header(current, payload)?;
                let scan_end = entropy_end(bytes, pos)?;
                let entropy = split_entropy(&bytes[pos..scan_end])?;
                decode_scan(
                    current,
                    &scan,
                    ss,
                    se,
                    ah,
                    al,
                    entropy,
                    restart_interval,
                    &dc_tables,
                    &ac_tables,
                )?;
                saw_scan = true;
                pos = scan_end;
            }
            0xc1 | 0xc3 | 0xc5..=0xc7 | 0xc9..=0xcf => {
                return Err(MediaError::unsupported(format!("unsupported JPEG frame marker 0xff{marker:02x}")));
            }
            _ => {}
        }
    }

    if !saw_eoi || pos != bytes.len() {
        return Err(MediaError::invalid_data("JPEG stream is missing clean EOI termination"));
    }
    if !saw_scan {
        return Err(MediaError::invalid_data("JPEG stream contains no scans"));
    }
    let frame = frame.ok_or_else(|| MediaError::invalid_data("JPEG stream contains no frame"))?;
    if frame.progressive {
        for component in &frame.components {
            if !component.dc_seen {
                return Err(MediaError::invalid_data("progressive JPEG component has no DC first scan"));
            }
        }
    }
    render(frame, &qtables)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_marker_stream_is_rejected() {
        assert!(decode_jpeg(&[0xff, 0xd8, 0x00, 0xff, 0xd9]).is_err());
    }

    #[test]
    fn restart_sequence_validation_rejects_wrong_marker_order() {
        let entropy = Entropy {
            segments: vec![vec![0], vec![0]],
            restarts: vec![0xd1],
        };
        assert!(validate_restarts(&entropy, 2, 1).is_err());
    }
}
