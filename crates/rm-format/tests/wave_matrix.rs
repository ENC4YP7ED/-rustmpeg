use rm_codec::{CodecId, pcm_bits_per_sample, pcm_bytes_per_sample};
use rm_format::{WaveAudioInfo, mux_wave, parse_wave, probe_wave};

const CODECS: [CodecId; 6] = [
    CodecId::PcmU8,
    CodecId::PcmS16Le,
    CodecId::PcmS24Le,
    CodecId::PcmS32Le,
    CodecId::PcmF32Le,
    CodecId::PcmF64Le,
];

fn wave_tag(codec: CodecId) -> u16 {
    match codec {
        CodecId::PcmF32Le | CodecId::PcmF64Le => 3,
        CodecId::PcmU8 | CodecId::PcmS16Le | CodecId::PcmS24Le | CodecId::PcmS32Le => 1,
        CodecId::Pbm
        | CodecId::Pgm
        | CodecId::Ppm
        | CodecId::Bmp
        | CodecId::Targa
        | CodecId::Png => {
            unreachable!("WAVE matrix received image codec")
        }
    }
}

fn audio_info(codec: CodecId, channels: u16, sample_rate: u32) -> WaveAudioInfo {
    let bits = pcm_bits_per_sample(codec).unwrap();
    let block_align = (pcm_bytes_per_sample(codec).unwrap() * usize::from(channels)) as u16;
    WaveAudioInfo {
        codec,
        channels,
        sample_rate,
        byte_rate: sample_rate * u32::from(block_align),
        block_align,
        bits_per_sample: bits,
        valid_bits_per_sample: bits,
        channel_mask: None,
    }
}

fn push_chunk(output: &mut Vec<u8>, id: &[u8; 4], payload: &[u8]) {
    output.extend_from_slice(id);
    output.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    output.extend_from_slice(payload);
    if payload.len() & 1 != 0 {
        output.push(0);
    }
}

fn manual_wave(codec: CodecId, channels: u16, sample_rate: u32, data: &[u8]) -> Vec<u8> {
    let bits = pcm_bits_per_sample(codec).unwrap();
    let block_align = (pcm_bytes_per_sample(codec).unwrap() * usize::from(channels)) as u16;
    let byte_rate = sample_rate * u32::from(block_align);
    let mut body = Vec::new();
    let mut fmt = Vec::new();
    fmt.extend_from_slice(&wave_tag(codec).to_le_bytes());
    fmt.extend_from_slice(&channels.to_le_bytes());
    fmt.extend_from_slice(&sample_rate.to_le_bytes());
    fmt.extend_from_slice(&byte_rate.to_le_bytes());
    fmt.extend_from_slice(&block_align.to_le_bytes());
    fmt.extend_from_slice(&bits.to_le_bytes());
    push_chunk(&mut body, b"fmt ", &fmt);
    push_chunk(&mut body, b"data", data);

    let mut output = Vec::new();
    output.extend_from_slice(b"RIFF");
    output.extend_from_slice(&((body.len() + 4) as u32).to_le_bytes());
    output.extend_from_slice(b"WAVE");
    output.extend_from_slice(&body);
    output
}

#[test]
fn every_supported_pcm_codec_muxes_and_parses() {
    for codec in CODECS {
        let audio = audio_info(codec, 2, 48_000);
        let data = vec![0x5A; usize::from(audio.block_align) * 3];
        let encoded = mux_wave(audio, &data).unwrap();
        let parsed = parse_wave(&encoded).unwrap();
        assert_eq!(parsed.info.audio, audio, "metadata mismatch for {codec:?}");
        assert_eq!(parsed.info.sample_count, 3);
        assert_eq!(parsed.data, data);
        assert_eq!(probe_wave(&encoded), 100);
    }
}

