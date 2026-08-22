use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

use rm_codec::{CodecId, descriptor};
use rm_core::{MediaError, Result};
use rm_format::{WaveFile, formats, parse_wave, probe_wave};

#[derive(Debug, Default)]
struct Options {
    input: Option<PathBuf>,
    show_streams: bool,
    show_format: bool,
    hide_banner: bool,
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
    if args.len() <= 1 || args.iter().skip(1).any(|arg| is(arg, "-h") || is(arg, "-help") || is(arg, "--help")) {
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
    let bytes = fs::read(input)?;

    if probe_wave(&bytes) == 0 {
        return Err(MediaError::unsupported(
            "input format is not implemented yet (currently supported: wav)",
        ));
    }

    let wave = parse_wave(&bytes)?;
    if !options.hide_banner && !options.show_streams && !options.show_format {
        eprintln!("{}", rm_core::build_banner("ffprobe"));
    }

    if options.show_streams {
        print_stream(&wave);
    }
    if options.show_format {
        print_format(input, bytes.len(), &wave);
    }
    if !options.show_streams && !options.show_format {
        print_human_summary(input, bytes.len(), &wave);
    }

    Ok(())
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
        } else if is(arg, "-v") || is(arg, "-loglevel") {
            index += 1;
            if index >= args.len() {
                return Err(MediaError::invalid_argument("missing value for loglevel option"));
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

fn is(value: &OsStr, expected: &str) -> bool {
    value == OsStr::new(expected)
}

fn print_help() {
    println!("{}", rm_core::build_banner("ffprobe"));
    println!("usage: ffprobe [OPTIONS] INPUT");
    println!("  -show_streams     show stream information");
    println!("  -show_format      show container information");
    println!("  -formats          list implemented formats");
    println!("  -codecs           list implemented codecs");
    println!("  -hide_banner      suppress banner");
    println!("  -of default       default key=value writer");
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
        println!(" {decode}{encode}.... {:<20} {}", codec.name, codec.long_name);
    }
}

fn print_stream(wave: &WaveFile<'_>) {
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

fn print_format(path: &Path, file_size: usize, wave: &WaveFile<'_>) {
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

fn print_human_summary(path: &Path, _file_size: usize, wave: &WaveFile<'_>) {
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
}
