//! Decoder (B6): hardware dulu, software fallback.
//! Windows: Media Foundation/D3D11; macOS: VideoToolbox; Linux: VAAPI.
//! Backend dipilih lewat trait; tidak ada cfg(target_os) di luar vcam/vmic (A9).

use phonecam_core::error::Result;

/// Prioritas decode (B6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DecodePath {
    /// Hardware: Media Foundation / VideoToolbox / VAAPI.
    #[default]
    Hardware,
    /// Software fallback.
    Software,
}

/// Trait decoder video per backend.
pub trait VideoDecoder: Send + Sync {
    /// Nama backend untuk log/diagnostik.
    fn name(&self) -> &'static str;
    /// Decode satu frame; mengembalikan byte frame dan apakah keyframe.
    fn decode(&mut self, packet: &[u8]) -> Result<DecodedFrame>;
}

#[derive(Debug, Clone)]
pub struct DecodedFrame {
    pub pts: u64,
    pub data: bytes::Bytes,
    pub keyframe: bool,
}
