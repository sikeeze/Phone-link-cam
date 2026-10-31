//! PhoneCam vcam: abstraksi virtual camera (README bagian Tech stack, B6).
//!
//! Aturan A9: kode platform (cfg(target_os)) hanya boleh ada di crate ini
//! (dan crates/vmic). Backend per OS mengimplementasikan trait VCam.
//!
//! - Windows 11: Media Foundation Virtual Camera (MFCreateVirtualCamera)
//! - Windows 10: DirectShow filter
//! - macOS: CoreMediaIO Camera Extension
//! - Linux: v4l2loopback
#![allow(dead_code)] // kerangka: backend per OS diisi oleh fase 1/4
#![forbid(unsafe_code)]

use phonecam_core::error::{Error, Result};

/// Output virtual camera (B6): format konstan, tidak berubah saat kualitas berubah.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VCamFormat {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub pixel_format: VCamPixelFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VCamPixelFormat {
    /// NV12 (B6).
    Nv12,
    /// YUY2 (B6).
    Yuy2,
}

/// Status pemasangan virtual camera (B8: VCAM_NOT_INSTALLED).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VCamStatus {
    /// Sudah terpasang dan aktif.
    #[default]
    Installed,
    /// Belum dipasang; UI menampilkan banner "Virtual camera belum terpasang" (C3.1).
    NotInstalled,
}

/// Trait backend virtual camera per OS (A9: implementasi cfg di sini).
#[async_trait::async_trait]
pub trait VCam: Send + Sync {
    /// Nama backend untuk log (mis. "mfvc", "dshow", "cmio", "v4l2loopback").
    fn name(&self) -> &'static str;

    /// Status pemasangan saat ini (B8).
    async fn status(&self) -> Result<VCamStatus>;

    /// Pasang virtual camera (butuh izin admin/signing; B8 VCAM_NOT_INSTALLED).
    async fn install(&self) -> Result<()>;

    /// Mulai mempublikasikan frame dengan format konstan (B6).
    async fn start(&self, format: VCamFormat) -> Result<()>;

    /// Kirim satu frame ke virtual camera. Dipanggil dari hot path (aturan A7):
    /// tidak boleh memblokir, tidak boleh mengalokasi.
    async fn push_frame(&self, frame: &[u8]) -> Result<()>;

    /// Berhenti mempublikasikan frame.
    async fn stop(&self) -> Result<()>;
}

/// Backend tidak tersedia di OS ini (stub untuk kompilasi silang).
pub struct UnsupportedBackend;

#[async_trait::async_trait]
impl VCam for UnsupportedBackend {
    fn name(&self) -> &'static str {
        "unsupported"
    }

    async fn status(&self) -> Result<VCamStatus> {
        Err(Error::internal(
            "backend virtual camera tidak tersedia di OS ini",
        ))
    }

    async fn install(&self) -> Result<()> {
        Err(Error::internal(
            "backend virtual camera tidak tersedia di OS ini",
        ))
    }

    async fn start(&self, _format: VCamFormat) -> Result<()> {
        Err(Error::internal(
            "backend virtual camera tidak tersedia di OS ini",
        ))
    }

    async fn push_frame(&self, _frame: &[u8]) -> Result<()> {
        Err(Error::internal(
            "backend virtual camera tidak tersedia di OS ini",
        ))
    }

    async fn stop(&self) -> Result<()> {
        Ok(())
    }
}
