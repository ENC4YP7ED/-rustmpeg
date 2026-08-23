#![allow(clippy::needless_range_loop)]

use rm_core::video::{PixelFormat, VideoFrame};
use rm_core::{MediaError, Result};

const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34,
    27, 20, 13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51,
    58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

#[derive(Clone, Copy)]
struct Component {
    id: u8,
    h: u8,
    v: u8,
    tq: u8,
    dc_table: u8,
    ac_table: u8,
}

#[derive(Clone)]
struct Huffman {
    counts: [u8; 16],
    symbols: Vec<u8>,
}

struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    bit: u8,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self { Self { data, pos: 0, bit: 0 } }

    fn read_bit(&mut self) -> Result<u8> {
        let byte = *self.data.get(self.pos).ok_or_else(|| MediaError::eof("truncated JPEG entropy data"))?;
        let value = (byte >> (7 - self.bit)) & 1;
        self.bit += 1;
        if self.bit == 8 { self.bit = 0; self.pos += 1; }
        Ok(value)
    }

    fn read_bits(&mut self, n: u8) -> Result<u32> {
        let mut out = 0u32;
        for _ in 0..n { out = (out << 1) | u32::from(self.read_bit()?); }
        Ok(out)
    }
}

fn receive_extend(reader: &mut BitReader<'_>, n: u8) -> Result<i32> {
    if n == 0 { return Ok(0); }
    let value = reader.read_bits(n)? as i32;
    let threshold = 1i32 << (n - 1);
    if value < threshold { Ok(value - ((1i32 << n) - 1)) } else { Ok(value) }
}

fn decode_huff(reader: &mut BitReader<'_>, table: &Huffman) -> Result<u8> {
    let mut code = 0u32;
    let mut first = 0u32;
    let mut index = 0usize;
    for len in 1..=16 {
        code = (code << 1) | u32::from(reader.read_bit()?);
        let count = u32::from(table.counts[len - 1]);
        if code >= first && code < first + count {
            return table.symbols.get(index + usize::try_from(code - first).unwrap()).copied()
                .ok_or_else(|| MediaError::invalid_data("JPEG Huffman symbol index out of range"));
        }
        index += usize::try_from(count).unwrap();
        first = (first + count) << 1;
    }
    Err(MediaError::invalid_data("invalid JPEG Huffman code"))
}

