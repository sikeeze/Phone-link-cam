//! PhoneCam vmic: abstraksi virtual mic (README Tech stack, B6).
//!
//! Aturan A9: kode platform hanya di crate ini (dan crates/vcam).
//!
//! - Windows: driver audio virtual (bundel atau arahkan install)
//! - macOS: Audio Server plugin (mirip BlackHole)
//! - Linux: virtual sink/source PipeWire atau PulseAudio
#![allow(dead_code)] // kerangka: backend per OS diisi oleh fase 2/4
#![forbid(unsafe_code)]

use phonecam_core::error::{Error, Result};

/// Format audio (B6: Opus 48 kHz, 20 ms).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VMicFormat {
    pub sample_rate: u32,
    pub channels: u16,
    /// Durasi frame audio dalam ms (B6: 20 ms).
    pub frame_ms: u32,
}

impl Default for VMicFormat {
    fn default() -> Self {
        Self {
            sample_rate: 48_000,
            channels: 1,
            frame_ms: 20,
        }
    }
}

/// Status pemasangan virtual mic (B8: VMIC_NOT_INSTALLED).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VMicStatus {
    #[default]
    Installed,
    NotInstalled,
}

/// Trait backend virtual mic per OS (A9).
#[async_trait::async_trait]
pub trait VMic: Send + Sync {
    fn name(&self) -> &'static str;
    async fn status(&self) -> Result<VMicStatus>;
    async fn install(&self) -> Result<()>;
    async fn start(&self, format: VMicFormat) -> Result<()>;
    /// Kirim PCM ke virtual mic. Hot path (aturan A7): tidak boleh memblokir.
    async fn push_pcm(&self, pcm: &[u8]) -> Result<()>;
    async fn stop(&self) -> Result<()>;
}

/// Backend tidak tersedia di OS ini (stub untuk kompilasi silang).
pub struct UnsupportedBackend;

#[async_trait::async_trait]
impl VMic for UnsupportedBackend {
    fn name(&self) -> &'static str {
        "unsupported"
    }

    async fn status(&self) -> Result<VMicStatus> {
        Err(Error::internal(
            "backend virtual mic tidak tersedia di OS ini",
        ))
    }

    async fn install(&self) -> Result<()> {
        Err(Error::internal(
            "backend virtual mic tidak tersedia di OS ini",
        ))
    }

    async fn start(&self, _format: VMicFormat) -> Result<()> {
        Err(Error::internal(
            "backend virtual mic tidak tersedia di OS ini",
        ))
    }
    async fn push_pcm(&self, _pcm: &[u8]) -> Result<()> {
        Err(Error::internal(
            "backend virtual mic tidak tersedia di OS ini",
        ))
    }
    async fn stop(&self) -> Result<()> {
        Ok(())
    }
}
