#![forbid(unsafe_code)]

mod ffprobe;

use std::ffi::OsString;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Program {
    Ffmpeg,
    Ffprobe,
    Ffplay,
}

impl Program {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Ffmpeg => "ffmpeg",
            Self::Ffprobe => "ffprobe",
            Self::Ffplay => "ffplay",
        }
    }
}

pub fn run(program: Program, args: impl IntoIterator<Item = OsString>) -> i32 {
    let args: Vec<OsString> = args.into_iter().collect();
    let wants_version = args
        .iter()
        .skip(1)
        .any(|arg| arg == "-version" || arg == "--version");

    if wants_version {
        println!("{}", rm_core::build_banner(program.name()));
        return 0;
    }

    match program {
        Program::Ffprobe => ffprobe::run(&args),
        Program::Ffmpeg | Program::Ffplay => run_bootstrap(program, &args),
    }
}

fn run_bootstrap(program: Program, args: &[OsString]) -> i32 {
    let wants_help = args.len() <= 1
        || args
            .iter()
            .skip(1)
            .any(|arg| arg == "-h" || arg == "-help" || arg == "--help");

    if wants_help {
        println!("{}", rm_core::build_banner(program.name()));
        println!("clean-room bootstrap: media commands are not implemented yet");
        return 0;
    }

    eprintln!(
        "{}: requested command is not implemented in this bootstrap revision",
        program.name()
    );
    1
}
