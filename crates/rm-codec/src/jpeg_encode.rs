#![allow(clippy::needless_range_loop)]

use rm_core::video::{PixelFormat, VideoFrame};
use rm_core::{MediaError, Result};

const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

const Q_LUMA: [u8; 64] = [
    16,11,10,16,24,40,51,61, 12,12,14,19,26,58,60,55,
    14,13,16,24,40,57,69,56, 14,17,22,29,51,87,80,62,
    18,22,37,56,68,109,103,77, 24,35,55,64,81,104,113,92,
    49,64,78,87,103,121,120,101, 72,92,95,98,112,100,103,99,
];
const Q_CHROMA: [u8; 64] = [
    17,18,24,47,99,99,99,99, 18,21,26,66,99,99,99,99,
    24,26,56,99,99,99,99,99, 47,66,99,99,99,99,99,99,
    99,99,99,99,99,99,99,99, 99,99,99,99,99,99,99,99,
    99,99,99,99,99,99,99,99, 99,99,99,99,99,99,99,99,
];

const DC_L_BITS: [u8; 16] = [0,1,5,1,1,1,1,1,1,0,0,0,0,0,0,0];
const DC_L_VALS: [u8; 12] = [0,1,2,3,4,5,6,7,8,9,10,11];
const DC_C_BITS: [u8; 16] = [0,3,1,1,1,1,1,1,1,1,1,0,0,0,0,0];
const DC_C_VALS: [u8; 12] = [0,1,2,3,4,5,6,7,8,9,10,11];

const AC_L_BITS: [u8; 16] = [0,2,1,3,3,2,4,3,5,5,4,4,0,0,1,125];
const AC_L_VALS: [u8; 162] = [
    1,2,3,0,4,17,5,18,33,49,65,6,19,81,97,7,34,113,20,50,129,145,161,8,35,66,177,193,
    21,82,209,240,36,51,98,114,130,9,10,22,23,24,25,26,37,38,39,40,41,42,52,53,54,55,56,
    57,58,67,68,69,70,71,72,73,74,83,84,85,86,87,88,89,90,99,100,101,102,103,104,105,106,
    115,116,117,118,119,120,121,122,131,132,133,134,135,136,137,138,146,147,148,149,150,151,
    152,153,154,162,163,164,165,166,167,168,169,170,178,179,180,181,182,183,184,185,186,194,
    195,196,197,198,199,200,201,202,210,211,212,213,214,215,216,217,218,225,226,227,228,229,
    230,231,232,233,234,241,242,243,244,245,246,247,248,249,250,
];
const AC_C_BITS: [u8; 16] = [0,2,1,2,4,4,3,4,7,5,4,4,0,1,2,119];
const AC_C_VALS: [u8; 162] = [
    0,1,2,3,17,4,5,33,49,6,18,65,81,7,97,113,19,34,50,129,8,20,66,145,161,177,193,9,35,
    51,82,240,21,98,114,209,10,22,36,52,225,37,241,23,24,25,26,38,39,40,41,42,53,54,55,
    56,57,58,67,68,69,70,71,72,73,74,83,84,85,86,87,88,89,90,99,100,101,102,103,104,105,
    106,115,116,117,118,119,120,121,122,130,131,132,133,134,135,136,137,138,146,147,148,149,
    150,151,152,153,154,162,163,164,165,166,167,168,169,170,178,179,180,181,182,183,184,185,
    186,194,195,196,197,198,199,200,201,202,210,211,212,213,214,215,216,217,218,226,227,228,
    229,230,231,232,233,234,242,243,244,245,246,247,248,249,250,
];

#[derive(Clone, Copy, Default)]
struct Code { bits: u16, len: u8 }

fn build_codes(bits: &[u8;16], values: &[u8]) -> Result<[Code;256]> {
    let mut out = [Code::default();256];
    let mut code = 0u16;
    let mut p = 0usize;
    for len in 1..=16 {
        for _ in 0..bits[len-1] {
            let sym = *values.get(p).ok_or_else(|| MediaError::invalid_data("JPEG Huffman value count mismatch"))?;
            out[usize::from(sym)] = Code { bits: code, len: len as u8 };
            p += 1;
            code = code.checked_add(1).ok_or_else(|| MediaError::overflow("JPEG Huffman code overflow"))?;
        }
        code <<= 1;
    }
    if p != values.len() { return Err(MediaError::invalid_data("JPEG Huffman value count mismatch")); }
    Ok(out)
}

