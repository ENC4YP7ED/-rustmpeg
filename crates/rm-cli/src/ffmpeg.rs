use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

use rm_codec::{
    CodecId, convert_pcm, descriptor, find_by_name, pcm_bits_per_sample, pcm_bytes_per_sample,
};
use rm_core::{MediaError, Result};
use rm_format::{WaveAudioInfo, mux_wave, parse_wave, probe_wave};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodecSelection {
    Copy,
    Encode(CodecId),
}

#[derive(Debug, Default)]
struct Options {
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    codec: Option<CodecSelection>,
    overwrite: bool,
    never_overwrite: bool,
    hide_banner: bool,
}

pub fn run(args: &[OsString]) -> i32 {
    match run_inner(args) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("ffmpeg: {error}");
            1
        }
    }
}

fn run_inner(args: &[OsString]) -> Result<()> {
    if args.len() <= 1
        || args
            .iter()
            .skip(1)
            .any(|arg| is(arg, "-h") || is(arg, "-help") || is(arg, "--help"))
    {
        print_help();
        return Ok(());
    }

    let options = parse_options(args)?;
    let input = options
        .input
        .as_deref()
        .ok_or_else(|| MediaError::invalid_argument("missing input file"))?;
    let output = options
        .output
        .as_deref()
        .ok_or_else(|| MediaError::invalid_argument("missing output file"))?;

    if !is_wave_path(output) {
        return Err(MediaError::unsupported(
            "this revision only implements WAVE output (.wav/.wave)",
        ));
    }
    if input == output {
        return Err(MediaError::invalid_argument(
            "input and output paths must differ",
        ));
    }

    if output.exists() {
        if options.never_overwrite {
            return Err(MediaError::invalid_argument(format!(
                "output '{}' already exists",
                output.display()
            )));
        }
        if !options.overwrite {
            return Err(MediaError::invalid_argument(format!(
                "output '{}' already exists; use -y to overwrite",
                output.display()
            )));
        }
    }

    let input_bytes = fs::read(input)?;
    if probe_wave(&input_bytes) == 0 {
        return Err(MediaError::unsupported(
            "input format is not implemented yet (currently supported: wav)",
        ));
    }
    let wave = parse_wave(&input_bytes)?;
    let selection = options
        .codec
        .unwrap_or(CodecSelection::Encode(CodecId::PcmS16Le));

    let (output_audio, output_pcm, mapping) = match selection {
        CodecSelection::Copy => (
            wave.info.audio,
            wave.data.to_vec(),
            format!("{} (copy)", descriptor(wave.info.audio.codec).name),
        ),
        CodecSelection::Encode(target) => {
            let converted = convert_pcm(
                wave.info.audio.codec,
                target,
                wave.info.audio.channels,
                wave.data,
            )?;
            let output_audio = pcm_wave_audio(
                target,
                wave.info.audio.channels,
                wave.info.audio.sample_rate,
                wave.info.audio.channel_mask,
            )?;
            (
                output_audio,
                converted,
                format!(
                    "{} -> {}",
                    descriptor(wave.info.audio.codec).name,
                    descriptor(target).name
                ),
            )
        }
    };

    let output_bytes = mux_wave(output_audio, &output_pcm)?;

    if !options.hide_banner {
        eprintln!("{}", rm_core::build_banner("ffmpeg"));
    }
    let input_codec = descriptor(wave.info.audio.codec);
    let output_codec = descriptor(output_audio.codec);
    eprintln!("Input #0, wav, from '{}':", input.display());
    eprintln!(
        "  Stream #0:0: Audio: {}, {} Hz, {} channels",
        input_codec.name, wave.info.audio.sample_rate, wave.info.audio.channels
    );
    eprintln!("Stream mapping:");
    eprintln!("  Stream #0:0 -> #0:0 ({mapping})");

    fs::write(output, output_bytes)?;

    eprintln!("Output #0, wav, to '{}':", output.display());
    eprintln!(
        "  Stream #0:0: Audio: {}, {} Hz, {} channels",
        output_codec.name, output_audio.sample_rate, output_audio.channels
    );
    eprintln!(
        "size={} bytes time={:.6} bitrate={} kbits/s",
        fs::metadata(output)?.len(),
        wave.info.duration_seconds(),
        u64::from(output_audio.byte_rate) * 8 / 1_000
    );

    Ok(())
}

