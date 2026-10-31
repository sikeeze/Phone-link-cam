//! Server HTTPS + WSS (README B1, B2).
//!
//! Fase 0: kerangka. Bind ke antarmuka privat saja (B2: 10/8, 172.16/12,
//! 192.168/16, link-local, subnet tethering/hotspot), sertifikat self-signed
//! via rcgen, validasi Origin, dan WebSocket signaling.
//!
//! Implementasi penuh (axum 0.8 + axum-server 0.8 + rustls 0.23) diisi
//! saat fase 1. Lihat PROGRESS.md.

#![allow(dead_code)] // kerangka: diisi oleh fase berikutnya

use phonecam_core::net::list_private_interfaces;
use phonecam_core::settings::Settings;

/// Jalankan server HTTPS+WSS sampai dihentikan.
///
/// Mengembalikan `Err` bila tidak ada antarmuka privat yang bisa dipakai,
/// atau bila pembuatan sertifikat self-signed gagal.
pub async fn serve(settings: Settings) -> phonecam_core::Result<()> {
    let bind = pick_bind_address(&settings)?;
    tracing::info!("server akan bind ke {bind} (fase 0: belum melayani)");

    // ponytail: fase 1 — axum router (static + WebSocket signaling),
    // axum-server dengan rustls 0.23 dan sertifikat rcgen.
    std::future::pending::<()>().await;
    Ok(())
}

/// Pilih alamat bind: hanya antarmuka privat (B2).
fn pick_bind_address(settings: &Settings) -> Result<String, phonecam_core::Error> {
    let port = settings.network.https_port;
    let ifs = list_private_interfaces()?;
    for iface in ifs {
        for ip in iface.addrs {
            return Ok(format!("{ip}:{port}"));
        }
    }
    Err(phonecam_core::Error::internal(
        "tidak ada antarmuka jaringan privat (Wi-Fi/LAN) yang ditemukan",
    ))
}