fn idct(coeff: &[i32; 64]) -> [u8; 64] {
    let mut out = [0u8; 64];
    let pi = std::f64::consts::PI;
    for y in 0..8 {
        for x in 0..8 {
            let mut sum = 0.0f64;
            for v in 0..8 {
                for u in 0..8 {
                    let cu = if u == 0 { 1.0 / 2.0f64.sqrt() } else { 1.0 };
                    let cv = if v == 0 { 1.0 / 2.0f64.sqrt() } else { 1.0 };
                    let a = ((2 * x + 1) as f64 * u as f64 * pi / 16.0).cos();
                    let b = ((2 * y + 1) as f64 * v as f64 * pi / 16.0).cos();
                    sum += cu * cv * coeff[v * 8 + u] as f64 * a * b;
                }
            }
            out[y * 8 + x] = (sum / 4.0 + 128.0).round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

fn entropy_unstuff(data: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0usize;
    while i < data.len() {
        if data[i] != 0xff { out.push(data[i]); i += 1; continue; }
        let next = *data.get(i + 1).ok_or_else(|| MediaError::eof("truncated JPEG entropy byte stuffing"))?;
        match next {
            0x00 => { out.push(0xff); i += 2; }
            0xd0..=0xd7 => { i += 2; }
            _ => return Err(MediaError::invalid_data("unexpected marker inside JPEG entropy payload")),
        }
    }
    Ok(out)
}

pub fn decode_jpeg(bytes: &[u8]) -> Result<VideoFrame> {
    if bytes.len() < 4 || bytes[..2] != [0xff, 0xd8] { return Err(MediaError::invalid_data("missing JPEG SOI")); }
    let mut pos = 2usize;
    let mut width = 0u16;
    let mut height = 0u16;
    let mut components = Vec::<Component>::new();
    let mut qtables: [Option<[u16; 64]>; 4] = [None, None, None, None];
    let mut dc: [Option<Huffman>; 4] = [None, None, None, None];
    let mut ac: [Option<Huffman>; 4] = [None, None, None, None];
    let mut entropy = None::<Vec<u8>>;

    while pos + 1 < bytes.len() {
        if bytes[pos] != 0xff { return Err(MediaError::invalid_data("expected JPEG marker")); }
        while pos < bytes.len() && bytes[pos] == 0xff { pos += 1; }
        let marker = *bytes.get(pos).ok_or_else(|| MediaError::eof("truncated JPEG marker"))?; pos += 1;
        if marker == 0xd9 { break; }
        if marker == 0xd8 || (0xd0..=0xd7).contains(&marker) { continue; }
        if pos + 2 > bytes.len() { return Err(MediaError::eof("truncated JPEG segment length")); }
        let len = usize::from(u16::from_be_bytes([bytes[pos], bytes[pos + 1]]));
        if len < 2 || pos + len > bytes.len() { return Err(MediaError::eof("truncated JPEG segment")); }
        let seg = &bytes[pos + 2..pos + len];
        pos += len;
        match marker {
            0xdb => {
                let mut p = 0usize;
                while p < seg.len() {
                    let spec = seg[p]; p += 1;
                    let precision = spec >> 4; let id = usize::from(spec & 15);
                    if id > 3 || precision > 1 { return Err(MediaError::invalid_data("invalid JPEG DQT")); }
                    let mut table = [0u16; 64];
                    for zz in 0..64 {
                        let value = if precision == 0 {
                            let v = *seg.get(p).ok_or_else(|| MediaError::eof("truncated JPEG DQT"))?; p += 1; u16::from(v)
                        } else {
                            let s = seg.get(p..p + 2).ok_or_else(|| MediaError::eof("truncated JPEG DQT"))?; p += 2; u16::from_be_bytes([s[0], s[1]])
                        };
                        table[ZIGZAG[zz]] = value;
                    }
                    qtables[id] = Some(table);
                }
            }
            0xc4 => {
                let mut p = 0usize;
                while p < seg.len() {
                    let spec = seg[p]; p += 1;
                    let class = spec >> 4; let id = usize::from(spec & 15);
                    if class > 1 || id > 3 { return Err(MediaError::invalid_data("invalid JPEG DHT")); }
                    let counts_slice = seg.get(p..p + 16).ok_or_else(|| MediaError::eof("truncated JPEG DHT counts"))?; p += 16;
                    let mut counts = [0u8; 16]; counts.copy_from_slice(counts_slice);
                    let n: usize = counts.iter().map(|&v| usize::from(v)).sum();
                    let symbols = seg.get(p..p + n).ok_or_else(|| MediaError::eof("truncated JPEG DHT symbols"))?.to_vec(); p += n;
                    let table = Huffman { counts, symbols };
                    if class == 0 { dc[id] = Some(table); } else { ac[id] = Some(table); }
                }
            }
            0xc0 => {
                if seg.len() < 6 || seg[0] != 8 { return Err(MediaError::unsupported("only 8-bit baseline JPEG is decoded")); }
                height = u16::from_be_bytes([seg[1], seg[2]]); width = u16::from_be_bytes([seg[3], seg[4]]);
                let n = usize::from(seg[5]);
                if n != 1 && n != 3 { return Err(MediaError::unsupported("baseline JPEG supports 1 or 3 components")); }
                if seg.len() != 6 + n * 3 { return Err(MediaError::invalid_data("JPEG SOF0 size mismatch")); }
                components.clear();
                for c in seg[6..].chunks_exact(3) {
                    let h = c[1] >> 4; let v = c[1] & 15;
                    if h == 0 || v == 0 || h > 4 || v > 4 || c[2] > 3 { return Err(MediaError::invalid_data("invalid JPEG component")); }
                    components.push(Component { id: c[0], h, v, tq: c[2], dc_table: 0, ac_table: 0 });
                }
            }
            0xc2 => return Err(MediaError::unsupported("progressive JPEG decode is not implemented yet")),
            0xda => {
                if components.is_empty() { return Err(MediaError::invalid_data("JPEG SOS before SOF0")); }
                let n = usize::from(*seg.first().ok_or_else(|| MediaError::eof("empty JPEG SOS"))?);
                if seg.len() != 1 + n * 2 + 3 || n != components.len() { return Err(MediaError::unsupported("only single-scan interleaved baseline JPEG is decoded")); }
                for i in 0..n {
                    let id = seg[1 + i * 2]; let selectors = seg[2 + i * 2];
                    let c = components.iter_mut().find(|c| c.id == id).ok_or_else(|| MediaError::invalid_data("unknown JPEG SOS component"))?;
                    c.dc_table = selectors >> 4; c.ac_table = selectors & 15;
                }
                if seg[1 + n * 2] != 0 || seg[2 + n * 2] != 63 || seg[3 + n * 2] != 0 { return Err(MediaError::unsupported("non-sequential JPEG scan parameters")); }
                let start = pos;
                let mut end = pos;
                while end + 1 < bytes.len() {
                    if bytes[end] == 0xff {
                        let code = bytes[end + 1];
                        if code == 0x00 || code == 0xff || (0xd0..=0xd7).contains(&code) { end += 2; continue; }
                        break;
                    }
                    end += 1;
                }
                entropy = Some(entropy_unstuff(&bytes[start..end])?);
                pos = end;
            }
            _ => {}
        }
    }

    if width == 0 || height == 0 || components.is_empty() { return Err(MediaError::invalid_data("JPEG missing baseline frame")); }
    let entropy = entropy.ok_or_else(|| MediaError::invalid_data("JPEG missing entropy scan"))?;
    let max_h = components.iter().map(|c| c.h).max().unwrap();
    let max_v = components.iter().map(|c| c.v).max().unwrap();
    let mcu_w = usize::from(max_h) * 8; let mcu_h = usize::from(max_v) * 8;
    let mcus_x = (usize::from(width) + mcu_w - 1) / mcu_w; let mcus_y = (usize::from(height) + mcu_h - 1) / mcu_h;
    let mut planes: Vec<Vec<u8>> = components.iter().map(|c| {
        let pw = mcus_x * usize::from(c.h) * 8; let ph = mcus_y * usize::from(c.v) * 8; vec![0; pw * ph]
    }).collect();
    let plane_widths: Vec<usize> = components.iter().map(|c| mcus_x * usize::from(c.h) * 8).collect();
    let mut predictors = vec![0i32; components.len()];
    let mut br = BitReader::new(&entropy);

    for my in 0..mcus_y {
        for mx in 0..mcus_x {
            for ci in 0..components.len() {
                let c = components[ci];
                let qt = qtables[usize::from(c.tq)].as_ref().ok_or_else(|| MediaError::invalid_data("missing JPEG quantization table"))?;
                let dct = dc[usize::from(c.dc_table)].as_ref().ok_or_else(|| MediaError::invalid_data("missing JPEG DC table"))?;
                let act = ac[usize::from(c.ac_table)].as_ref().ok_or_else(|| MediaError::invalid_data("missing JPEG AC table"))?;
                for by in 0..usize::from(c.v) {
                    for bx in 0..usize::from(c.h) {
                        let mut coeff = [0i32; 64];
                        let s = decode_huff(&mut br, dct)?;
                        if s > 11 { return Err(MediaError::invalid_data("invalid JPEG DC magnitude")); }
                        predictors[ci] += receive_extend(&mut br, s)?;
                        coeff[0] = predictors[ci] * i32::from(qt[0]);
                        let mut k = 1usize;
                        while k < 64 {
                            let rs = decode_huff(&mut br, act)?;
                            let r = usize::from(rs >> 4); let s = rs & 15;
                            if s == 0 {
                                if r == 15 { k += 16; continue; }
                                break;
                            }
                            k += r;
                            if k >= 64 { return Err(MediaError::invalid_data("JPEG AC run exceeds block")); }
                            coeff[ZIGZAG[k]] = receive_extend(&mut br, s)? * i32::from(qt[ZIGZAG[k]]);
                            k += 1;
                        }
                        let block = idct(&coeff);
                        let pw = plane_widths[ci];
                        let ox = (mx * usize::from(c.h) + bx) * 8; let oy = (my * usize::from(c.v) + by) * 8;
                        for y in 0..8 { let dst = (oy + y) * pw + ox; planes[ci][dst..dst + 8].copy_from_slice(&block[y * 8..y * 8 + 8]); }
                    }
                }
            }
        }
    }

    if components.len() == 1 {
        let pw = plane_widths[0]; let mut out = vec![0u8; usize::from(width) * usize::from(height)];
        for y in 0..usize::from(height) { out[y * usize::from(width)..(y + 1) * usize::from(width)].copy_from_slice(&planes[0][y * pw..y * pw + usize::from(width)]); }
        return VideoFrame::from_vec(u32::from(width), u32::from(height), PixelFormat::Gray8, out);
    }

    let mut out = Vec::with_capacity(usize::from(width) * usize::from(height) * 3);
    for y in 0..usize::from(height) {
        for x in 0..usize::from(width) {
            let mut sample = [0u8; 3];
            for ci in 0..3 {
                let c = components[ci];
                let sx = x * usize::from(c.h) / usize::from(max_h); let sy = y * usize::from(c.v) / usize::from(max_v);
                sample[ci] = planes[ci][sy * plane_widths[ci] + sx];
            }
            let yy = f64::from(sample[0]); let cb = f64::from(sample[1]) - 128.0; let cr = f64::from(sample[2]) - 128.0;
            out.push((yy + 1.402 * cr).round().clamp(0.0, 255.0) as u8);
            out.push((yy - 0.344_136 * cb - 0.714_136 * cr).round().clamp(0.0, 255.0) as u8);
            out.push((yy + 1.772 * cb).round().clamp(0.0, 255.0) as u8);
        }
    }
    VideoFrame::from_vec(u32::from(width), u32::from(height), PixelFormat::Rgb24, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_progressive_until_progressive_path_exists() {
        let bytes = [0xff,0xd8,0xff,0xc2,0x00,0x0b,8,0,8,0,8,1,1,0x11,0,0xff,0xd9];
        assert!(decode_jpeg(&bytes).is_err());
    }
}
