//! PhoneCam core: sesi, signaling, state, Connection Manager.
//!
//! Sumber kebenaran: README.md bagian B (spesifikasi protokol dan perilaku).
//! Semua kode di crate ini sadar protokol tetapi tidak sadar OS: tidak ada
//! `cfg(target_os)` di luar crates/vcam dan crates/vmic (aturan A9).
#![allow(dead_code)] // kerangka: bagian yang belum dipakai diisi oleh fase berikutnya
#![forbid(unsafe_code)]

pub mod constants;
pub mod error;
pub mod net;
pub mod pairing;
pub mod protocol;
pub mod quality;
pub mod security;
pub mod session;
pub mod settings;
pub mod source;

pub use constants::*;
pub use error::{Error, Result, UserErrorCode};
