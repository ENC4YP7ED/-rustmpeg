#![forbid(unsafe_code)]

pub mod image2;

use rm_codec::{CodecId, pcm_bits_per_sample, pcm_from_wave_tag, pcm_wave_format_tag};
use rm_core::{MediaError, Result};
use rm_io::{ByteReader, ByteWriter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatDescriptor {
    pub name: &'static str,
    pub long_name: &'static str,
    pub extensions: &'static [&'static str],
    pub can_demux: bool,
    pub can_mux: bool,
}

const FORMATS: [FormatDescriptor; 2] = [
    FormatDescriptor {
        name: "wav",
        long_name: "WAV / WAVE (Waveform Audio)",
        extensions: &["wav", "wave"],
        can_demux: true,
        can_mux: true,
    },
    FormatDescriptor {
        name: "image2",
        long_name: "image2 sequence",
        extensions: &["pbm", "pgm", "ppm", "pnm"],
        can_demux: true,
        can_mux: true,
    },
];

#[must_use]
pub const fn formats() -> &'static [FormatDescriptor] {
    &FORMATS
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaveAudioInfo {
    pub codec: CodecId,
    pub channels: u16,
    pub sample_rate: u32,
    pub byte_rate: u32,
    pub block_align: u16,
    pub bits_per_sample: u16,
    pub valid_bits_per_sample: u16,
    pub channel_mask: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaveInfo {
    pub riff_size: u32,
    pub audio: WaveAudioInfo,
    pub data_offset: usize,
    pub data_size: usize,
    pub sample_count: u64,
}

impl WaveInfo {
    #[must_use]
    pub fn duration_seconds(self) -> f64 {
        self.sample_count as f64 / f64::from(self.audio.sample_rate)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct WaveFile<'a> {
    pub info: WaveInfo,
    pub data: &'a [u8],
}

#[must_use]
pub fn probe_wave(bytes: &[u8]) -> u8 {
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE" {
        100
    } else {
        0
    }
}

pub fn parse_wave(bytes: &[u8]) -> Result<WaveFile<'_>> {
    if bytes.len() < 12 {
        return Err(MediaError::invalid_data(
            "WAVE input is shorter than a RIFF header",
        ));
    }

    let mut header = ByteReader::new(bytes);
    if header.read_array::<4>()? != *b"RIFF" {
        return Err(MediaError::invalid_data("missing RIFF signature"));
    }
    let riff_size = header.read_u32_le()?;
    if header.read_array::<4>()? != *b"WAVE" {
        return Err(MediaError::invalid_data("RIFF form type is not WAVE"));
    }
    if riff_size < 4 {
        return Err(MediaError::invalid_data("invalid RIFF size"));
    }

    let declared_end = usize::try_from(riff_size)
        .map_err(|_| MediaError::overflow("RIFF size does not fit usize"))?
        .checked_add(8)
        .ok_or_else(|| MediaError::overflow("RIFF size overflow"))?;
    if declared_end > bytes.len() {
        return Err(MediaError::invalid_data(
            "RIFF size exceeds available input",
        ));
    }

    let mut reader = ByteReader::new(&bytes[..declared_end]);
    reader.skip(12)?;

    let mut audio = None;
    let mut data_range = None;

    while reader.remaining() >= 8 {
        let chunk_id = reader.read_array::<4>()?;
        let chunk_size = usize::try_from(reader.read_u32_le()?)
            .map_err(|_| MediaError::overflow("RIFF chunk size does not fit usize"))?;
        let chunk_offset = reader.position();
        let chunk_data = reader.take(chunk_size)?;

        match &chunk_id {
            b"fmt " => {
                if audio.is_none() {
                    audio = Some(parse_wave_fmt(chunk_data)?);
                }
            }
            b"data" => {
                if data_range.is_none() {
                    data_range = Some((chunk_offset, chunk_size));
                }
            }
            _ => {}
        }

        if chunk_size & 1 != 0 && reader.remaining() != 0 {
            reader.skip(1)?;
        }
    }

    let audio = audio.ok_or_else(|| MediaError::invalid_data("WAVE file has no fmt chunk"))?;
    let (data_offset, data_size) =
        data_range.ok_or_else(|| MediaError::invalid_data("WAVE file has no data chunk"))?;

    validate_wave_audio(audio)?;

    if data_size % usize::from(audio.block_align) != 0 {
        return Err(MediaError::invalid_data(
            "WAVE data size is not aligned to complete audio sample frames",
        ));
    }

    let sample_count = u64::try_from(data_size / usize::from(audio.block_align))
        .map_err(|_| MediaError::overflow("WAVE sample count exceeds u64"))?;
    let data_end = data_offset
        .checked_add(data_size)
        .ok_or_else(|| MediaError::overflow("WAVE data range overflow"))?;
    let data = bytes
        .get(data_offset..data_end)
        .ok_or_else(|| MediaError::invalid_data("WAVE data range exceeds input"))?;

    Ok(WaveFile {
        info: WaveInfo {
            riff_size,
            audio,
            data_offset,
            data_size,
            sample_count,
        },
        data,
    })
}

