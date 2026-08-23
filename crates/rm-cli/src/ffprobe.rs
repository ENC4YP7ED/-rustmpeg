use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

use rm_codec::{CodecId, descriptor};
use rm_core::{MediaError, Rational, Result};
use rm_format::image2::ImagePattern;
use rm_format::{WaveFile, formats, parse_wave, probe_wave};

use crate::image::{decode_image, probe_image};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ForcedFormat {
    Wav,
    Image2,
}

#[derive(Debug, Clone)]
struct Options {
    input: Option<PathBuf>,
    show_streams: bool,
    show_format: bool,
    hide_banner: bool,
    input_format: Option<ForcedFormat>,
    start_number: i64,
    start_number_range: usize,
    framerate: Rational,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            input: None,
            show_streams: false,
            show_format: false,
            hide_banner: false,
            input_format: None,
            start_number: 0,
            start_number_range: 5,
            framerate: Rational::new(25, 1).expect("25/1 is a valid rational"),
        }
    }
}

pub fn run(args: &[OsString]) -> i32 {
    match run_inner(args) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("ffprobe: {error}");
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

    if args.iter().skip(1).any(|arg| is(arg, "-formats")) {
        print_formats();
        return Ok(());
    }
    if args.iter().skip(1).any(|arg| is(arg, "-codecs")) {
        print_codecs();
        return Ok(());
    }

    let options = parse_options(args)?;
    let input = options
        .input
        .as_deref()
        .ok_or_else(|| MediaError::invalid_argument("missing input file"))?;

    let pattern = ImagePattern::parse(input)?;
    let forced_image = options.input_format == Some(ForcedFormat::Image2);
    let forced_wave = options.input_format == Some(ForcedFormat::Wav);

    if forced_image || pattern.is_sequence() {
        return probe_image2_input(&options, input, &pattern);
    }

    let bytes = fs::read(input)?;
    if forced_wave || probe_wave(&bytes) >= probe_image(&bytes) && probe_wave(&bytes) != 0 {
        let wave = parse_wave(&bytes)?;
        if !options.hide_banner && !options.show_streams && !options.show_format {
            eprintln!("{}", rm_core::build_banner("ffprobe"));
        }
        if options.show_streams {
            print_wave_stream(&wave);
        }
        if options.show_format {
            print_wave_format(input, bytes.len(), &wave);
        }
        if !options.show_streams && !options.show_format {
            print_wave_human_summary(input, &wave);
        }
        return Ok(());
    }

    if probe_image(&bytes) != 0 {
        return probe_image2_input(&options, input, &pattern);
    }

    Err(MediaError::unsupported(
        "input format is not implemented yet (supported: wav, image2 PBM/PGM/PPM/BMP/TGA/PNG)",
    ))
}

fn probe_image2_input(options: &Options, input: &Path, pattern: &ImagePattern) -> Result<()> {
    let paths = pattern.collect_existing_in_range(
        options.start_number,
        options.start_number_range,
        None,
    )?;
    let first_bytes = fs::read(&paths[0])?;
    if probe_image(&first_bytes) == 0 {
        return Err(MediaError::invalid_data(format!(
            "image2 frame '{}' is not a supported image",
            paths[0].display()
        )));
    }
    let image = decode_image(&first_bytes)?;
    let total_size = sequence_size(&paths)?;
    let frame_count = u64::try_from(paths.len())
        .map_err(|_| MediaError::overflow("image2 frame count exceeds u64"))?;
    let duration = frame_count as f64 / options.framerate.as_f64();

    if !options.hide_banner && !options.show_streams && !options.show_format {
        eprintln!("{}", rm_core::build_banner("ffprobe"));
    }
    if options.show_streams {
        print_image_stream(&image, frame_count, duration, options.framerate);
    }
    if options.show_format {
        print_image_format(input, total_size, duration);
    }
    if !options.show_streams && !options.show_format {
        eprintln!("Input #0, image2, from '{}':", input.display());
        eprintln!(
            "  Duration: {}, start: 0.000000, bitrate: {} kb/s",
            human_duration(duration),
            bitrate_kbps(total_size, duration)
        );
        eprintln!(
            "  Stream #0:0: Video: {}, {}, {}x{}, {:.3} fps",
            descriptor(image.codec).name,
            image.frame.format.name(),
            image.frame.width,
            image.frame.height,
            options.framerate.as_f64()
        );
    }
    Ok(())
}

