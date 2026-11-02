//! Aset statis frontend HP (README B1: `/` dan `/assets/*` di-embed di binary).

use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../../frontend/dist"]
pub struct FrontendAssets;

/// Sajikan `/` dan `/assets/*` (B1). Maskudnya: halaman pairing PWA.
///
/// Rute tidak dikenal jatuh ke `index.html` (SPA fallback bertingkat:
/// PWA pairing sederhana, router ada di klien).
pub async fn serve(path: &str) -> Response {
    let path = path.trim_start_matches('/');
    let file = if path.is_empty() { "index.html" } else { path };
    match FrontendAssets::get(file) {
        Some(f) => {
            let mime = mime_guess::from_path(file).first_or_octet_stream();
            // Aset ber-hash (Vite) tidak akan berubah; bisa di-cache kuat.
            // index.html tidak di-cache supaya pembaruan langsung terlihat.
            let cache_control = if file == "index.html" {
                "no-cache"
            } else {
                "public, max-age=31536000, immutable"
            };
            (
                [
                    (axum::http::header::CONTENT_TYPE, mime.as_ref()),
                    (axum::http::header::CACHE_CONTROL, cache_control),
                ],
                f.data,
            )
                .into_response()
        }
        None => match FrontendAssets::get("index.html") {
            Some(f) => (
                [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
                f.data,
            )
                .into_response(),
            None => (axum::http::StatusCode::NOT_FOUND, "frontend belum dibuild").into_response(),
        },
    }
}
