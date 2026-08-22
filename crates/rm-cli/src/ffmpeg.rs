use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

use rm_codec::descriptor;
use rm_core::{MediaError, Result};
use rm_format::{mux_wave, parse_wave, probe_wave};

#[derive(Debug, Default)]
struct Options {
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    copy_codec: bool,
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
    if args.len() <= 1 || args.iter().skip(1).any(|arg| is(arg, "-h") || is(arg, "-help") || is(arg, "--help")) {
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

    if !options.copy_codec {
        return Err(MediaError::unsupported(
            "this revision implements WAVE stream-copy only; use -c copy",
        ));
    }
    if !is_wave_path(output) {
        return Err(MediaError::unsupported(
            "this revision only implements WAVE output (.wav/.wave)",
        ));
    }
    if input == output {
        return Err(MediaError::invalid_argument("input and output paths must differ"));
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
    let output_bytes = mux_wave(wave.info.audio, wave.data)?;

    if !options.hide_banner {
        eprintln!("{}", rm_core::build_banner("ffmpeg"));
    }
    let codec = descriptor(wave.info.audio.codec);
    eprintln!("Input #0, wav, from '{}':", input.display());
    eprintln!(
        "  Stream #0:0: Audio: {}, {} Hz, {} channels",
        codec.name, wave.info.audio.sample_rate, wave.info.audio.channels
    );
    eprintln!("Stream mapping:");
    eprintln!("  Stream #0:0 -> #0:0 (copy)");

    fs::write(output, output_bytes)?;

    eprintln!("Output #0, wav, to '{}':", output.display());
    eprintln!(
        "  Stream #0:0: Audio: {}, {} Hz, {} channels",
        codec.name, wave.info.audio.sample_rate, wave.info.audio.channels
    );
    eprintln!(
        "size={} bytes time={:.6} bitrate={} kbits/s",
        fs::metadata(output)?.len(),
        wave.info.duration_seconds(),
        u64::from(wave.info.audio.byte_rate) * 8 / 1_000
    );

    Ok(())
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
        } else if is(arg, "-c") || is(arg, "-codec") || is(arg, "-c:a") || is(arg, "-codec:a") {
            index += 1;
            let value = args
                .get(index)
                .ok_or_else(|| MediaError::invalid_argument("missing codec value"))?;
            if !is(value, "copy") {
                return Err(MediaError::unsupported(format!(
                    "codec '{}' is not implemented by the ffmpeg CLI yet",
                    value.to_string_lossy()
                )));
            }
            options.copy_codec = true;
        } else if is(arg, "-y") {
            options.overwrite = true;
        } else if is(arg, "-n") {
            options.never_overwrite = true;
        } else if is(arg, "-hide_banner") {
            options.hide_banner = true;
        } else if is(arg, "-v") || is(arg, "-loglevel") {
            index += 1;
            if index >= args.len() {
                return Err(MediaError::invalid_argument("missing value for loglevel option"));
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
        return Err(MediaError::invalid_argument("-y and -n cannot be used together"));
    }

    Ok(options)
}

fn is(value: &OsStr, expected: &str) -> bool {
    value == OsStr::new(expected)
}

fn is_wave_path(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case("wav") || extension.eq_ignore_ascii_case("wave"))
}

fn print_help() {
    println!("{}", rm_core::build_banner("ffmpeg"));
    println!("usage: ffmpeg -i INPUT -c copy OUTPUT.wav");
    println!("  -i FILE           input file");
    println!("  -c copy           stream-copy the implemented codec");
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
        assert!(options.copy_codec);
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
