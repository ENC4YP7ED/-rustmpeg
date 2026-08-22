use rm_codec::{CodecId, convert_pcm, pcm_bytes_per_sample};

const CODECS: [CodecId; 6] = [
    CodecId::PcmU8,
    CodecId::PcmS16Le,
    CodecId::PcmS24Le,
    CodecId::PcmS32Le,
    CodecId::PcmF32Le,
    CodecId::PcmF64Le,
];

fn f64_bytes(samples: &[f64]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(samples.len() * 8);
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

fn decode_f64(bytes: &[u8]) -> Vec<f64> {
    bytes
        .chunks_exact(8)
        .map(|sample| {
            f64::from_le_bytes([
                sample[0], sample[1], sample[2], sample[3], sample[4], sample[5], sample[6],
                sample[7],
            ])
        })
        .collect()
}

fn tolerance(codec: CodecId) -> f64 {
    match codec {
        CodecId::PcmU8 => 1.0 / 128.0 + 1.0e-12,
        CodecId::PcmS16Le => 1.0 / 32_768.0 + 1.0e-12,
        CodecId::PcmS24Le => 1.0 / 8_388_608.0 + 1.0e-12,
        CodecId::PcmS32Le => 1.0 / 2_147_483_648.0 + 1.0e-12,
        CodecId::PcmF32Le => 1.0e-6,
        CodecId::PcmF64Le => 1.0e-12,
        CodecId::Pbm
        | CodecId::Pgm
        | CodecId::Ppm
        | CodecId::Bmp
        | CodecId::Targa
        | CodecId::Png => {
            unreachable!("PCM test received image codec")
        }
    }
}

#[test]
fn every_pcm_pair_converts_with_expected_output_length() {
    for input in CODECS {
        let input_width = pcm_bytes_per_sample(input).unwrap();
        let source = vec![0_u8; input_width * 8];
        for output in CODECS {
            let converted = convert_pcm(input, output, 2, &source).unwrap();
            assert_eq!(
                converted.len(),
                pcm_bytes_per_sample(output).unwrap() * 8,
                "length mismatch for {input:?} -> {output:?}"
            );
        }
    }
}

#[test]
fn canonical_amplitudes_round_trip_through_every_pcm_format() {
    let samples = [
        -1.0,
        -0.75,
        -0.5,
        -1.0 / 32_768.0,
        0.0,
        1.0 / 32_768.0,
        0.5,
        0.75,
        1.0,
    ];
    let source = f64_bytes(&samples);

    for codec in CODECS {
        let encoded = convert_pcm(CodecId::PcmF64Le, codec, 1, &source).unwrap();
        let decoded = convert_pcm(codec, CodecId::PcmF64Le, 1, &encoded).unwrap();
        let decoded = decode_f64(&decoded);
        let tolerance = tolerance(codec);

        for (index, (expected, actual)) in samples.iter().zip(decoded.iter()).enumerate() {
            assert!(
                (expected - actual).abs() <= tolerance,
                "sample {index} failed through {codec:?}: expected {expected}, got {actual}, tolerance {tolerance}"
            );
        }
    }
}

#[test]
fn interleaved_stereo_sample_order_is_preserved() {
    let source = f64_bytes(&[-1.0, 1.0, -0.5, 0.5]);
    let converted = convert_pcm(CodecId::PcmF64Le, CodecId::PcmS16Le, 2, &source).unwrap();
    let samples: Vec<i16> = converted
        .chunks_exact(2)
        .map(|sample| i16::from_le_bytes([sample[0], sample[1]]))
        .collect();
    assert_eq!(samples, [i16::MIN, i16::MAX, -16_384, 16_384]);
}

#[test]
fn malformed_interleaved_frames_are_rejected_for_every_codec() {
    for codec in CODECS {
        let width = pcm_bytes_per_sample(codec).unwrap();
        let malformed = vec![0_u8; width * 2 - 1];
        assert!(
            convert_pcm(codec, CodecId::PcmS16Le, 2, &malformed).is_err(),
            "misaligned {codec:?} input was accepted"
        );
    }
}

#[test]
fn zero_channels_are_rejected() {
    assert!(convert_pcm(CodecId::PcmS16Le, CodecId::PcmF32Le, 0, &[0, 0]).is_err());
}

#[test]
fn non_finite_float_samples_are_sanitized_to_silence() {
    let source = f64_bytes(&[f64::NAN, f64::INFINITY, f64::NEG_INFINITY]);
    let converted = convert_pcm(CodecId::PcmF64Le, CodecId::PcmS16Le, 1, &source).unwrap();
    let samples: Vec<i16> = converted
        .chunks_exact(2)
        .map(|sample| i16::from_le_bytes([sample[0], sample[1]]))
        .collect();
    assert_eq!(samples, [0, 0, 0]);
}

#[test]
fn integer_endpoints_clip_deterministically() {
    let source = f64_bytes(&[-2.0, -1.0, 0.0, 1.0, 2.0]);
    let converted = convert_pcm(CodecId::PcmF64Le, CodecId::PcmS16Le, 1, &source).unwrap();
    let samples: Vec<i16> = converted
        .chunks_exact(2)
        .map(|sample| i16::from_le_bytes([sample[0], sample[1]]))
        .collect();
    assert_eq!(samples, [i16::MIN, i16::MIN, 0, i16::MAX, i16::MAX]);
}
