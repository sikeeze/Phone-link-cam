//! PhoneCam desktop: aplikasi host (README bagian C).
//!
//! Fase 1: server HTTPS + WSS sungguhnya (B1/B2/B4). Bin tipis: semua logika
//! ada di `lib.rs` (dipakai bersama oleh integration test).

#![forbid(unsafe_code)]

fn main() -> phonecam_core::Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| phonecam_core::Error::internal(format!("gagal membuat runtime: {e}")))?;
    rt.block_on(async {
        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .init();

        let settings = phonecam_core::settings::Settings::default();
        settings.validate()?;

        let app_state = std::sync::Arc::new(phonecam_desktop::AppState::new(settings));

        let handle = tokio::spawn({
            let app_state = app_state.clone();
            async move { phonecam_desktop::serve(app_state).await }
        });

        tracing::info!("server HTTPS/WSS PhoneCam siap (fase 1)");

        tokio::select! {
            res = handle => {
                match res {
                    Ok(Ok(())) => tracing::info!("server berhenti"),
                    Ok(Err(e)) => {
                        tracing::error!(?e, "server berhenti dengan error");
                        return Err(e);
                    }
                    Err(e) => tracing::error!(?e, "server task panic"),
                }
            }
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("Ctrl-C diterima, mematikan server");
            }
        }
        Ok(())
    })
}
