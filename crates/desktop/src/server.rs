//! Server HTTPS + WSS (README B1, B2, B4).
//!
//! Fase 1: axum 0.8 router (`/`, `/assets/*`, `/ws`, `/health`) dijalankan
//! dengan axum-server 0.8 + rustls 0.23. Bind hanya ke antarmuka privat (B1).
//! Sertifikat self-signed via rcgen (risiko #1, prototipe).

#![allow(dead_code)]

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{ConnectInfo, State};
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;

use phonecam_core::net::list_private_interfaces;

use crate::app::AppState;
use crate::assets;
use crate::session::Session;
use crate::tls;

/// Jalankan server HTTPS+WSS sampai dihentikan.
///
/// Mengembalikan `Err` bila tidak ada antarmuka privat yang bisa dipakai,
/// atau bila pembuatan sertifikat self-signed gagal.
pub async fn serve(app_state: Arc<AppState>) -> phonecam_core::Result<()> {
    let settings = app_state.settings.read().clone();
    let base_port = settings.network.https_port;

    // B1: bind ke setiap IP antarmuka privat (loopback ikut untuk pengujian).
    let bind_ips = bind_ips()?;
    // B1: bila port 8443 sudah dipakai, cari port kosong berikutnya.
    let port = pick_free_port(base_port, &bind_ips)?;
    tracing::info!(port, base = base_port, "port HTTPS dipilih (B1)");
    let cert = tls::generate(&bind_ips)?;
    let rustls_config = axum_server::tls_rustls::RustlsConfig::from_pem(
        cert.cert_pem.as_bytes().to_vec(),
        cert.key_pem.as_bytes().to_vec(),
    )
    .await
    .map_err(|e| phonecam_core::Error::Tls(e.to_string()))?;

    let app = router();

    // B1: satu listener per IP privat, semuanya berjalan paralel.
    let mut servers = Vec::new();
    for ip in bind_ips {
        let addr = SocketAddr::new(ip, port);
        let app = app.clone().with_state(app_state.clone());
        let cfg = rustls_config.clone();
        servers.push(tokio::spawn(async move {
            tracing::info!(%addr, "listener HTTPS siap");
            let res = axum_server::bind_rustls(addr, cfg)
                .serve(app.into_make_service_with_connect_info::<SocketAddr>())
                .await;
            match res {
                Ok(()) => tracing::info!(%addr, "listener berhenti"),
                Err(e) => tracing::error!(%addr, %e, "listener error"),
            }
        }));
    }
    for h in servers {
        let _ = h.await;
    }
    Ok(())
}

/// B1: cari port kosong mulai dari `base`. Cek dengan bind uji ke setiap IP
/// target; bila semua kosong, kembalikan port tersebut. Bila `base` sudah
/// dipakai, naik satu per satu sampai batas atas (mencegah loop tak terhingga).
fn pick_free_port(mut base: u16, ips: &[IpAddr]) -> phonecam_core::Result<u16> {
    for _ in 0..64 {
        let all_free = ips
            .iter()
            .all(|ip| std::net::TcpListener::bind(SocketAddr::new(*ip, base)).is_ok());
        if all_free {
            return Ok(base);
        }
        base = base.wrapping_add(1);
    }
    Err(phonecam_core::Error::internal(
        "tidak ada port HTTPS yang bebas (8443 sampai +64 semuanya terpakai)",
    ))
}

/// Daftar IP untuk bind (B1: hanya antarmuka privat).
fn bind_ips() -> phonecam_core::Result<Vec<IpAddr>> {
    let mut ips = Vec::new();
    for iface in list_private_interfaces()? {
        for ip in iface.addrs {
            if !ips.contains(&ip) {
                ips.push(ip);
            }
        }
    }
    if ips.is_empty() {
        return Err(phonecam_core::Error::internal(
            "tidak ada antarmuka jaringan privat (Wi-Fi/LAN) yang ditemukan",
        ));
    }
    Ok(ips)
}

/// Router axum (B1): `/`, `/assets/*`, `/ws`, `/health`, `/qr`.
/// `pub` agar integration test dapat memakai router produksi (fase 1).
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(index))
        .route("/assets/{*path}", get(asset))
        .route("/ws", get(ws_handler))
        .route("/health", get(health))
        .route("/qr", get(qr))
}

/// `GET /` — halaman pairing PWA (B1).
async fn index() -> impl IntoResponse {
    assets::serve("index.html").await
}

/// `GET /assets/*path` — aset statis PWA (B1).
/// Kunci rust-embed memakai prefix `assets/` (struktur folder dist).
async fn asset(axum::extract::Path(path): axum::extract::Path<String>) -> impl IntoResponse {
    assets::serve(&format!("assets/{path}")).await
}

/// `GET /health` — status JSON, hanya dari LAN (B1).
async fn health(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let sources = state.sources.read().list().len();
    let pairing = state.pairing.current();
    axum::Json(serde_json::json!({
        "status": "ok",
        "sources": sources,
        "pin_remaining_secs": pairing.remaining(crate::app::now_secs()),
    }))
}

/// `GET /ws` — WebSocket signaling (B1/B4).
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> axum::response::Response {
    // B2: validasi Origin header (mode LAN: https + host cocok).
    let origin = headers
        .get(axum::http::header::ORIGIN)
        .and_then(|v| v.to_str().ok());
    let host = headers
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok());
    let policy = phonecam_core::security::OriginPolicy::Lan;
    if let Err(e) = policy.validate(origin, host) {
        tracing::warn!(?origin, ?host, "Origin ditolak: {e}");
        return (axum::http::StatusCode::FORBIDDEN, "Origin tidak diizinkan").into_response();
    }
    ws.on_upgrade(move |socket| async move {
        let session = Session::new();
        session.run(socket, state, addr.ip().to_string()).await;
    })
}

/// `GET /qr` — QR SVG berisi URL pairing + PIN (B2).
///
/// Host diambil dari header `Host` (B9.1: URL memakai IP antarmuka jalur
/// itu). Header `Host` diharapkan berisi `<ip>:<port>`; bila tidak ada,
/// kembalikan 503. PIN dikirim sebagai query string agar HP langsung
/// membuka layar pairing dengan PIN terisi.
async fn qr(State(state): State<Arc<AppState>>, headers: HeaderMap) -> axum::response::Response {
    let Some(host) = headers
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok())
    else {
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "header Host hilang",
        )
            .into_response();
    };
    let pin_state = state.pairing.current();
    let pin = pin_state.pin.as_str();
    let url = format!("https://{host}/?pin={pin}");
    match qrcode::QrCode::new(url.as_bytes()) {
        Ok(code) => {
            let svg = code.render::<qrcode::render::svg::Color>().build();
            (
                [(
                    axum::http::header::CONTENT_TYPE,
                    "image/svg+xml; charset=utf-8",
                )],
                svg,
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!(?e, "gagal membuat QR");
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "gagal membuat QR",
            )
                .into_response()
        }
    }
}
