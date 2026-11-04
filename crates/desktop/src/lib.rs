//! PhoneCam desktop (library): server HTTPS pairing + signaling (fase 1).
//!
//! Bin `src/main.rs` memakai crate ini; integration test juga memakainya
//! agar yang diuji adalah kode produksi, bukan salinannya.

#![forbid(unsafe_code)]

pub mod adb;
pub mod app;
pub mod assets;
pub mod pipeline;
pub mod server;
pub mod session;
pub mod tls;
pub mod tray;

pub use app::AppState;
pub use server::serve;
