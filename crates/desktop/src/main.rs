//! PhoneCam desktop: aplikasi host (README bagian C).
//!
//! Fase 0: kerangka. `app.rs` (state aplikasi), `server.rs` (HTTPS + WSS),
//! `tray.rs` (system tray), `tauri.rs` (perintah Tauri ke UI).

#![forbid(unsafe_code)]

mod app;
mod server;
mod tray;

use phonecam_core::constants::port::HTTPS;

#[tokio::main]
async fn main() -> phonecam_core::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let settings = phonecam_core::settings::Settings::default();

    let handle = tokio::spawn({
        let settings = settings.clone();
        async move { crate::server::serve(settings).await }
    });

    tracing::info!("memulai server HTTPS/WSS di port {}", HTTPS);

    crate::app::run(settings)?;

    handle.abort();
    Ok(())
}
