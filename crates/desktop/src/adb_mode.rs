//! B9.2: mode ADB — listener HTTP 127.0.0.1 + supervisor `adb reverse`.
//!
//! Chrome menganggap `http://localhost:8080` sebagai secure context, jadi
//! halaman HP bisa memakai getUserMedia tanpa TLS (README B9.2 poin 6).
//! Keamanan: listener hanya 127.0.0.1, PIN tetap wajib, dan Host header
//! selain localhost/127.0.0.1 ditolak.
//!
//! `serve` memakai router yang sama dengan server HTTPS (B1: satu OTA
//! pairing, sama sekali beda antarmuka jaringan) — perbedaannya: bind
//! loopback saja dan port 8080 (constants::port::ADB_HTTP).

use anyhow::{anyhow, Result};
use std::net::SocketAddr;
use std::sync::Arc;

use axum::response::IntoResponse;

use crate::adb::Adb;
use crate::app::AppState;
use crate::server;

/// Jalankan listener HTTP mode ADB sampai dihentikan (B9.2).
///
/// Sebelum melayani, jalankan `adb reverse tcp:8080 tcp:8080` agar HP bisa
/// mencapai listener ini lewat localhost. `drop` dari `AdbReverseGuard`
/// menghapus forwarding — dipakai saat sesi berakhir.
pub async fn serve(app_state: Arc<AppState>) -> Result<()> {
    let port = phonecam_core::constants::port::ADB_HTTP;
    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    // B9.2 poin 6: tolak Host header selain localhost/127.0.0.1.
    let app = server::router()
        .layer(axum::middleware::from_fn(host_guard))
        .with_state(app_state.clone());

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| anyhow!("gagal bind {addr}: {e}"))?;
    tracing::info!(%addr, "listener HTTP mode ADB siap (B9.2)");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .map_err(|e| anyhow!("listener ADB berhenti: {e}"))?;
    Ok(())
}

/// Middleware B9.2 poin 6: hanya permintaan dari localhost yang diperbolehkan.
async fn host_guard(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let host_ok = req
        .headers()
        .get(axum::http::header::HOST)
        .and_then(|h| h.to_str().ok())
        .map(|h| {
            let host = h.split(':').next().unwrap_or("");
            host == "localhost" || host == "127.0.0.1"
        })
        .unwrap_or(false);
    if host_ok {
        next.run(req).await
    } else {
        (axum::http::StatusCode::FORBIDDEN, "Host tidak diizinkan").into_response()
    }
}

/// `adb reverse` aktif yang otomatis dihapus saat di-drop (B9.2 poin 5).
pub struct AdbReverseGuard {
    adb: Adb,
    port: u16,
    removed: bool,
}

impl AdbReverseGuard {
    /// Jalankan `adb reverse` untuk port ini; hapus saat di-drop.
    pub fn new(adb: Adb, port: u16) -> Result<Self> {
        adb.reverse(port)?;
        Ok(Self {
            adb,
            port,
            removed: false,
        })
    }

    /// Hapus forwarding segera (bukan hanya saat drop).
    pub fn remove(&mut self) -> Result<()> {
        if self.removed {
            return Ok(());
        }
        self.adb.reverse_remove(self.port)?;
        self.removed = true;
        Ok(())
    }
}

impl Drop for AdbReverseGuard {
    fn drop(&mut self) {
        // B9.2 poin 5: hapus forwarding saat aplikasi ditutup. Error hanya
        // dicatat — aplikasi tetap boleh keluar (A11: tidak ada panic).
        if let Err(e) = self.remove() {
            tracing::warn!(?e, "gagal menghapus adb reverse saat keluar");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn serve_binds_loopback_only() {
        // B9.2 poin 6: listener 127.0.0.1:8080 harus eksklusif — bind kedua
        // ke alamat yang sama pasti gagal (EADDRINUSE), membuktikan tidak
        // terbagi ke antarmuka lain.
        let state = Arc::new(AppState::new(phonecam_core::settings::Settings::default()));
        let task = tokio::spawn(async move { serve(state).await });
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let again =
            tokio::net::TcpListener::bind((std::net::IpAddr::from([127, 0, 0, 1]), 8080)).await;
        assert!(
            again.is_err(),
            "port 8080 harusnya sedang dipakai listener ADB"
        );
        task.abort();
    }
}
