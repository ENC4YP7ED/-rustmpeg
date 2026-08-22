use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

use rm_codec::{
    CodecId, MediaType, convert_pcm, descriptor, find_by_name, pcm_bits_per_sample,
    pcm_bytes_per_sample,
};
use rm_core::{MediaError, Rational, Result};
use rm_format::image2::ImagePattern;
use rm_format::{WaveAudioInfo, mux_wave, parse_wave, probe_wave};

use crate::image::{codec_for_path, decode_image, encode_image, is_image_codec, prepare_frame, probe_image};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodecSelection {
    Copy,
    Encode(CodecId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ForcedFormat {
    Wav,
    Image2,
}

#[derive(Debug, Clone)]
struct Options {
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    codec: Option<CodecSelection>,
    overwrite: bool,
    never_overwrite: bool,
    hide_banner: bool,
    input_format: Option<ForcedFormat>,
    output_format: Option<ForcedFormat>,
    input_start_number: i64,
    output_start_number: i64,
    start_number_range: usize,
    frame_limit: Option<usize>,
    size: Option<(u32, u32)>,
    input_framerate: Rational,
    output_framerate: Option<Rational>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            input: None,
            output: None,
            codec: None,
            overwrite: false,
            never_overwrite: false,
            hide_banner: false,
            input_format: None,
            output_format: None,
            input_start_number: 0,
            output_start_number: 1,
            start_number_range: 5,
            frame_limit: None,
            size: None,
            input_framerate: Rational::new(25, 1).expect("25/1 is a valid rational"),
            output_framerate: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MediaPath {
    Wave,
    Image2,
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

    if input == output {
        return Err(MediaError::invalid_argument(
            "input and output paths must differ",
        ));
    }

    let input_kind = detect_input(&options, input)?;
    let output_kind = detect_output(&options, output)?;
    match (input_kind, output_kind) {
        (MediaPath::Wave, MediaPath::Wave) => run_wave(&options, input, output),
        (MediaPath::Image2, MediaPath::Image2) => run_image2(&options, input, output),
        (MediaPath::Wave, MediaPath::Image2) => Err(MediaError::unsupported(
            "audio-to-image transcoding is not implemented",
        )),
        (MediaPath::Image2, MediaPath::Wave) => Err(MediaError::unsupported(
            "image-to-audio transcoding is not implemented",
        )),
    }
}

fn detect_input(options: &Options, input: &Path) -> Result<MediaPath> {
    if let Some(format) = options.input_format {
        return Ok(match format {
            ForcedFormat::Wav => MediaPath::Wave,
            ForcedFormat::Image2 => MediaPath::Image2,
        });
    }

    let pattern = ImagePattern::parse(input)?;
    if pattern.is_sequence() {
        return Ok(MediaPath::Image2);
    }

    let bytes = fs::read(input)?;
    let wave_score = probe_wave(&bytes);
    let image_score = probe_image(&bytes);
    if wave_score == 0 && image_score == 0 {
        return Err(MediaError::unsupported(
            "input format is not implemented yet (supported: wav, image2 PBM/PGM/PPM)",
        ));
    }
    if wave_score >= image_score {
        Ok(MediaPath::Wave)
    } else {
        Ok(MediaPath::Image2)
    }
}

fn detect_output(options: &Options, output: &Path) -> Result<MediaPath> {
    if let Some(format) = options.output_format {
        return Ok(match format {
            ForcedFormat::Wav => MediaPath::Wave,
            ForcedFormat::Image2 => MediaPath::Image2,
        });
    }
    if is_wave_path(output) {
        return Ok(MediaPath::Wave);
    }
    if codec_for_path(output).is_some() || ImagePattern::parse(output)?.is_sequence() {
        return Ok(MediaPath::Image2);
    }
    if matches!(options.codec, Some(CodecSelection::Encode(codec)) if is_image_codec(codec)) {
        return Ok(MediaPath::Image2);
    }
    Err(MediaError::unsupported(format!(
        "cannot infer output format from '{}'",
        output.display()
    )))
}

fn run_wave(options: &Options, input: &Path, output: &Path) -> Result<()> {
    check_output(output, options)?;
    let input_bytes = fs::read(input)?;
    if probe_wave(&input_bytes) == 0 {
        return Err(MediaError::invalid_data("forced WAVE input is not a WAVE file"));
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
            if descriptor(target).media_type != MediaType::Audio {
                return Err(MediaError::invalid_argument(format!(
                    "codec '{}' is not an audio codec",
                    descriptor(target).name
                )));
            }
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
    eprintln!("Input #0, wav, from '{}':", input.display());
    eprintln!(
        "  Stream #0:0: Audio: {}, {} Hz, {} channels",
        descriptor(wave.info.audio.codec).name,
        wave.info.audio.sample_rate,
        wave.info.audio.channels
    );
    eprintln!("Stream mapping:");
    eprintln!("  Stream #0:0 -> #0:0 ({mapping})");

    fs::write(output, output_bytes)?;
    eprintln!("Output #0, wav, to '{}':", output.display());
    eprintln!(
        "  Stream #0:0: Audio: {}, {} Hz, {} channels",
        descriptor(output_audio.codec).name,
        output_audio.sample_rate,
        output_audio.channels
    );
    eprintln!(
        "size={} bytes time={:.6} bitrate={} kbits/s",
        fs::metadata(output)?.len(),
        wave.info.duration_seconds(),
        u64::from(output_audio.byte_rate) * 8 / 1_000
    );
    Ok(())
}

fn run_image2(options: &Options, input: &Path, output: &Path) -> Result<()> {
    let input_pattern = ImagePattern::parse(input)?;
    let input_paths = input_pattern.collect_existing_in_range(
        options.input_start_number,
        options.start_number_range,
        options.frame_limit,
    )?;
    let output_pattern = ImagePattern::parse(output)?;

    if input_paths.len() > 1 && !output_pattern.is_sequence() {
        return Err(MediaError::invalid_argument(
            "multiple input images require a numbered output pattern such as frame-%03d.ppm",
        ));
    }

    let mut output_paths = Vec::with_capacity(input_paths.len());
    for index in 0..input_paths.len() {
        let path = if output_pattern.is_sequence() {
            let index = i64::try_from(index)
                .map_err(|_| MediaError::overflow("output frame index exceeds i64"))?;
            let number = options
                .output_start_number
                .checked_add(index)
                .ok_or_else(|| MediaError::overflow("output frame number overflow"))?;
            output_pattern.path_for(number)?
        } else {
            output.to_path_buf()
        };
        if input_paths.iter().any(|input_path| input_path == &path) {
            return Err(MediaError::invalid_argument(format!(
                "output '{}' would overwrite an input frame",
                path.display()
            )));
        }
        check_output(&path, options)?;
        output_paths.push(path);
    }

    if !options.hide_banner {
        eprintln!("{}", rm_core::build_banner("ffmpeg"));
    }

    let mut total_bytes = 0_u64;
    let mut first_input_codec = None;
    let mut first_output_codec = None;
    let mut first_size = None;

    for (index, (input_path, output_path)) in input_paths.iter().zip(&output_paths).enumerate() {
        let input_bytes = fs::read(input_path)?;
        if probe_image(&input_bytes) == 0 {
            return Err(MediaError::invalid_data(format!(
                "image2 frame '{}' is not PBM/PGM/PPM",
                input_path.display()
            )));
        }
        let decoded = decode_image(&input_bytes)?;
        first_input_codec.get_or_insert(decoded.codec);
        first_size.get_or_insert((decoded.frame.width, decoded.frame.height));

        let (target_codec, encoded) = match options.codec {
            Some(CodecSelection::Copy) => {
                if options.size.is_some() {
                    return Err(MediaError::invalid_argument(
                        "-s cannot be combined with stream copy",
                    ));
                }
                let extension_codec = codec_for_path(output_path);
                if extension_codec.is_some_and(|codec| codec != decoded.codec) {
                    return Err(MediaError::invalid_argument(format!(
                        "cannot stream-copy {} into a {} filename",
                        descriptor(decoded.codec).name,
                        descriptor(extension_codec.expect("checked Some above")).name
                    )));
                }
                (decoded.codec, input_bytes)
            }
            Some(CodecSelection::Encode(codec)) => {
                if !is_image_codec(codec) {
                    return Err(MediaError::invalid_argument(format!(
                        "codec '{}' is not an image codec",
                        descriptor(codec).name
                    )));
                }
                let prepared = prepare_frame(&decoded.frame, codec, options.size)?;
                (codec, encode_image(codec, &prepared)?)
            }
            None => {
                let codec = codec_for_path(output_path).unwrap_or(decoded.codec);
                let prepared = prepare_frame(&decoded.frame, codec, options.size)?;
                (codec, encode_image(codec, &prepared)?)
            }
        };

        first_output_codec.get_or_insert(target_codec);
        fs::write(output_path, &encoded)?;
        total_bytes = total_bytes
            .checked_add(u64::try_from(encoded.len()).map_err(|_| {
                MediaError::overflow("encoded image size exceeds u64")
            })?)
            .ok_or_else(|| MediaError::overflow("total output size overflow"))?;

        if index == 0 {
            let mapping = if matches!(options.codec, Some(CodecSelection::Copy)) {
                format!("{} (copy)", descriptor(decoded.codec).name)
            } else {
                format!(
                    "{} -> {}",
                    descriptor(decoded.codec).name,
                    descriptor(target_codec).name
                )
            };
            eprintln!("Input #0, image2, from '{}':", input.display());
            eprintln!(
                "  Stream #0:0: Video: {}, {}, {}x{}, {:.3} fps",
                descriptor(decoded.codec).name,
                decoded.frame.format.name(),
                decoded.frame.width,
                decoded.frame.height,
                options.input_framerate.as_f64()
            );
            eprintln!("Stream mapping:");
            eprintln!("  Stream #0:0 -> #0:0 ({mapping})");
        }
    }

    let output_rate = options.output_framerate.unwrap_or(options.input_framerate);
    let (width, height) = options.size.or(first_size).ok_or_else(|| {
        MediaError::invalid_data("image2 sequence produced no frame dimensions")
    })?;
    eprintln!("Output #0, image2, to '{}':", output.display());
    eprintln!(
        "  Stream #0:0: Video: {}, {}x{}, {:.3} fps",
        descriptor(first_output_codec.ok_or_else(|| {
            MediaError::invalid_data("image2 sequence produced no output codec")
        })?)
        .name,
        width,
        height,
        output_rate.as_f64()
    );
    let duration = f64::from(
        u32::try_from(input_paths.len())
            .map_err(|_| MediaError::overflow("image2 frame count exceeds u32"))?,
    ) / output_rate.as_f64();
    eprintln!(
        "frame={} size={} bytes time={duration:.6}",
        input_paths.len(),
        total_bytes
    );
    Ok(())
}

fn check_output(output: &Path, options: &Options) -> Result<()> {
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
    Ok(())
}

fn pcm_wave_audio(
    codec: CodecId,
    channels: u16,
    sample_rate: u32,
    channel_mask: Option<u32>,
) -> Result<WaveAudioInfo> {
    let bits_per_sample = pcm_bits_per_sample(codec)
        .ok_or_else(|| MediaError::invalid_argument("WAVE output requires a PCM codec"))?;
    let bytes_per_sample = pcm_bytes_per_sample(codec)
        .ok_or_else(|| MediaError::invalid_argument("WAVE output requires a PCM codec"))?;
    let block_align_usize = bytes_per_sample
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
        } else if is(arg, "-f") {
            index += 1;
            let value = args
                .get(index)
                .ok_or_else(|| MediaError::invalid_argument("missing format after -f"))?;
            let format = parse_format(value)?;
            if options.input.is_none() {
                options.input_format = Some(format);
            } else {
                options.output_format = Some(format);
            }
        } else if is(arg, "-start_number") {
            index += 1;
            let value = parse_i64(
                args.get(index)
                    .ok_or_else(|| MediaError::invalid_argument("missing start_number value"))?,
                "start_number",
            )?;
            if options.input.is_none() {
                options.input_start_number = value;
            } else {
                options.output_start_number = value;
            }
        } else if is(arg, "-start_number_range") {
            if options.input.is_some() {
                return Err(MediaError::invalid_argument(
                    "-start_number_range is an image2 input option and must appear before -i",
                ));
            }
            index += 1;
            options.start_number_range = parse_usize(
                args.get(index).ok_or_else(|| {
                    MediaError::invalid_argument("missing start_number_range value")
                })?,
                "start_number_range",
            )?;
            if options.start_number_range == 0 {
                return Err(MediaError::invalid_argument(
                    "start_number_range must be positive",
                ));
            }
        } else if is(arg, "-framerate") {
            if options.input.is_some() {
                return Err(MediaError::invalid_argument(
                    "-framerate is an image2 input option and must appear before -i",
                ));
            }
            index += 1;
            options.input_framerate = parse_rate(args.get(index).ok_or_else(|| {
                MediaError::invalid_argument("missing framerate value")
            })?)?;
        } else if is(arg, "-r") {
            index += 1;
            options.output_framerate = Some(parse_rate(args.get(index).ok_or_else(|| {
                MediaError::invalid_argument("missing output frame rate after -r")
            })?)?);
        } else if is(arg, "-frames:v") || is(arg, "-vframes") {
            index += 1;
            let value = parse_usize(
                args.get(index)
                    .ok_or_else(|| MediaError::invalid_argument("missing video frame count"))?,
                "video frame count",
            )?;
            if value == 0 {
                return Err(MediaError::invalid_argument(
                    "video frame count must be positive",
                ));
            }
            options.frame_limit = Some(value);
        } else if is(arg, "-s") || is(arg, "-s:v") {
            index += 1;
            options.size = Some(parse_size(args.get(index).ok_or_else(|| {
                MediaError::invalid_argument("missing video size")
            })?)?);
        } else if is(arg, "-c")
            || is(arg, "-codec")
            || is(arg, "-c:a")
            || is(arg, "-codec:a")
            || is(arg, "-acodec")
            || is(arg, "-c:v")
            || is(arg, "-codec:v")
            || is(arg, "-vcodec")
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

fn parse_format(value: &OsStr) -> Result<ForcedFormat> {
    let value = value.to_string_lossy();
    if value.eq_ignore_ascii_case("wav") || value.eq_ignore_ascii_case("wave") {
        Ok(ForcedFormat::Wav)
    } else if value.eq_ignore_ascii_case("image2") {
        Ok(ForcedFormat::Image2)
    } else {
        Err(MediaError::unsupported(format!(
            "format '{value}' is not implemented yet"
        )))
    }
}

fn parse_i64(value: &OsStr, name: &str) -> Result<i64> {
    value
        .to_string_lossy()
        .parse::<i64>()
        .map_err(|_| MediaError::invalid_argument(format!("invalid {name}")))
}

fn parse_usize(value: &OsStr, name: &str) -> Result<usize> {
    value
        .to_string_lossy()
        .parse::<usize>()
        .map_err(|_| MediaError::invalid_argument(format!("invalid {name}")))
}

fn parse_size(value: &OsStr) -> Result<(u32, u32)> {
    let value = value.to_string_lossy();
    let (width, height) = value
        .split_once('x')
        .or_else(|| value.split_once('X'))
        .ok_or_else(|| MediaError::invalid_argument("video size must be WIDTHxHEIGHT"))?;
    let width = width
        .parse::<u32>()
        .map_err(|_| MediaError::invalid_argument("invalid video width"))?;
    let height = height
        .parse::<u32>()
        .map_err(|_| MediaError::invalid_argument("invalid video height"))?;
    if width == 0 || height == 0 {
        return Err(MediaError::invalid_argument(
            "video dimensions must be non-zero",
        ));
    }
    Ok((width, height))
}

fn parse_rate(value: &OsStr) -> Result<Rational> {
    let text = value.to_string_lossy();
    let (numerator, denominator) = if let Some((num, den)) = text.split_once('/') {
        let numerator = num
            .parse::<i64>()
            .map_err(|_| MediaError::invalid_argument("invalid frame-rate numerator"))?;
        let denominator = den
            .parse::<i64>()
            .map_err(|_| MediaError::invalid_argument("invalid frame-rate denominator"))?;
        (numerator, denominator)
    } else if let Some((whole, fraction)) = text.split_once('.') {
        if fraction.is_empty() || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(MediaError::invalid_argument("invalid decimal frame rate"));
        }
        let whole = whole
            .parse::<i64>()
            .map_err(|_| MediaError::invalid_argument("invalid decimal frame rate"))?;
        if whole < 0 {
            return Err(MediaError::invalid_argument("frame rate must be positive"));
        }
        let denominator = 10_i64
            .checked_pow(u32::try_from(fraction.len()).map_err(|_| {
                MediaError::overflow("frame-rate decimal precision exceeds u32")
            })?)
            .ok_or_else(|| MediaError::overflow("frame-rate denominator overflow"))?;
        let fraction = fraction
            .parse::<i64>()
            .map_err(|_| MediaError::invalid_argument("invalid decimal frame rate"))?;
        let numerator = whole
            .checked_mul(denominator)
            .and_then(|value| value.checked_add(fraction))
            .ok_or_else(|| MediaError::overflow("frame-rate numerator overflow"))?;
        (numerator, denominator)
    } else {
        (
            text.parse::<i64>()
                .map_err(|_| MediaError::invalid_argument("invalid frame rate"))?,
            1,
        )
    };
    if numerator <= 0 || denominator <= 0 {
        return Err(MediaError::invalid_argument("frame rate must be positive"));
    }
    Rational::new(numerator, denominator)
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
    println!("usage: ffmpeg [INPUT OPTIONS] -i INPUT [OUTPUT OPTIONS] OUTPUT");
    println!("  -f wav|image2      force implemented input/output format");
    println!("  -i FILE            input file or image2 pattern");
    println!("  -c copy            stream-copy supported bitstreams");
    println!("  -c:a CODEC         PCM: pcm_u8/s16le/s24le/s32le/f32le/f64le");
    println!("  -c:v CODEC         image: pbm/pgm/ppm");
    println!("  -framerate RATE    image2 input rate (default 25)");
    println!("  -r RATE            output frame rate metadata");
    println!("  -start_number N    image2 input before -i, output after -i");
    println!("  -start_number_range N  image2 first-frame search range (default 5)");
    println!("  -frames:v N        limit image frames");
    println!("  -s[:v] WxH         nearest-neighbor resize for image output");
    println!("  -y                  overwrite output without asking");
    println!("  -n                  never overwrite output");
    println!("  -hide_banner        suppress banner");
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
    fn scopes_image2_start_numbers_around_input() {
        let args = [
            OsString::from("ffmpeg"),
            OsString::from("-start_number"),
            OsString::from("3"),
            OsString::from("-i"),
            OsString::from("in-%03d.ppm"),
            OsString::from("-start_number"),
            OsString::from("10"),
            OsString::from("out-%03d.pgm"),
        ];
        let options = parse_options(&args).unwrap();
        assert_eq!(options.input_start_number, 3);
        assert_eq!(options.output_start_number, 10);
    }

    #[test]
    fn parses_image_codec_resize_and_frame_limit() {
        let args = [
            OsString::from("ffmpeg"),
            OsString::from("-i"),
            OsString::from("in.ppm"),
            OsString::from("-c:v"),
            OsString::from("pgm"),
            OsString::from("-s:v"),
            OsString::from("320x200"),
            OsString::from("-frames:v"),
            OsString::from("1"),
            OsString::from("out.pgm"),
        ];
        let options = parse_options(&args).unwrap();
        assert_eq!(options.codec, Some(CodecSelection::Encode(CodecId::Pgm)));
        assert_eq!(options.size, Some((320, 200)));
        assert_eq!(options.frame_limit, Some(1));
    }

    #[test]
    fn parses_fractional_and_decimal_frame_rates() {
        assert_eq!(parse_rate(OsStr::new("30000/1001")).unwrap(), Rational::new(30000, 1001).unwrap());
        assert_eq!(parse_rate(OsStr::new("29.97")).unwrap(), Rational::new(2997, 100).unwrap());
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