fn parse_wave_fmt(bytes: &[u8]) -> Result<WaveAudioInfo> {
    if bytes.len() < 16 {
        return Err(MediaError::invalid_data(
            "WAVE fmt chunk is shorter than 16 bytes",
        ));
    }

    let mut reader = ByteReader::new(bytes);
    let original_format_tag = reader.read_u16_le()?;
    let channels = reader.read_u16_le()?;
    let sample_rate = reader.read_u32_le()?;
    let byte_rate = reader.read_u32_le()?;
    let block_align = reader.read_u16_le()?;
    let bits_per_sample = reader.read_u16_le()?;

    let mut format_tag = original_format_tag;
    let mut valid_bits_per_sample = bits_per_sample;
    let mut channel_mask = None;

    if original_format_tag == 0xFFFE {
        if bytes.len() < 40 {
            return Err(MediaError::invalid_data(
                "WAVE_FORMAT_EXTENSIBLE fmt chunk is shorter than 40 bytes",
            ));
        }
        let extension_size = reader.read_u16_le()?;
        if extension_size < 22 {
            return Err(MediaError::invalid_data(
                "WAVE_FORMAT_EXTENSIBLE extension is shorter than 22 bytes",
            ));
        }
        valid_bits_per_sample = reader.read_u16_le()?;
        channel_mask = Some(reader.read_u32_le()?);
        let subformat = reader.read_array::<16>()?;
        const KS_SUBTYPE_TAIL: [u8; 12] = [
            0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71,
        ];
        if subformat[4..] != KS_SUBTYPE_TAIL {
            return Err(MediaError::unsupported(
                "unsupported WAVE_FORMAT_EXTENSIBLE subformat GUID",
            ));
        }
        let data1 = u32::from_le_bytes([subformat[0], subformat[1], subformat[2], subformat[3]]);
        format_tag = u16::try_from(data1)
            .map_err(|_| MediaError::unsupported("unsupported WAVE extensible format tag"))?;
    }

    let codec = pcm_from_wave_tag(format_tag, bits_per_sample).ok_or_else(|| {
        MediaError::unsupported(format!(
            "unsupported WAVE codec tag 0x{format_tag:04x} with {bits_per_sample} bits per sample"
        ))
    })?;

    Ok(WaveAudioInfo {
        codec,
        channels,
        sample_rate,
        byte_rate,
        block_align,
        bits_per_sample,
        valid_bits_per_sample,
        channel_mask,
    })
}

fn validate_wave_audio(audio: WaveAudioInfo) -> Result<()> {
    if audio.channels == 0 {
        return Err(MediaError::invalid_data(
            "WAVE channel count must be non-zero",
        ));
    }
    if audio.sample_rate == 0 {
        return Err(MediaError::invalid_data(
            "WAVE sample rate must be non-zero",
        ));
    }
    if audio.block_align == 0 {
        return Err(MediaError::invalid_data(
            "WAVE block alignment must be non-zero",
        ));
    }
    if audio.bits_per_sample == 0 || audio.bits_per_sample % 8 != 0 {
        return Err(MediaError::invalid_data(
            "supported WAVE PCM sample widths must be byte-aligned",
        ));
    }
    if audio.valid_bits_per_sample == 0 || audio.valid_bits_per_sample > audio.bits_per_sample {
        return Err(MediaError::invalid_data(
            "invalid WAVE valid-bits-per-sample value",
        ));
    }
    if pcm_bits_per_sample(audio.codec) != Some(audio.bits_per_sample) {
        return Err(MediaError::invalid_data(
            "WAVE codec and bits-per-sample disagree",
        ));
    }

    let expected_block_align = u32::from(audio.channels)
        .checked_mul(u32::from(audio.bits_per_sample / 8))
        .ok_or_else(|| MediaError::overflow("WAVE block alignment overflow"))?;
    if expected_block_align != u32::from(audio.block_align) {
        return Err(MediaError::invalid_data("invalid WAVE block alignment"));
    }

    let expected_byte_rate = audio
        .sample_rate
        .checked_mul(u32::from(audio.block_align))
        .ok_or_else(|| MediaError::overflow("WAVE byte rate overflow"))?;
    if expected_byte_rate != audio.byte_rate {
        return Err(MediaError::invalid_data("invalid WAVE byte rate"));
    }

    Ok(())
}

