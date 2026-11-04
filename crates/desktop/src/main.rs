//! PhoneCam desktop: aplikasi host (README bagian C).
//!
//! Arsitektur MVP: server HTTPS+signaling+WebRTC (axum) berjalan di thread
//! terpisah; jendela Tauri (webview OS) membuka UI dari server yang sama.
//!
//! Sertifikat self-signed loopback tidak diverifikasi WebView2
//! (`--ignore-certificate-errors`): aman karena window hanya memuat URL
//! `https://localhost:<port>` milik server ini sendiri (B1). ponytail:
//! pinning sertifikat menyusul bersama strategi sertifikat fase selanjutnya.
//!
//! Semua logika ada di `lib.rs` (dipakai bersama oleh integration test).

#![forbid(unsafe_code)]

fn main() -> phonecam_core::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let settings = phonecam_core::settings::Settings::default();
    settings.validate()?;
    let app_state = std::sync::Arc::new(phonecam_desktop::AppState::new(settings));

    // Server HTTPS + signaling + WebRTC di thread terpisah (B1/B2/B4).
    let state_for_server = app_state.clone();
    std::thread::Builder::new()
        .name("phonecam-server".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    tracing::error!(?e, "gagal membuat runtime server");
                    return;
                }
            };
            rt.block_on(async move {
                if let Err(e) = phonecam_desktop::serve(state_for_server).await {
                    tracing::error!(?e, "server berhenti dengan error");
                }
            });
        })
        .map_err(|e| phonecam_core::Error::internal(format!("gagal spawn server: {e}")))?;

    // B9.2: listener HTTP mode ADB (127.0.0.1:8080) berjalan bersamaan
    // dengan server HTTPS. `adb reverse` dijalankan lewat UI (endpoint
    // /api/adb/reverse) dan dihapus saat aplikasi keluar (AdbReverseGuard).
    let state_for_adb = app_state.clone();
    std::thread::Builder::new()
        .name("phonecam-adb".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    tracing::error!(?e, "gagal membuat runtime ADB");
                    return;
                }
            };
            rt.block_on(async move {
                if let Err(e) = phonecam_desktop::adb_mode::serve(state_for_adb).await {
                    tracing::error!(?e, "listener ADB berhenti dengan error");
                }
            });
        })
        .map_err(|e| phonecam_core::Error::internal(format!("gagal spawn listener ADB: {e}")))?;

    // B1: tunggu port HTTPS dipilih (fallback bila 8443 sibuk).
    let port = wait_for_port(&app_state, std::time::Duration::from_secs(5));

    run_window(port);
    Ok(())
}

/// Tunggu `https_port` terisi oleh `serve`; kembali ke port default
/// bila lewat batas waktu (server mungkin gagal mulai — log mencatatnya).
fn wait_for_port(state: &phonecam_desktop::AppState, timeout: std::time::Duration) -> u16 {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Some(port) = *state.https_port.read() {
            return port;
        }
        if std::time::Instant::now() > deadline {
            tracing::warn!("port HTTPS belum terisi; memakai default");
            return phonecam_core::settings::Settings::default()
                .network
                .https_port;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// Jendela Tauri (C3: 1100x720, minimum 900x600) membuka UI desktop.
///
/// `additional_browser_args` mematikan verifikasi sertifikat WebView2
/// khusus window ini (hanya memuat `https://localhost` sendiri).
#[cfg(target_os = "windows")]
fn run_window(port: u16) {
    use tauri::WebviewUrl;
    // Format string literal: parse hanya gagal bila port tak valid (tidak
    // mungkin di sini), tapi tangani tetap agar tidak panic (A11).
    let Ok(url) = format!("https://localhost:{port}/app").parse() else {
        tracing::error!("URL UI tidak valid: port {port}");
        return;
    };
    let url: tauri::Url = url;
    let app = tauri::Builder::default().setup(move |app| {
        tauri::WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url.clone()))
            .title("PhoneCam")
            .inner_size(1100.0, 720.0)
            .min_inner_size(900.0, 600.0)
            .additional_browser_args("--ignore-certificate-errors")
            .build()
            .map_err(|e| Box::from(e) as Box<dyn std::error::Error>)?;
        Ok(())
    });
    if let Err(e) = app.run(tauri::generate_context!()) {
        tracing::error!(?e, "Tauri gagal; UI tetap tersedia di browser");
    }
}

/// OS lain (fase 4): belum ada window; UI dibuka dari browser.
#[cfg(not(target_os = "windows"))]
fn run_window(port: u16) {
    tracing::info!(port, "buka UI desktop di https://localhost:{port}/app");
}