#[test]
fn parser_accepts_data_before_fmt_chunk() {
    let codec = CodecId::PcmS16Le;
    let audio = audio_info(codec, 1, 44_100);
    let data = [1_u8, 2, 3, 4];
    let mut body = Vec::new();
    push_chunk(&mut body, b"data", &data);

    let mut fmt = Vec::new();
    fmt.extend_from_slice(&1_u16.to_le_bytes());
    fmt.extend_from_slice(&audio.channels.to_le_bytes());
    fmt.extend_from_slice(&audio.sample_rate.to_le_bytes());
    fmt.extend_from_slice(&audio.byte_rate.to_le_bytes());
    fmt.extend_from_slice(&audio.block_align.to_le_bytes());
    fmt.extend_from_slice(&audio.bits_per_sample.to_le_bytes());
    push_chunk(&mut body, b"fmt ", &fmt);

    let mut file = Vec::new();
    file.extend_from_slice(b"RIFF");
    file.extend_from_slice(&((body.len() + 4) as u32).to_le_bytes());
    file.extend_from_slice(b"WAVE");
    file.extend_from_slice(&body);

    let parsed = parse_wave(&file).unwrap();
    assert_eq!(parsed.data, data);
    assert_eq!(parsed.info.audio, audio);
}

#[test]
fn odd_sized_unknown_chunk_padding_is_honored() {
    let codec = CodecId::PcmU8;
    let audio = audio_info(codec, 1, 8_000);
    let mut original = manual_wave(codec, 1, 8_000, &[0, 128, 255]);
    let fmt_and_data = original.split_off(12);

    let mut body = Vec::new();
    push_chunk(&mut body, b"JUNK", &[0xAB]);
    body.extend_from_slice(&fmt_and_data);

    let mut file = Vec::new();
    file.extend_from_slice(b"RIFF");
    file.extend_from_slice(&((body.len() + 4) as u32).to_le_bytes());
    file.extend_from_slice(b"WAVE");
    file.extend_from_slice(&body);

    let parsed = parse_wave(&file).unwrap();
    assert_eq!(parsed.info.audio, audio);
    assert_eq!(parsed.data, [0, 128, 255]);
}

#[test]
fn wave_format_extensible_pcm_is_recognized() {
    let channels = 2_u16;
    let sample_rate = 48_000_u32;
    let bits = 16_u16;
    let block_align = 4_u16;
    let byte_rate = sample_rate * u32::from(block_align);

    let mut fmt = Vec::new();
    fmt.extend_from_slice(&0xFFFE_u16.to_le_bytes());
    fmt.extend_from_slice(&channels.to_le_bytes());
    fmt.extend_from_slice(&sample_rate.to_le_bytes());
    fmt.extend_from_slice(&byte_rate.to_le_bytes());
    fmt.extend_from_slice(&block_align.to_le_bytes());
    fmt.extend_from_slice(&bits.to_le_bytes());
    fmt.extend_from_slice(&22_u16.to_le_bytes());
    fmt.extend_from_slice(&bits.to_le_bytes());
    fmt.extend_from_slice(&3_u32.to_le_bytes());
    fmt.extend_from_slice(&1_u32.to_le_bytes());
    fmt.extend_from_slice(&[
        0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71,
    ]);

    let mut body = Vec::new();
    push_chunk(&mut body, b"fmt ", &fmt);
    push_chunk(&mut body, b"data", &[0, 0, 0, 0]);

    let mut file = Vec::new();
    file.extend_from_slice(b"RIFF");
    file.extend_from_slice(&((body.len() + 4) as u32).to_le_bytes());
    file.extend_from_slice(b"WAVE");
    file.extend_from_slice(&body);

    let parsed = parse_wave(&file).unwrap();
    assert_eq!(parsed.info.audio.codec, CodecId::PcmS16Le);
    assert_eq!(parsed.info.audio.channel_mask, Some(3));
    assert_eq!(parsed.info.audio.valid_bits_per_sample, 16);
}

