use bytes::Bytes;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodecType {
    H264,
    H265,
    AAC,
    Opus,
    PcmMulaw,
    PcmAlaw,
}

pub trait CodecData {
    fn codec_type(&self) -> CodecType;
}

#[derive(Debug, Clone)]
pub struct Packet {
    pub idx: i8,
    pub is_key_frame: bool,
    pub time: Duration,
    pub composition_time: Duration,
    pub duration: Duration,
    pub data: Bytes, // Bytes is cheap to clone (zero-copy references)
}
