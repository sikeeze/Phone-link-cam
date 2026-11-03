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
use axum::routing::{get, post};
use axum::extract::Path;
use axum::response::Response;
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
    // B1: catat port aktual agar UI desktop bisa membuat URL pairing tanpa
    // menebak (fallback port default bila belum ditulis).
    *app_state.https_port.write() = Some(port);
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

/// Router axum (B1): `/`, `/assets/*`, `/ws`, `/health`, `/qr`, plus UI
/// desktop (C3): `/app` dan endpoint JSON `/api/*`.
/// `pub` agar integration test dapat memakai router produksi (fase 1).
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(index))
        .route("/assets/{*path}", get(asset))
        .route("/ws", get(ws_handler))
        .route("/health", get(health))
        .route("/qr", get(qr))
        // C3: UI desktop (webview Tauri maupun browser) disajikan dari
        // sub-folder `desktop/` hasil build Vite multi-page.
        .route("/app", get(desktop_index))
        .route("/app/assets/{*path}", get(desktop_asset))
        // C3.1: state untuk UI desktop (PIN, QR, sumber, status vcam).
        .route("/api/state", get(api_state))
        .route("/api/pin/regenerate", post(api_pin_regenerate))
        // B6: Jadikan aktif (switcher) — tepat satu sumber aktif.
        .route("/api/source/{id}/active", post(api_source_active))
        // B6: ganti nama perangkat dari menu kartu kamera.
        .route("/api/source/{id}/rename", post(api_source_rename))
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

/// `GET /app` — UI desktop (C3). Webview Tauri membuka URL ini dari
/// server HTTPS lokal; tidak ada server dev terpisah.
async fn desktop_index() -> impl IntoResponse {
    assets::serve("desktop/index.html").await
}

/// `GET /app/assets/*path` — aset statis UI desktop (C3).
async fn desktop_asset(
    axum::extract::Path(path): axum::extract::Path<String>,
) -> impl IntoResponse {
    assets::serve(&format!("desktop/assets/{path}")).await
}

/// `GET /api/state` — state UI desktop (C3.1): PIN aktif + hitung mundur,
/// URL pairing, QR, status pemasangan vcam, dan daftar sumber HP.
async fn api_state(State(state): State<Arc<AppState>>) -> axum::Json<serde_json::Value> {
    let pairing = state.pairing.current();
    let sources = state
        .sources
        .read()
        .list()
        .iter()
        .map(|s| {
            serde_json::json!({
                "id": s.id.to_string(),
                "name": s.info.name,
                "connection_kind": s.info.connection_kind.label_id(),
                "state": state_label(&s.status.state.0),
                "battery": s.status.battery,
                "rtt_ms": s.status.rtt_ms,
                "role": s.role,
            })
        })
        .collect::<Vec<_>>();
    axum::Json(serde_json::json!({
        "pin": pairing.pin.as_str(),
        "pin_remaining_secs": pairing.remaining(crate::app::now_secs()),
        "pair_url": format!("https://{}", lan_host(&state)),
        "qr_url": "/qr",
        "vcam_installed": vcam_installed(&state).await,
        "sources": sources,
    }))
}

/// `POST /api/pin/regenerate` — PIN baru dari UI (B2: bisa di-regenerate).
async fn api_pin_regenerate(State(state): State<Arc<AppState>>) -> axum::Json<serde_json::Value> {
    let pairing = state.pairing.regenerate(crate::app::now_secs());
    axum::Json(serde_json::json!({
        "pin": pairing.pin.as_str(),
        "pin_remaining_secs": pairing.remaining(crate::app::now_secs()),
    }))
}