fn pcm_wave_audio(
    codec: CodecId,
    channels: u16,
    sample_rate: u32,
    channel_mask: Option<u32>,
) -> Result<WaveAudioInfo> {
    let bits_per_sample = pcm_bits_per_sample(codec);
    let block_align_usize = pcm_bytes_per_sample(codec)
        .checked_mul(usize::from(channels))
        .ok_or_else(|| MediaError::overflow("PCM block alignment overflow"))?;
    let block_align = u16::try_from(block_align_usize)
        .map_err(|_| MediaError::overflow("PCM block alignment exceeds u16"))?;
    let byte_rate = sample_rate
        .checked_mul(u32::from(block_align))
        .ok_or_else(|| MediaError::overflow("PCM byte rate overflow"))?;

    Ok(WaveAudioInfo {
        codec,
        channels,
        sample_rate,
        byte_rate,
        block_align,
        bits_per_sample,
        valid_bits_per_sample: bits_per_sample,
        channel_mask,
    })
}

fn parse_options(args: &[OsString]) -> Result<Options> {
    let mut options = Options::default();
    let mut index = 1;

    while index < args.len() {
        let arg = &args[index];
        if is(arg, "-i") {
            index += 1;
            let value = args
                .get(index)
                .ok_or_else(|| MediaError::invalid_argument("missing input after -i"))?;
            if options.input.is_some() {
                return Err(MediaError::unsupported(
                    "multiple inputs are not implemented yet",
                ));
            }
            options.input = Some(PathBuf::from(value));
        } else if is(arg, "-c")
            || is(arg, "-codec")
            || is(arg, "-c:a")
            || is(arg, "-codec:a")
            || is(arg, "-acodec")
        {
            index += 1;
            let value = args
                .get(index)
                .ok_or_else(|| MediaError::invalid_argument("missing codec value"))?;
            let value = value.to_string_lossy();
            options.codec = if value.eq_ignore_ascii_case("copy") {
                Some(CodecSelection::Copy)
            } else {
                Some(CodecSelection::Encode(find_by_name(&value).ok_or_else(
                    || MediaError::unsupported(format!("codec '{value}' is not implemented yet")),
                )?))
            };
        } else if is(arg, "-y") {
            options.overwrite = true;
        } else if is(arg, "-n") {
            options.never_overwrite = true;
        } else if is(arg, "-hide_banner") {
            options.hide_banner = true;
        } else if is(arg, "-v") || is(arg, "-loglevel") {
            index += 1;
            if index >= args.len() {
                return Err(MediaError::invalid_argument(
                    "missing value for loglevel option",
                ));
            }
        } else if arg.to_string_lossy().starts_with('-') {
            return Err(MediaError::unsupported(format!(
                "option '{}' is not implemented yet",
                arg.to_string_lossy()
            )));
        } else if options.output.is_none() {
            options.output = Some(PathBuf::from(arg));
        } else {
            return Err(MediaError::unsupported(
                "multiple outputs are not implemented yet",
            ));
        }
        index += 1;
    }

    if options.overwrite && options.never_overwrite {
        return Err(MediaError::invalid_argument(
            "-y and -n cannot be used together",
        ));
    }

    Ok(options)
}

fn is(value: &OsStr, expected: &str) -> bool {
    value == OsStr::new(expected)
}

fn is_wave_path(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("wav") || extension.eq_ignore_ascii_case("wave")
        })
}

fn print_help() {
    println!("{}", rm_core::build_banner("ffmpeg"));
    println!("usage: ffmpeg -i INPUT [OPTIONS] OUTPUT.wav");
    println!("  -i FILE           input file");
    println!("  -c copy           stream-copy the implemented codec");
    println!(
        "  -c:a CODEC        encode PCM as pcm_u8/pcm_s16le/pcm_s24le/pcm_s32le/pcm_f32le/pcm_f64le"
    );
    println!("  -y                 overwrite output without asking");
    println!("  -n                 never overwrite output");
    println!("  -hide_banner       suppress banner");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_stream_copy_command() {
        let args = [
            OsString::from("ffmpeg"),
            OsString::from("-i"),
            OsString::from("in.wav"),
            OsString::from("-c"),
            OsString::from("copy"),
            OsString::from("out.wav"),
        ];
        let options = parse_options(&args).unwrap();
        assert_eq!(options.input.as_deref(), Some(Path::new("in.wav")));
        assert_eq!(options.output.as_deref(), Some(Path::new("out.wav")));
        assert_eq!(options.codec, Some(CodecSelection::Copy));
    }

    #[test]
    fn resolves_in_tree_pcm_encoder_by_ffmpeg_name() {
        let args = [
            OsString::from("ffmpeg"),
            OsString::from("-i"),
            OsString::from("in.wav"),
            OsString::from("-c:a"),
            OsString::from("pcm_f32le"),
            OsString::from("out.wav"),
        ];
        let options = parse_options(&args).unwrap();
        assert_eq!(
            options.codec,
            Some(CodecSelection::Encode(CodecId::PcmF32Le))
        );
    }

    #[test]
    fn rejects_conflicting_overwrite_modes() {
        let args = [
            OsString::from("ffmpeg"),
            OsString::from("-y"),
            OsString::from("-n"),
            OsString::from("out.wav"),
        ];
        assert!(parse_options(&args).is_err());
    }
}
