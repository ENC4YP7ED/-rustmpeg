#![forbid(unsafe_code)]

pub const RUSTMPEG_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const FFMPEG_COMPAT_VERSION: &str = "9.0.1";

#[must_use]
pub fn build_banner(program: &str) -> String {
    format!(
        "{program} version rustmpeg-{RUSTMPEG_VERSION} (FFmpeg {FFMPEG_COMPAT_VERSION} compatibility target)"
    )
}