struct BitWriter { out: Vec<u8>, acc: u32, n: u8 }
impl BitWriter {
    fn new() -> Self { Self { out: Vec::new(), acc: 0, n: 0 } }
    fn write(&mut self, bits: u16, len: u8) {
        if len == 0 { return; }
        self.acc = (self.acc << len) | u32::from(bits & ((1u16 << len.min(15)) - 1));
        self.n += len;
        while self.n >= 8 {
            let shift = self.n - 8;
            let byte = ((self.acc >> shift) & 0xff) as u8;
            self.out.push(byte);
            if byte == 0xff { self.out.push(0x00); }
            self.n -= 8;
            self.acc &= if self.n == 0 { 0 } else { (1u32 << self.n) - 1 };
        }
    }
    fn finish(mut self) -> Vec<u8> {
        if self.n != 0 {
            let pad = 8 - self.n;
            let byte = ((self.acc << pad) | ((1u32 << pad) - 1)) as u8;
            self.out.push(byte);
            if byte == 0xff { self.out.push(0x00); }
        }
        self.out
    }
}

fn magnitude(v: i32) -> (u8, u16) {
    if v == 0 { return (0,0); }
    let a = v.unsigned_abs();
    let n = (32 - a.leading_zeros()) as u8;
    if v > 0 { (n, v as u16) } else { (n, ((1u32 << n) - 1 - a) as u16) }
}

fn fdct_quant(block: &[f64;64], q: &[u8;64]) -> [i32;64] {
    let mut out = [0i32;64];
    let pi = std::f64::consts::PI;
    for v in 0..8 { for u in 0..8 {
        let cu = if u == 0 { 1.0 / 2.0f64.sqrt() } else { 1.0 };
        let cv = if v == 0 { 1.0 / 2.0f64.sqrt() } else { 1.0 };
        let mut sum = 0.0;
        for y in 0..8 { for x in 0..8 {
            sum += block[y*8+x]
                * (((2*x+1) as f64 * u as f64 * pi)/16.0).cos()
                * (((2*y+1) as f64 * v as f64 * pi)/16.0).cos();
        }}
        out[v*8+u] = (0.25 * cu * cv * sum / f64::from(q[v*8+u])).round() as i32;
    }}
    out
}

fn emit_block(bw: &mut BitWriter, coeff: &[i32;64], prev_dc: &mut i32, dc: &[Code;256], ac: &[Code;256]) -> Result<()> {
    let delta = coeff[0] - *prev_dc; *prev_dc = coeff[0];
    let (n,bits) = magnitude(delta);
    let c = dc[usize::from(n)];
    if c.len == 0 { return Err(MediaError::invalid_data("JPEG DC category has no Huffman code")); }
    bw.write(c.bits,c.len); bw.write(bits,n);
    let mut run = 0usize;
    for k in 1..64 {
        let v = coeff[ZIGZAG[k]];
        if v == 0 { run += 1; continue; }
        while run >= 16 { let z=ac[0xf0]; bw.write(z.bits,z.len); run -= 16; }
        let (n,bits) = magnitude(v);
        if n > 10 { return Err(MediaError::invalid_data("JPEG AC magnitude exceeds baseline range")); }
        let sym = ((run as u8)<<4)|n;
        let c=ac[usize::from(sym)];
        if c.len == 0 { return Err(MediaError::invalid_data("JPEG AC symbol has no Huffman code")); }
        bw.write(c.bits,c.len); bw.write(bits,n); run=0;
    }
    if run != 0 { let eob=ac[0]; bw.write(eob.bits,eob.len); }
    Ok(())
}

fn marker(out: &mut Vec<u8>, code: u8, payload: &[u8]) -> Result<()> {
    let len = u16::try_from(payload.len()+2).map_err(|_| MediaError::overflow("JPEG segment too large"))?;
    out.extend_from_slice(&[0xff,code]); out.extend_from_slice(&len.to_be_bytes()); out.extend_from_slice(payload); Ok(())
}
fn dht_payload(class:u8,id:u8,bits:&[u8;16],vals:&[u8]) -> Vec<u8> {
    let mut p=Vec::with_capacity(17+vals.len()); p.push((class<<4)|id); p.extend_from_slice(bits); p.extend_from_slice(vals); p
}

