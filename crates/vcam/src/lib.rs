//! PhoneCam vcam: abstraksi virtual camera (README bagian Tech stack, B6).
//!
//! Aturan A9: kode platform (cfg(target_os)) hanya boleh ada di crate ini
//! (dan crates/vmic). Backend per OS mengimplementasikan trait VCam.
//!
//! - Windows 11: Media Foundation Virtual Camera (MFCreateVirtualCamera)
//! - Windows 10: DirectShow filter
//! - macOS: CoreMediaIO Camera Extension
//! - Linux: v4l2loopback
//!
//! Fase 1: backend Windows memakai crate `virtualcam` yang membungkus OBS
//! Virtual Camera (protokol shared memory \\\\Global\\vcam_frames). Pengguna
//! memasang OBS Virtual Camera sekali (installer terpisah), lalu PhoneCam
//! menulis frame langsung tanpa admin elevation (B8).
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

impl VCamPixelFormat {
    /// Konversi ke format pixel crate virtualcam.
    #[cfg(target_os = "windows")]
    fn to_virtualcam(self) -> virtualcam::PixelFormat {
        match self {
            VCamPixelFormat::Nv12 => virtualcam::PixelFormat::NV12,
            // ponytail: virtualcam menamai YUY2 sebagai YUYV.
            VCamPixelFormat::Yuy2 => virtualcam::PixelFormat::YUYV,
        }
    }
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

// --- Windows: OBS Virtual Camera via crate `virtualcam` --------------------

/// Backend Windows memakai OBS Virtual Camera (virtualcam crate).
#[cfg(target_os = "windows")]
pub struct WindowsBackend {
    inner: parking_lot::Mutex<Option<VirtualCamHandle>>,
}

#[cfg(target_os = "windows")]
impl Default for WindowsBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "windows")]
struct VirtualCamHandle {
    camera: virtualcam::Camera,
    format: VCamFormat,
}

#[cfg(target_os = "windows")]
impl WindowsBackend {
    pub fn new() -> Self {
        Self {
            inner: parking_lot::Mutex::new(None),
        }
    }
}

#[cfg(target_os = "windows")]
#[async_trait::async_trait]
impl VCam for WindowsBackend {
    fn name(&self) -> &'static str {
        "obs-vcam"
    }

    async fn status(&self) -> Result<VCamStatus> {
        // OBS Virtual Camera terdaftar sebagai filter DirectShow; deteksi
        // keberadaan lewat is_available() crate virtualcam.
        if virtualcam::backend::windows_obs::is_available() {
            Ok(VCamStatus::Installed)
        } else {
            Ok(VCamStatus::NotInstalled)
        }
    }

    async fn install(&self) -> Result<()> {
        // OBS Virtual Camera dipasang lewat installer OBS (distribusi
        // terpisah, lisensi GPL). PhoneCam tidak boleh membundel binary
        // berlisensi GPL tanpa keputusan ADR — arahkan pengguna.
        Err(Error::user(
            phonecam_core::error::UserErrorCode::VcamNotInstalled,
        ))
    }

    async fn start(&self, format: VCamFormat) -> Result<()> {
        let mut guard = self.inner.lock();
        if guard.is_some() {
            return Err(Error::internal("virtual camera sudah aktif"));
        }
        let camera = virtualcam::Camera::builder(format.width, format.height, format.fps as f64)
            .format(format.pixel_format.to_virtualcam())
            .build()
            .map_err(|e| Error::internal(format!("gagal membuka OBS Virtual Camera: {e}")))?;
        *guard = Some(VirtualCamHandle { camera, format });
        Ok(())
    }

    async fn push_frame(&self, frame: &[u8]) -> Result<()> {
        // A7 hot path: lock parking_lot (thread kecil), send frame.
        let mut guard = self.inner.lock();
        let Some(handle) = guard.as_mut() else {
            return Err(Error::internal("virtual camera belum distart"));
        };
        let f = handle.format;
        let expected = match f.pixel_format {
            VCamPixelFormat::Nv12 => (f.width as usize * f.height as usize * 3) / 2,
            VCamPixelFormat::Yuy2 => f.width as usize * f.height as usize * 2,
        };
        if frame.len() != expected {
            return Err(Error::internal(format!(
                "ukuran frame {} tidak sesuai format yang diminta {}",
                frame.len(),
                expected
            )));
        }
        handle
            .camera
            .send_native(frame)
            .map_err(|e| Error::internal(format!("gagal mengirim frame vcam: {e:?}")))
    }

    async fn stop(&self) -> Result<()> {
        let mut guard = self.inner.lock();
        if let Some(mut handle) = guard.take() {
            handle
                .camera
                .close()
                .map_err(|e| Error::internal(format!("gagal menutup vcam: {e:?}")))?;
        }
        Ok(())
    }
}

/// Backend virtual camera untuk OS ini (A9). Mengembalikan `Arc<dyn VCam>`
/// (bukan `Box`) agar future yang menahan backend tetap `Send` — handler
/// axum wajib `Send` (Phase 4: `GET /api/devices` memanggil `status()`
/// langsung dari handler).
#[cfg(target_os = "windows")]
pub fn platform_backend() -> std::sync::Arc<dyn VCam> {
    std::sync::Arc::new(WindowsBackend::new())
}

/// Backend virtual camera untuk OS ini (A9: non-Windows belum diimplementasi).
#[cfg(not(target_os = "windows"))]
pub fn platform_backend() -> std::sync::Arc<dyn VCam> {
    std::sync::Arc::new(UnsupportedBackend)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn unsupported_backend_errors_cleanly() {
        let b = UnsupportedBackend;
        assert_eq!(b.name(), "unsupported");
        assert!(b.status().await.is_err());
        assert!(b.install().await.is_err());
        assert!(b
            .start(VCamFormat {
                width: 1280,
                height: 720,
                fps: 30,
                pixel_format: VCamPixelFormat::Nv12
            })
            .await
            .is_err());
        assert!(b.push_frame(&[]).await.is_err());
        assert!(b.stop().await.is_ok());
    }

    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn windows_backend_reports_status_without_panic() {
        // B8: status() harus errorType bersih, bukan panic, walau OBS
        // Virtual Camera belum dipasang di mesin uji.
        let b = WindowsBackend::new();
        let s = b.status().await;
        assert!(s.is_ok(), "status harus Ok(Installed|NotInstalled): {s:?}");
        // install() harus error bermakna (B8), bukan panic.
        let i = b.install().await;
        assert!(i.is_err(), "install tanpa OBS harus error terkode: {i:?}");
    }

    #[test]
    fn vcam_format_copy() {
        let f = VCamFormat {
            width: 1280,
            height: 720,
            fps: 30,
            pixel_format: VCamPixelFormat::Nv12,
        };
        assert_eq!(f.width, 1280);
        assert_eq!(f.pixel_format, VCamPixelFormat::Nv12);
    }
}