pub fn mux_wave(audio: WaveAudioInfo, data: &[u8]) -> Result<Vec<u8>> {
    validate_wave_audio(audio)?;

    if data.len() % usize::from(audio.block_align) != 0 {
        return Err(MediaError::invalid_argument(
            "PCM payload does not contain complete sample frames",
        ));
    }

    let format_tag = pcm_wave_format_tag(audio.codec)
        .ok_or_else(|| MediaError::invalid_argument("WAVE muxer requires a PCM codec"))?;
    let data_size = u32::try_from(data.len()).map_err(|_| {
        MediaError::unsupported(
            "classic RIFF/WAVE output is limited to 4 GiB; RF64 is not implemented yet",
        )
    })?;
    let padded_data_size = data_size
        .checked_add(data_size & 1)
        .ok_or_else(|| MediaError::overflow("WAVE padded data size overflow"))?;
    let riff_size = 4_u32
        .checked_add(8 + 16)
        .and_then(|value| value.checked_add(8))
        .and_then(|value| value.checked_add(padded_data_size))
        .ok_or_else(|| MediaError::unsupported("classic RIFF/WAVE size exceeds 4 GiB"))?;

    let mut writer = ByteWriter::with_capacity(
        usize::try_from(riff_size)
            .ok()
            .and_then(|size| size.checked_add(8))
            .unwrap_or(data.len()),
    );
    writer.write(b"RIFF");
    writer.write_u32_le(riff_size);
    writer.write(b"WAVE");
    writer.write(b"fmt ");
    writer.write_u32_le(16);
    writer.write_u16_le(format_tag);
    writer.write_u16_le(audio.channels);
    writer.write_u32_le(audio.sample_rate);
    writer.write_u32_le(audio.byte_rate);
    writer.write_u16_le(audio.block_align);
    writer.write_u16_le(audio.bits_per_sample);
    writer.write(b"data");
    writer.write_u32_le(data_size);
    writer.write(data);
    if data_size & 1 != 0 {
        writer.write_u8(0);
    }

    Ok(writer.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stereo_s16() -> WaveAudioInfo {
        WaveAudioInfo {
            codec: CodecId::PcmS16Le,
            channels: 2,
            sample_rate: 48_000,
            byte_rate: 192_000,
            block_align: 4,
            bits_per_sample: 16,
            valid_bits_per_sample: 16,
            channel_mask: None,
        }
    }

    #[test]
    fn wave_mux_round_trips_through_parser() {
        let pcm = [1_u8, 0, 2, 0, 3, 0, 4, 0];
        let encoded = mux_wave(stereo_s16(), &pcm).unwrap();
        let parsed = parse_wave(&encoded).unwrap();
        assert_eq!(parsed.info.audio, stereo_s16());
        assert_eq!(parsed.info.sample_count, 2);
        assert_eq!(parsed.data, pcm);
        assert_eq!(probe_wave(&encoded), 100);
    }

    #[test]
    fn malformed_chunk_length_is_rejected() {
        let mut encoded = mux_wave(stereo_s16(), &[0, 0, 0, 0]).unwrap();
        encoded[40..44].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse_wave(&encoded).is_err());
    }

    #[test]
    fn misaligned_pcm_payload_is_rejected() {
        assert!(mux_wave(stereo_s16(), &[0, 0]).is_err());
    }

    #[test]
    fn image_codec_cannot_be_muxed_as_wave() {
        let mut audio = stereo_s16();
        audio.codec = CodecId::Ppm;
        assert!(mux_wave(audio, &[0, 0, 0, 0]).is_err());
    }
}
