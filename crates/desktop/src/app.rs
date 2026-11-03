//! State aplikasi desktop (README bagian C).
//!
//! Fase 0: kerangka. State dibagikan lewat `AppState` (parking_lot::RwLock)
//! antara server HTTPS/WSS, tray, dan UI Tauri.

#![allow(dead_code)] // kerangka: diisi oleh fase berikutnya

use parking_lot::RwLock;

use phonecam_core::pairing::PairingManager;
use phonecam_core::security::PinRateLimiter;
use phonecam_core::settings::Settings;
use std::collections::HashMap;

use phonecam_core::source::{SourceId, SourceRegistry};
use phonecam_vcam::VCam;
use phonecam_vmic::{VMic, VMicFormat};

/// State global aplikasi (dibagikan ke semua handler).
pub struct AppState {
    /// Pengaturan (C3.4: perubahan langsung tanpa tombol Simpan).
    pub settings: RwLock<Settings>,
    /// Manajer PIN pairing (B2).
    pub pairing: PairingManager,
    /// Rate limiter PIN per IP (B2: 5 salah -> kunci 60 detik).
    pub rate_limiter: RwLock<PinRateLimiter>,
    /// Registri sumber HP (B2: maks 6).
    pub sources: RwLock<SourceRegistry>,
    /// Port HTTPS aktual yang dipilih server (B1: fallback bila port
    /// default sibuk; ditulis sekali oleh `serve`).
    pub https_port: RwLock<Option<u16>>,
    /// Virtual camera aktif (B6: satu output global; None bila belum aktif).
    pub vcam: RwLock<Option<std::sync::Arc<dyn VCam>>>,
    /// Virtual mic aktif (B6: satu output global; None bila belum aktif).
    pub vmic: RwLock<Option<std::sync::Arc<dyn VMic>>>,
    /// Pengirim perintah keluar ke sesi HP yang sedang terhubung, per
    /// source_id (B6/B7: cmd_role, cmd_camera, cmd_keyframe dari UI desktop).
    pub session_cmds: RwLock<HashMap<SourceId, tokio::sync::mpsc::UnboundedSender<String>>>,
    /// Penunggu ack keyframe per source_id (B6: switch tunggu maks 500 ms).
    pub keyframe_waits: RwLock<HashMap<SourceId, tokio::sync::oneshot::Sender<()>>>,
}

impl AppState {
    pub fn new(settings: Settings) -> Self {
        Self {
            settings: RwLock::new(settings),
            pairing: PairingManager::new(now_secs()),
            rate_limiter: RwLock::new(PinRateLimiter::new()),
            sources: RwLock::new(SourceRegistry::default()),
            https_port: RwLock::new(None),
            vcam: RwLock::new(None),
            vmic: RwLock::new(None),
            session_cmds: RwLock::new(HashMap::new()),
            keyframe_waits: RwLock::new(HashMap::new()),
        }
    }

    /// Aktifkan virtual camera bila belum aktif (B2: VCAM_NOT_INSTALLED
    /// bila backend tidak terpasang). Mengembalikan handle yang sama untuk
    /// semua pemanggil.
    pub async fn vcam_or_start(&self) -> phonecam_core::Result<std::sync::Arc<dyn VCam>> {
        if let Some(v) = self.vcam.read().clone() {
            return Ok(v);
        }
        let backend = phonecam_vcam::platform_backend();
        let format = phonecam_vcam::VCamFormat {
            width: 1280,
            height: 720,
            fps: 30,
            pixel_format: phonecam_vcam::VCamPixelFormat::Nv12,
        };
        backend.start(format).await?;
        let v: std::sync::Arc<dyn VCam> = std::sync::Arc::from(backend);
        *self.vcam.write() = Some(v.clone());
        Ok(v)
    }

    /// Aktifkan virtual mic bila belum aktif (B6: satu output global;
    /// VMIC_NOT_INSTALLED bila perangkat input virtual tidak ada).
    pub async fn vmic_or_start(&self) -> phonecam_core::Result<std::sync::Arc<dyn VMic>> {
        if let Some(v) = self.vmic.read().clone() {
            return Ok(v);
        }
        let backend = phonecam_vmic::platform_backend();
        backend.start(VMicFormat::default()).await?;
        let v: std::sync::Arc<dyn VMic> = std::sync::Arc::from(backend);
        *self.vmic.write() = Some(v.clone());
        Ok(v)
    }
}

/// Waktu sekarang dalam detik (epoch) — dipakai pairing dan rate limiter.
pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Menjalankan UI desktop (fase 0: placeholder; fase 1: Tauri 2).
pub fn run(_settings: Settings) -> phonecam_core::Result<()> {
    // ponytail: fase 1 ganti dengan tauri::Builder::default().run().
    tracing::info!("UI desktop belum diimplementasikan (fase 0)");
    Ok(())
}