fn sequence_size(paths: &[PathBuf]) -> Result<u64> {
    let mut total = 0_u64;
    for path in paths {
        total = total
            .checked_add(fs::metadata(path)?.len())
            .ok_or_else(|| MediaError::overflow("image2 total byte size overflow"))?;
    }
    Ok(total)
}

fn parse_options(args: &[OsString]) -> Result<Options> {
    let mut options = Options::default();
    let mut index = 1;

    while index < args.len() {
        let arg = &args[index];
        if is(arg, "-show_streams") {
            options.show_streams = true;
        } else if is(arg, "-show_format") {
            options.show_format = true;
        } else if is(arg, "-hide_banner") {
            options.hide_banner = true;
        } else if is(arg, "-f") {
            index += 1;
            let value = args
                .get(index)
                .ok_or_else(|| MediaError::invalid_argument("missing format after -f"))?;
            options.input_format = Some(parse_format(value)?);
        } else if is(arg, "-start_number") {
            index += 1;
            options.start_number = args
                .get(index)
                .ok_or_else(|| MediaError::invalid_argument("missing start_number value"))?
                .to_string_lossy()
                .parse::<i64>()
                .map_err(|_| MediaError::invalid_argument("invalid start_number"))?;
        } else if is(arg, "-start_number_range") {
            index += 1;
            options.start_number_range = args
                .get(index)
                .ok_or_else(|| MediaError::invalid_argument("missing start_number_range value"))?
                .to_string_lossy()
                .parse::<usize>()
                .map_err(|_| MediaError::invalid_argument("invalid start_number_range"))?;
            if options.start_number_range == 0 {
                return Err(MediaError::invalid_argument(
                    "start_number_range must be positive",
                ));
            }
        } else if is(arg, "-framerate") {
            index += 1;
            options.framerate = parse_rate(
                args.get(index)
                    .ok_or_else(|| MediaError::invalid_argument("missing framerate value"))?,
            )?;
        } else if is(arg, "-v") || is(arg, "-loglevel") {
            index += 1;
            if index >= args.len() {
                return Err(MediaError::invalid_argument(
                    "missing value for loglevel option",
                ));
            }
        } else if is(arg, "-of") || is(arg, "-print_format") {
            index += 1;
            let value = args
                .get(index)
                .ok_or_else(|| MediaError::invalid_argument("missing output format value"))?;
            if !is(value, "default") {
                return Err(MediaError::unsupported(
                    "ffprobe output writers other than 'default' are not implemented yet",
                ));
            }
        } else if is(arg, "-i") {
            index += 1;
            let value = args
                .get(index)
                .ok_or_else(|| MediaError::invalid_argument("missing input after -i"))?;
            set_input(&mut options, value)?;
        } else if arg.to_string_lossy().starts_with('-') {
            return Err(MediaError::unsupported(format!(
                "option '{}' is not implemented yet",
                arg.to_string_lossy()
            )));
        } else {
            set_input(&mut options, arg)?;
        }
        index += 1;
    }
    Ok(options)
}

