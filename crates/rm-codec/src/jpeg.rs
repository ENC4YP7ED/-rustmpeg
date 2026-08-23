use rm_core::{MediaError, Result};

const SOI: u8 = 0xd8;
const EOI: u8 = 0xd9;
const SOS: u8 = 0xda;
const DQT: u8 = 0xdb;
const DHT: u8 = 0xc4;
const DRI: u8 = 0xdd;
const SOF0: u8 = 0xc0;
const SOF2: u8 = 0xc2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JpegCoding {
    Baseline,
    Progressive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JpegInfo {
    pub width: u16,
    pub height: u16,
    pub precision: u8,
    pub components: u8,
    pub coding: JpegCoding,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarkerSummary {
    pub quantization_tables: u8,
    pub huffman_tables: u8,
    pub restart_interval: Option<u16>,
    pub scans: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JpegProbe {
    pub info: JpegInfo,
    pub markers: MarkerSummary,
}

#[must_use]
pub fn probe_jpeg(bytes: &[u8]) -> u8 {
    if bytes.len() >= 4 && bytes[0] == 0xff && bytes[1] == SOI && bytes[2] == 0xff {
        100
    } else {
        0
    }
}

pub fn parse_jpeg(bytes: &[u8]) -> Result<JpegProbe> {
    if bytes.len() < 4 || bytes[0] != 0xff || bytes[1] != SOI {
        return Err(MediaError::invalid_data("missing JPEG SOI marker"));
    }

    let mut pos = 2usize;
    let mut frame = None;
    let mut qtables = 0u8;
    let mut htables = 0u8;
    let mut restart_interval = None;
    let mut scans = 0u16;
    let mut saw_eoi = false;

    while pos < bytes.len() {
        let marker = next_marker(bytes, &mut pos)?;
        match marker {
            EOI => {
                saw_eoi = true;
                break;
            }
            0x01 | 0xd0..=0xd7 => {
                return Err(MediaError::invalid_data(
                    "standalone JPEG marker found outside entropy-coded scan",
                ));
            }
            SOF0 | SOF2 => {
                let segment = segment(bytes, &mut pos)?;
                if frame.is_some() {
                    return Err(MediaError::invalid_data("multiple JPEG frame headers"));
                }
                frame = Some(parse_frame(marker, segment)?);
            }
            DQT => {
                let segment = segment(bytes, &mut pos)?;
                qtables = qtables.checked_add(count_dqt(segment)?).ok_or_else(|| {
                    MediaError::overflow("JPEG quantization table count overflow")
                })?;
            }
            DHT => {
                let segment = segment(bytes, &mut pos)?;
                htables = htables
                    .checked_add(count_dht(segment)?)
                    .ok_or_else(|| MediaError::overflow("JPEG Huffman table count overflow"))?;
            }
            DRI => {
                let segment = segment(bytes, &mut pos)?;
                if segment.len() != 2 {
                    return Err(MediaError::invalid_data("JPEG DRI payload must be 2 bytes"));
                }
                restart_interval = Some(u16::from_be_bytes([segment[0], segment[1]]));
            }
            SOS => {
                let scan = segment(bytes, &mut pos)?;
                validate_scan_header(scan)?;
                scans = scans
                    .checked_add(1)
                    .ok_or_else(|| MediaError::overflow("JPEG scan count overflow"))?;
                skip_entropy_data(bytes, &mut pos)?;
            }
            0xc1 | 0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf => {
                return Err(MediaError::unsupported(format!(
                    "unsupported JPEG frame marker 0xff{marker:02x}"
                )));
            }
            _ => {
                let _ = segment(bytes, &mut pos)?;
            }
        }
    }

    if !saw_eoi {
        return Err(MediaError::eof("JPEG stream has no EOI marker"));
    }
    if pos != bytes.len() {
        return Err(MediaError::invalid_data(
            "bytes found after JPEG EOI marker",
        ));
    }
    let info = frame.ok_or_else(|| MediaError::invalid_data("JPEG stream has no supported SOF"))?;
    if scans == 0 {
        return Err(MediaError::invalid_data("JPEG stream has no SOS scan"));
    }

    Ok(JpegProbe {
        info,
        markers: MarkerSummary {
            quantization_tables: qtables,
            huffman_tables: htables,
            restart_interval,
            scans,
        },
    })
}

fn next_marker(bytes: &[u8], pos: &mut usize) -> Result<u8> {
    if bytes.get(*pos) != Some(&0xff) {
        return Err(MediaError::invalid_data("expected JPEG marker prefix"));
    }
    while bytes.get(*pos) == Some(&0xff) {
        *pos += 1;
    }
    let marker = *bytes
        .get(*pos)
        .ok_or_else(|| MediaError::eof("truncated JPEG marker"))?;
    *pos += 1;
    if marker == 0x00 || marker == 0xff {
        return Err(MediaError::invalid_data("invalid JPEG marker code"));
    }
    Ok(marker)
}

fn segment<'a>(bytes: &'a [u8], pos: &mut usize) -> Result<&'a [u8]> {
    let length_bytes = bytes
        .get(
            *pos..pos
                .checked_add(2)
                .ok_or_else(|| MediaError::overflow("JPEG range overflow"))?,
        )
        .ok_or_else(|| MediaError::eof("truncated JPEG segment length"))?;
    let length = usize::from(u16::from_be_bytes([length_bytes[0], length_bytes[1]]));
    if length < 2 {
        return Err(MediaError::invalid_data(
            "JPEG segment length is smaller than 2",
        ));
    }
    let payload_start = *pos + 2;
    let payload_end = (*pos)
        .checked_add(length)
        .ok_or_else(|| MediaError::overflow("JPEG segment range overflow"))?;
    let payload = bytes
        .get(payload_start..payload_end)
        .ok_or_else(|| MediaError::eof("truncated JPEG segment payload"))?;
    *pos = payload_end;
    Ok(payload)
}

fn parse_frame(marker: u8, data: &[u8]) -> Result<JpegInfo> {
    if data.len() < 6 {
        return Err(MediaError::eof("truncated JPEG frame header"));
    }
    let precision = data[0];
    let height = u16::from_be_bytes([data[1], data[2]]);
    let width = u16::from_be_bytes([data[3], data[4]]);
    let components = data[5];
    if precision != 8 {
        return Err(MediaError::unsupported(format!(
            "JPEG sample precision {precision} is not implemented"
        )));
    }
    if width == 0 || height == 0 || components == 0 {
        return Err(MediaError::invalid_data(
            "JPEG frame dimensions/components must be non-zero",
        ));
    }
    let expected = 6usize
        .checked_add(usize::from(components) * 3)
        .ok_or_else(|| MediaError::overflow("JPEG component table size overflow"))?;
    if data.len() != expected {
        return Err(MediaError::invalid_data(
            "JPEG frame component table length mismatch",
        ));
    }
    for component in data[6..].chunks_exact(3) {
        let sampling = component[1];
        let h = sampling >> 4;
        let v = sampling & 0x0f;
        if h == 0 || v == 0 || h > 4 || v > 4 {
            return Err(MediaError::invalid_data("invalid JPEG sampling factor"));
        }
        if component[2] > 3 {
            return Err(MediaError::invalid_data(
                "JPEG quantization table id exceeds 3",
            ));
        }
    }
    Ok(JpegInfo {
        width,
        height,
        precision,
        components,
        coding: if marker == SOF0 {
            JpegCoding::Baseline
        } else {
            JpegCoding::Progressive
        },
    })
}

fn count_dqt(data: &[u8]) -> Result<u8> {
    let mut pos = 0usize;
    let mut count = 0u8;
    while pos < data.len() {
        let spec = data[pos];
        pos += 1;
        let precision = spec >> 4;
        let id = spec & 0x0f;
        if precision > 1 || id > 3 {
            return Err(MediaError::invalid_data("invalid JPEG DQT table specifier"));
        }
        let bytes = if precision == 0 { 64 } else { 128 };
        pos = pos
            .checked_add(bytes)
            .ok_or_else(|| MediaError::overflow("JPEG DQT range overflow"))?;
        if pos > data.len() {
            return Err(MediaError::eof("truncated JPEG quantization table"));
        }
        count = count
            .checked_add(1)
            .ok_or_else(|| MediaError::overflow("JPEG DQT count overflow"))?;
    }
    Ok(count)
}

fn count_dht(data: &[u8]) -> Result<u8> {
    let mut pos = 0usize;
    let mut count = 0u8;
    while pos < data.len() {
        let spec = *data
            .get(pos)
            .ok_or_else(|| MediaError::eof("truncated JPEG Huffman table"))?;
        pos += 1;
        if spec >> 4 > 1 || spec & 0x0f > 3 {
            return Err(MediaError::invalid_data(
                "invalid JPEG Huffman table specifier",
            ));
        }
        let counts = data
            .get(pos..pos + 16)
            .ok_or_else(|| MediaError::eof("truncated JPEG Huffman code counts"))?;
        pos += 16;
        let symbols = counts.iter().try_fold(0usize, |sum, &value| {
            sum.checked_add(usize::from(value))
                .ok_or_else(|| MediaError::overflow("JPEG Huffman symbol count overflow"))
        })?;
        if symbols == 0 || symbols > 256 {
            return Err(MediaError::invalid_data(
                "invalid JPEG Huffman symbol count",
            ));
        }
        pos = pos
            .checked_add(symbols)
            .ok_or_else(|| MediaError::overflow("JPEG Huffman table range overflow"))?;
        if pos > data.len() {
            return Err(MediaError::eof("truncated JPEG Huffman symbols"));
        }
        count = count
            .checked_add(1)
            .ok_or_else(|| MediaError::overflow("JPEG DHT count overflow"))?;
    }
    Ok(count)
}

fn validate_scan_header(data: &[u8]) -> Result<()> {
    let components = usize::from(
        *data
            .first()
            .ok_or_else(|| MediaError::eof("empty JPEG SOS"))?,
    );
    if components == 0 || components > 4 {
        return Err(MediaError::invalid_data("invalid JPEG SOS component count"));
    }
    let expected = 1usize
        .checked_add(components * 2)
        .and_then(|value| value.checked_add(3))
        .ok_or_else(|| MediaError::overflow("JPEG SOS size overflow"))?;
    if data.len() != expected {
        return Err(MediaError::invalid_data("JPEG SOS header length mismatch"));
    }
    Ok(())
}

fn skip_entropy_data(bytes: &[u8], pos: &mut usize) -> Result<()> {
    while *pos < bytes.len() {
        if bytes[*pos] != 0xff {
            *pos += 1;
            continue;
        }
        let mut marker_pos = *pos + 1;
        while bytes.get(marker_pos) == Some(&0xff) {
            marker_pos += 1;
        }
        let code = *bytes
            .get(marker_pos)
            .ok_or_else(|| MediaError::eof("truncated JPEG entropy marker"))?;
        if code == 0x00 {
            *pos = marker_pos + 1;
            continue;
        }
        if (0xd0..=0xd7).contains(&code) {
            *pos = marker_pos + 1;
            continue;
        }
        return Ok(());
    }
    Err(MediaError::eof("JPEG entropy data has no following marker"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment_bytes(marker: u8, payload: &[u8], out: &mut Vec<u8>) {
        out.extend_from_slice(&[0xff, marker]);
        out.extend_from_slice(&u16::try_from(payload.len() + 2).unwrap().to_be_bytes());
        out.extend_from_slice(payload);
    }

    fn minimal(coding: u8) -> Vec<u8> {
        let mut out = vec![0xff, SOI];
        let mut dqt = vec![0x00];
        dqt.extend_from_slice(&[1; 64]);
        segment_bytes(DQT, &dqt, &mut out);
        let mut dht = vec![0x00];
        dht.extend_from_slice(&[1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        dht.push(0);
        segment_bytes(DHT, &dht, &mut out);
        segment_bytes(coding, &[8, 0, 8, 0, 8, 1, 1, 0x11, 0], &mut out);
        segment_bytes(SOS, &[1, 1, 0, 0, 63, 0], &mut out);
        out.push(0x00);
        out.extend_from_slice(&[0xff, EOI]);
        out
    }

    #[test]
    fn parses_baseline_and_progressive_frame_metadata() {
        for (marker, coding) in [
            (SOF0, JpegCoding::Baseline),
            (SOF2, JpegCoding::Progressive),
        ] {
            let parsed = parse_jpeg(&minimal(marker)).unwrap();
            assert_eq!(parsed.info.width, 8);
            assert_eq!(parsed.info.height, 8);
            assert_eq!(parsed.info.components, 1);
            assert_eq!(parsed.info.coding, coding);
            assert_eq!(parsed.markers.quantization_tables, 1);
            assert_eq!(parsed.markers.huffman_tables, 1);
            assert_eq!(parsed.markers.scans, 1);
        }
    }

    #[test]
    fn rejects_truncated_and_invalid_lengths() {
        assert!(parse_jpeg(&[0xff, SOI, 0xff]).is_err());
        assert!(parse_jpeg(&[0xff, SOI, 0xff, DQT, 0, 1]).is_err());
        let mut jpeg = minimal(SOF0);
        jpeg.pop();
        assert!(parse_jpeg(&jpeg).is_err());
    }

    #[test]
    fn entropy_stuffing_and_restart_markers_do_not_end_scan() {
        let mut jpeg = minimal(SOF0);
        let eoi = jpeg.len() - 2;
        jpeg.splice(eoi - 1..eoi, [0x12, 0xff, 0x00, 0x34, 0xff, 0xd0, 0x56]);
        assert!(parse_jpeg(&jpeg).is_ok());
    }
}
