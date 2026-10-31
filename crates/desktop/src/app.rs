//! State aplikasi desktop (README bagian C).
//!
//! Fase 0: kerangka. State dibagikan lewat `AppState` (parking_lot::RwLock)
//! antara server HTTPS/WSS, tray, dan UI Tauri.

#![allow(dead_code)] // kerangka: diisi oleh fase berikutnya

use parking_lot::RwLock;

use phonecam_core::pairing::PairingManager;
use phonecam_core::security::PinRateLimiter;
use phonecam_core::settings::Settings;
use phonecam_core::source::SourceRegistry;

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
}

impl AppState {
    pub fn new(settings: Settings) -> Self {
        Self {
            settings: RwLock::new(settings),
            pairing: PairingManager::new(now_secs()),
            rate_limiter: RwLock::new(PinRateLimiter::new()),
            sources: RwLock::new(SourceRegistry::default()),
        }
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
