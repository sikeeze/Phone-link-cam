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
    /// Mutex start VCam (mencegah double-start saat panggilan konkuren).
    /// tokio::sync::Mutex: guard-nya Send, aman di-hold across .await.
    pub vcam_start: tokio::sync::Mutex<()>,
    /// Virtual mic aktif (B6: satu output global; None bila belum aktif).
    pub vmic: RwLock<Option<std::sync::Arc<dyn VMic>>>,
    /// Mutex start VMic (mencegah double-start saat panggilan konkuren).
    /// tokio::sync::Mutex: guard-nya Send, aman di-hold across .await.
    pub vmic_start: tokio::sync::Mutex<()>,
    /// Pengirim perintah keluar ke sesi HP yang sedang terhubung, per
    /// source_id (B6/B7: cmd_role, cmd_camera, cmd_keyframe dari UI desktop).
    pub session_cmds: RwLock<HashMap<SourceId, tokio::sync::mpsc::UnboundedSender<String>>>,
    /// Penunggu ack keyframe per source_id (B6: switch tunggu maks 500 ms).
    pub keyframe_waits: RwLock<HashMap<SourceId, tokio::sync::oneshot::Sender<()>>>,
    /// Guard `adb reverse` aktif (B9.2: forwarding dihapus saat di-drop,
    /// baik sesi berakhir maupun aplikasi ditutup).
    pub adb_reverse: parking_lot::Mutex<Option<crate::adb_mode::AdbReverseGuard>>,
    /// Pindah otomatis ke jalur berikutnya saat koneksi putus (B9.1:
    /// default mati; diaktifkan dari layar Koneksi).
    pub auto_failover: RwLock<bool>,
    /// Port listener HTTP mode ADB aktual (B9.2: fallback bila 8080 sibuk;
    /// ditulis sekali oleh `adb_mode::serve`). `adb reverse` mengikat port
    /// ini, dan `/api/connections` membacanya untuk `pair_url` adb.
    pub adb_port: RwLock<Option<u16>>,
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
            vcam_start: tokio::sync::Mutex::new(()),
            vmic: RwLock::new(None),
            vmic_start: tokio::sync::Mutex::new(()),
            session_cmds: RwLock::new(HashMap::new()),
            keyframe_waits: RwLock::new(HashMap::new()),
            adb_reverse: parking_lot::Mutex::new(None),
            auto_failover: RwLock::new(false),
            adb_port: RwLock::new(None),
        }
    }

    /// Aktifkan virtual camera bila belum aktif (B2: VCAM_NOT_INSTALLED
    /// bila backend tidak terpasang). Mengembalikan handle yang sama untuk
    /// semua pemanggil.
    pub async fn vcam_or_start(&self) -> phonecam_core::Result<std::sync::Arc<dyn VCam>> {
        // B6: satu output global. Cek di bawah Mutex (bukan RwLock biasa)
        // agar start tidak balapan saat dua sumber terhubung bersamaan.
        {
            let guard = self.vcam.read();
            if let Some(v) = guard.as_ref() {
                return Ok(v.clone());
            }
        }
        let _slot = self.vcam_start.lock();
        if let Some(v) = self.vcam.read().as_ref() {
            return Ok(v.clone());
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
        // B6: satu output global; Mutex mencegah double-start konkuren.
        {
            let guard = self.vmic.read();
            if let Some(v) = guard.as_ref() {
                return Ok(v.clone());
            }
        }
        let _slot = self.vmic_start.lock();
        if let Some(v) = self.vmic.read().as_ref() {
            return Ok(v.clone());
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
