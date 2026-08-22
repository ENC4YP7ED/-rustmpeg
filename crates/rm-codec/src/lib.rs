#![forbid(unsafe_code)]

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaType {
    Video,
    Audio,
    Subtitle,
    Data,
    Attachment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodecDescriptor {
    pub name: &'static str,
    pub media_type: MediaType,
    pub can_decode: bool,
    pub can_encode: bool,
}

#[must_use]
pub const fn codecs() -> &'static [CodecDescriptor] {
    &[]
}