/// `POST /api/source/{id}/active` — tandai sumber aktif (B6/B7: switcher).
///
/// Sumber aktif lain diturunkan ke `preview`; `cmd_role` dikirim ke kedua
/// HP bila terhubung. Mengembalikan 404 bila sumber tak dikenal.
async fn api_source_active(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Response {
    let demoted = {
        let mut sources = state.sources.write();
        if sources.get(&id).is_none() {
            return (axum::http::StatusCode::NOT_FOUND, "sumber tidak ditemukan").into_response();
        }
        sources.set_active(&id)
    };
    // B6: kabari kedua HP soal peran barunya (active/preview menentukan
    // resolusi kamera; B5: preview memakai resolusi rendah).
    let payload = phonecam_core::protocol::CmdRolePayload {
        source_id: id.clone(),
        role: phonecam_core::protocol::SourceRole::Active,
    };
    if let Ok(text) = phonecam_core::protocol::build(
        phonecam_core::protocol::types::CMD_ROLE,
        None,
        Some(&payload),
    ) {
        let _ = state.session_cmds.read().get(&id).map(|tx| tx.send(text));
    }
    if let Some(prev) = demoted {
        let payload = phonecam_core::protocol::CmdRolePayload {
            source_id: prev.clone(),
            role: phonecam_core::protocol::SourceRole::Preview,
        };
        if let Ok(text) = phonecam_core::protocol::build(
            phonecam_core::protocol::types::CMD_ROLE,
            None,
            Some(&payload),
        ) {
            let _ = state.session_cmds.read().get(&prev).map(|tx| tx.send(text));
        }
    }
    (axum::http::StatusCode::NO_CONTENT, "").into_response()
}

/// `POST /api/source/{id}/rename` — ganti nama perangkat (B6).
async fn api_source_rename(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    body: axum::Json<serde_json::Value>,
) -> Response {
    let name = body
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s.chars().count() <= 40);
    let Some(name) = name else {
        return (axum::http::StatusCode::BAD_REQUEST, "nama tidak valid").into_response();
    };
    let mut sources = state.sources.write();
    if sources.get(&id).is_none() {
        return (axum::http::StatusCode::NOT_FOUND, "sumber tidak ditemukan").into_response();
    }
    if let Some(s) = sources.get_mut(&id) {
        s.info.name = name;
    }
    (axum::http::StatusCode::NO_CONTENT, "").into_response()
}

/// Header Host tidak tersedia di handler JSON; pakai IP antarmuka privat
/// pertama (B9.1: URL memakai IP antarmuka jalur ini).
fn lan_host(state: &Arc<AppState>) -> String {
    let port = state.https_port.read().unwrap_or(settings_default_port());
    match list_private_interfaces() {
        Ok(ifs) => ifs
            .iter()
            .flat_map(|i| i.addrs.iter().copied())
            .next()
            .map(|ip| format!("{ip}:{port}"))
            .unwrap_or_else(|| format!("localhost:{port}")),
        Err(_) => format!("localhost:{port}"),
    }
}

/// Port default dari pengaturan (B1) sebagai cadangan sebelum `serve`
/// menulis port aktual.
fn settings_default_port() -> u16 {
    phonecam_core::settings::Settings::default()
        .network
        .https_port
}

/// Label state sumber untuk UI (buah SessionState::label_id bahasa Indonesia).
fn state_label(state: &phonecam_core::session::SessionState) -> &'static str {
    use phonecam_core::session::SessionState::*;
    match state {
        Idle => "Menganggur",
        Authenticating => "Mengautentikasi",
        Connecting => "Menghubungkan",
        Live => "live",
        Degraded => "Menurun",
        Reconnecting => "Menyambung ulang",
        Closed => "Tertutup",
    }
}

/// Port HTTPS tempat server mendengar (B1). Nilai konstan MVP; baca
/// pengaturan saat sudah tersedia di UI.
fn listen_port() -> u16 {
    8443
}

/// Cek status pemasangan virtual camera (C3.1 banner).
async fn vcam_installed(state: &Arc<AppState>) -> bool {
    match state.vcam_or_start().await {
        Ok(v) => matches!(v.status().await, Ok(phonecam_vcam::VCamStatus::Installed)),
        Err(_) => false,
    }
}
