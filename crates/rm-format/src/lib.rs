#![forbid(unsafe_code)]

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatDescriptor {
    pub name: &'static str,
    pub can_demux: bool,
    pub can_mux: bool,
}

#[must_use]
pub const fn formats() -> &'static [FormatDescriptor] {
    &[]
}