fn set_input(options: &mut Options, value: &OsStr) -> Result<()> {
    if options.input.is_some() {
        return Err(MediaError::invalid_argument(
            "multiple ffprobe inputs are not supported",
        ));
    }
    options.input = Some(PathBuf::from(value));
    Ok(())
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

fn parse_rate(value: &OsStr) -> Result<Rational> {
    let text = value.to_string_lossy();
    let (numerator, denominator) = if let Some((num, den)) = text.split_once('/') {
        (
            num.parse::<i64>()
                .map_err(|_| MediaError::invalid_argument("invalid frame-rate numerator"))?,
            den.parse::<i64>()
                .map_err(|_| MediaError::invalid_argument("invalid frame-rate denominator"))?,
        )
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

fn print_help() {
    println!("{}", rm_core::build_banner("ffprobe"));
    println!("usage: ffprobe [OPTIONS] INPUT");
    println!("  -f wav|image2      force implemented input format");
    println!("  -show_streams      show stream information");
    println!("  -show_format       show container information");
    println!("  -formats           list implemented formats");
    println!("  -codecs            list implemented codecs");
    println!("  -framerate RATE    image2 input frame rate");
    println!("  -start_number N    image2 first index");
    println!("  -start_number_range N  image2 first-frame search range");
    println!("  -hide_banner       suppress banner");
    println!("  -of default        default key=value writer");
}

fn print_formats() {
    println!("File formats:");
    println!(" D. = Demuxing supported");
    println!(" .E = Muxing supported");
    for format in formats() {
        let demux = if format.can_demux { 'D' } else { ' ' };
        let mux = if format.can_mux { 'E' } else { ' ' };
        println!(" {demux}{mux} {:<15} {}", format.name, format.long_name);
    }
}

fn print_codecs() {
    println!("Codecs:");
    println!(" D..... = Decoding supported");
    println!(" .E.... = Encoding supported");
    for codec in rm_codec::codecs() {
        let decode = if codec.can_decode { 'D' } else { '.' };
        let encode = if codec.can_encode { 'E' } else { '.' };
        println!(
            " {decode}{encode}.... {:<20} {}",
            codec.name, codec.long_name
        );
    }
}

fn print_wave_stream(wave: &WaveFile<'_>) {
    let audio = wave.info.audio;
    let codec = descriptor(audio.codec);
    let bit_rate = u64::from(audio.byte_rate) * 8;

    println!("[STREAM]");
    println!("index=0");
    println!("codec_name={}", codec.name);
    println!("codec_long_name={}", codec.long_name);
    println!("codec_type=audio");
    println!("sample_fmt={}", sample_format_name(audio.codec));
    println!("sample_rate={}", audio.sample_rate);
    println!("channels={}", audio.channels);
    println!("bits_per_sample={}", audio.bits_per_sample);
    println!("duration_ts={}", wave.info.sample_count);
    println!("duration={:.6}", wave.info.duration_seconds());
    println!("bit_rate={bit_rate}");
    println!("[/STREAM]");
}

fn print_image_stream(
    image: &crate::image::DecodedImage,
    frame_count: u64,
    duration: f64,
    framerate: Rational,
) {
    let codec = descriptor(image.codec);
    println!("[STREAM]");
    println!("index=0");
    println!("codec_name={}", codec.name);
    println!("codec_long_name={}", codec.long_name);
    println!("codec_type=video");
    println!("width={}", image.frame.width);
    println!("height={}", image.frame.height);
    println!("coded_width={}", image.frame.width);
    println!("coded_height={}", image.frame.height);
    println!("pix_fmt={}", image.frame.format.name());
    println!(
        "r_frame_rate={}/{}",
        framerate.numerator(),
        framerate.denominator()
    );
    println!(
        "avg_frame_rate={}/{}",
        framerate.numerator(),
        framerate.denominator()
    );
    println!(
        "time_base={}/{}",
        framerate.denominator(),
        framerate.numerator()
    );
    println!("duration_ts={frame_count}");
    println!("duration={duration:.6}");
    println!("nb_frames={frame_count}");
    println!("[/STREAM]");
}

fn print_wave_format(path: &Path, file_size: usize, wave: &WaveFile<'_>) {
    let duration = wave.info.duration_seconds();
    let bit_rate = if duration > 0.0 {
        ((file_size as f64 * 8.0) / duration).round() as u64
    } else {
        0
    };

    println!("[FORMAT]");
    println!("filename={}", path.display());
    println!("nb_streams=1");
    println!("nb_programs=0");
    println!("format_name=wav");
    println!("format_long_name=WAV / WAVE (Waveform Audio)");
    println!("start_time=0.000000");
    println!("duration={duration:.6}");
    println!("size={file_size}");
    println!("bit_rate={bit_rate}");
    println!("[/FORMAT]");
}

fn print_image_format(path: &Path, file_size: u64, duration: f64) {
    println!("[FORMAT]");
    println!("filename={}", path.display());
    println!("nb_streams=1");
    println!("nb_programs=0");
    println!("format_name=image2");
    println!("format_long_name=image2 sequence");
    println!("start_time=0.000000");
    println!("duration={duration:.6}");
    println!("size={file_size}");
    println!("bit_rate={}", bitrate_kbps(file_size, duration) * 1_000);
    println!("[/FORMAT]");
}

fn print_wave_human_summary(path: &Path, wave: &WaveFile<'_>) {
    let audio = wave.info.audio;
    let codec = descriptor(audio.codec);
    let duration = human_duration(wave.info.duration_seconds());
    let bit_rate_kbps = u64::from(audio.byte_rate) * 8 / 1_000;

    eprintln!("Input #0, wav, from '{}':", path.display());
    eprintln!("  Duration: {duration}, start: 0.000000, bitrate: {bit_rate_kbps} kb/s");
    eprintln!(
        "  Stream #0:0: Audio: {}, {} Hz, {} channels, {}, {} kb/s",
        codec.name,
        audio.sample_rate,
        audio.channels,
        sample_format_name(audio.codec),
        bit_rate_kbps
    );
}

fn sample_format_name(codec: CodecId) -> &'static str {
    match codec {
        CodecId::PcmU8 => "u8",
        CodecId::PcmS16Le => "s16",
        CodecId::PcmS24Le | CodecId::PcmS32Le => "s32",
        CodecId::PcmF32Le => "flt",
        CodecId::PcmF64Le => "dbl",
        CodecId::Pbm
        | CodecId::Pgm
        | CodecId::Ppm
        | CodecId::Bmp
        | CodecId::Targa
        | CodecId::Png
        | CodecId::Jpeg => "unknown",
    }
}

fn bitrate_kbps(file_size: u64, duration: f64) -> u64 {
    if duration > 0.0 {
        ((file_size as f64 * 8.0) / duration / 1_000.0).round() as u64
    } else {
        0
    }
}

fn human_duration(seconds: f64) -> String {
    let total_centiseconds = (seconds * 100.0).round() as u64;
    let hours = total_centiseconds / 360_000;
    let minutes = (total_centiseconds / 6_000) % 60;
    let secs = (total_centiseconds / 100) % 60;
    let centiseconds = total_centiseconds % 100;
    format!("{hours:02}:{minutes:02}:{secs:02}.{centiseconds:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_format_matches_ffmpeg_style_shape() {
        assert_eq!(human_duration(65.12), "00:01:05.12");
    }

    #[test]
    fn image2_options_parse() {
        let args = [
            OsString::from("ffprobe"),
            OsString::from("-f"),
            OsString::from("image2"),
            OsString::from("-start_number"),
            OsString::from("3"),
            OsString::from("-framerate"),
            OsString::from("30"),
            OsString::from("frame-%03d.ppm"),
        ];
        let options = parse_options(&args).unwrap();
        assert_eq!(options.input_format, Some(ForcedFormat::Image2));
        assert_eq!(options.start_number, 3);
        assert_eq!(options.framerate, Rational::new(30, 1).unwrap());
    }
}