#[test]
fn trailing_bytes_after_declared_riff_are_ignored() {
    let mut file = manual_wave(CodecId::PcmS16Le, 1, 44_100, &[0, 0]);
    file.extend_from_slice(b"garbage outside RIFF");
    let parsed = parse_wave(&file).unwrap();
    assert_eq!(parsed.data, [0, 0]);
}

#[test]
fn malformed_headers_and_chunk_sizes_are_rejected() {
    let valid = manual_wave(CodecId::PcmS16Le, 1, 44_100, &[0, 0]);

    for len in 0..12 {
        assert!(
            parse_wave(&valid[..len]).is_err(),
            "accepted truncated header length {len}"
        );
    }

    let mut bad_signature = valid.clone();
    bad_signature[0..4].copy_from_slice(b"NOPE");
    assert!(parse_wave(&bad_signature).is_err());

    let mut bad_form = valid.clone();
    bad_form[8..12].copy_from_slice(b"AVI ");
    assert!(parse_wave(&bad_form).is_err());

    let mut oversized_riff = valid.clone();
    oversized_riff[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(parse_wave(&oversized_riff).is_err());

    let mut oversized_data = valid.clone();
    oversized_data[40..44].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(parse_wave(&oversized_data).is_err());
}

#[test]
fn missing_required_chunks_are_rejected() {
    let mut no_fmt_body = Vec::new();
    push_chunk(&mut no_fmt_body, b"data", &[0, 0]);
    let mut no_fmt = Vec::new();
    no_fmt.extend_from_slice(b"RIFF");
    no_fmt.extend_from_slice(&((no_fmt_body.len() + 4) as u32).to_le_bytes());
    no_fmt.extend_from_slice(b"WAVE");
    no_fmt.extend_from_slice(&no_fmt_body);
    assert!(parse_wave(&no_fmt).is_err());

    let mut no_data = manual_wave(CodecId::PcmS16Le, 1, 44_100, &[0, 0]);
    no_data.truncate(36);
    no_data[4..8].copy_from_slice(&28_u32.to_le_bytes());
    assert!(parse_wave(&no_data).is_err());
}

#[test]
fn invalid_pcm_layout_fields_are_rejected() {
    let valid = manual_wave(CodecId::PcmS16Le, 2, 48_000, &[0, 0, 0, 0]);

    let mut zero_channels = valid.clone();
    zero_channels[22..24].copy_from_slice(&0_u16.to_le_bytes());
    assert!(parse_wave(&zero_channels).is_err());

    let mut zero_rate = valid.clone();
    zero_rate[24..28].copy_from_slice(&0_u32.to_le_bytes());
    assert!(parse_wave(&zero_rate).is_err());

    let mut bad_byte_rate = valid.clone();
    bad_byte_rate[28..32].copy_from_slice(&1_u32.to_le_bytes());
    assert!(parse_wave(&bad_byte_rate).is_err());

    let mut bad_align = valid.clone();
    bad_align[32..34].copy_from_slice(&2_u16.to_le_bytes());
    assert!(parse_wave(&bad_align).is_err());

    let mut bad_width = valid.clone();
    bad_width[34..36].copy_from_slice(&20_u16.to_le_bytes());
    assert!(parse_wave(&bad_width).is_err());
}

#[test]
fn unsupported_wave_codec_is_rejected() {
    let mut file = manual_wave(CodecId::PcmS16Le, 1, 44_100, &[0, 0]);
    file[20..22].copy_from_slice(&6_u16.to_le_bytes());
    assert!(parse_wave(&file).is_err());
}

#[test]
fn probe_does_not_accept_partial_or_wrong_signatures() {
    assert_eq!(probe_wave(b""), 0);
    assert_eq!(probe_wave(b"RIFF\0\0\0\0WAV"), 0);
    assert_eq!(probe_wave(b"RIFF\0\0\0\0AVI "), 0);
}