pub fn encode_jpeg(frame: &VideoFrame) -> Result<Vec<u8>> {
    if frame.width == 0 || frame.height == 0 { return Err(MediaError::invalid_argument("JPEG dimensions must be non-zero")); }
    if frame.width > 65535 || frame.height > 65535 { return Err(MediaError::unsupported("baseline JPEG dimensions exceed 65535")); }
    let gray = frame.format == PixelFormat::Gray8;
    if !gray && frame.format != PixelFormat::Rgb24 { return Err(MediaError::unsupported("JPEG encoder accepts gray or rgb24")); }
    let dc_l=build_codes(&DC_L_BITS,&DC_L_VALS)?; let ac_l=build_codes(&AC_L_BITS,&AC_L_VALS)?;
    let dc_c=build_codes(&DC_C_BITS,&DC_C_VALS)?; let ac_c=build_codes(&AC_C_BITS,&AC_C_VALS)?;
    let mut out=vec![0xff,0xd8];
    marker(&mut out,0xe0,&[b'J',b'F',b'I',b'F',0,1,1,0,0,1,0,1,0,0])?;
    let mut dqt=Vec::new(); dqt.push(0); for &i in &ZIGZAG { dqt.push(Q_LUMA[i]); }
    if !gray { dqt.push(1); for &i in &ZIGZAG { dqt.push(Q_CHROMA[i]); } }
    marker(&mut out,0xdb,&dqt)?;
    let mut sof=Vec::new(); sof.push(8); sof.extend_from_slice(&(frame.height as u16).to_be_bytes()); sof.extend_from_slice(&(frame.width as u16).to_be_bytes());
    if gray { sof.extend_from_slice(&[1,1,0x11,0]); } else { sof.extend_from_slice(&[3,1,0x11,0,2,0x11,1,3,0x11,1]); }
    marker(&mut out,0xc0,&sof)?;
    marker(&mut out,0xc4,&dht_payload(0,0,&DC_L_BITS,&DC_L_VALS))?; marker(&mut out,0xc4,&dht_payload(1,0,&AC_L_BITS,&AC_L_VALS))?;
    if !gray { marker(&mut out,0xc4,&dht_payload(0,1,&DC_C_BITS,&DC_C_VALS))?; marker(&mut out,0xc4,&dht_payload(1,1,&AC_C_BITS,&AC_C_VALS))?; }
    if gray { marker(&mut out,0xda,&[1,1,0,0,63,0])?; } else { marker(&mut out,0xda,&[3,1,0,2,0x11,3,0x11,0,63,0])?; }
    let w=frame.width as usize; let h=frame.height as usize; let mut bw=BitWriter::new(); let mut prev=[0i32;3];
    for by in (0..h).step_by(8) { for bx in (0..w).step_by(8) {
        if gray {
            let mut b=[0f64;64]; for y in 0..8 { for x in 0..8 { let sx=(bx+x).min(w-1); let sy=(by+y).min(h-1); b[y*8+x]=f64::from(frame.data.as_slice()[sy*frame.linesize+sx])-128.0; }}
            let c=fdct_quant(&b,&Q_LUMA); emit_block(&mut bw,&c,&mut prev[0],&dc_l,&ac_l)?;
        } else {
            let mut planes=[[0f64;64];3]; for y in 0..8 { for x in 0..8 { let sx=(bx+x).min(w-1); let sy=(by+y).min(h-1); let p=sy*frame.linesize+sx*3; let r=f64::from(frame.data.as_slice()[p]); let g=f64::from(frame.data.as_slice()[p+1]); let b=f64::from(frame.data.as_slice()[p+2]); planes[0][y*8+x]=0.299*r+0.587*g+0.114*b-128.0; planes[1][y*8+x]=-0.168736*r-0.331264*g+0.5*b; planes[2][y*8+x]=0.5*r-0.418688*g-0.081312*b; }}
            let yq=fdct_quant(&planes[0],&Q_LUMA); emit_block(&mut bw,&yq,&mut prev[0],&dc_l,&ac_l)?;
            for ci in 1..3 { let cq=fdct_quant(&planes[ci],&Q_CHROMA); emit_block(&mut bw,&cq,&mut prev[ci],&dc_c,&ac_c)?; }
        }
    }}
    out.extend_from_slice(&bw.finish()); out.extend_from_slice(&[0xff,0xd9]); Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jpeg_decode::decode_jpeg;
    #[test] fn grayscale_encodes_and_self_decodes() { let f=VideoFrame::from_vec(8,8,PixelFormat::Gray8,vec![128;64]).unwrap(); let j=encode_jpeg(&f).unwrap(); assert_eq!(&j[..2],&[0xff,0xd8]); let d=decode_jpeg(&j).unwrap(); assert_eq!(d.format,PixelFormat::Gray8); assert!(d.data.as_slice().iter().all(|&v| v.abs_diff(128)<=1)); }
    #[test] fn rgb_encodes_and_self_decodes() { let f=VideoFrame::from_vec(8,8,PixelFormat::Rgb24,vec![200,30,10].repeat(64)).unwrap(); let j=encode_jpeg(&f).unwrap(); let d=decode_jpeg(&j).unwrap(); assert_eq!(d.format,PixelFormat::Rgb24); for p in d.data.as_slice().chunks_exact(3) { assert!(p[0].abs_diff(200)<8 && p[1].abs_diff(30)<8 && p[2].abs_diff(10)<8); } }
}
