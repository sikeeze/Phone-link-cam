//! PhoneCam media: decode, scaler, compositor, switcher (README B6).
#![allow(dead_code)] // kerangka: diisi oleh fase berikutnya
#![forbid(unsafe_code)]

pub mod compositor;
pub mod decode;
pub mod switcher;

/// Format pixel output ke virtual camera (B6: NV12 atau YUY2 sesuai backend).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    Nv12,
    Yuy2,
}

/// Konfigurasi composator (B6): ukuran output tetap, mode fit/fill, transform.
#[derive(Debug, Clone, Copy)]
pub struct CompositorConfig {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub fit: FitMode,
    pub mirror: bool,
    /// Rotasi derajat (kelipatan 90).
    pub rotate: u32,
    /// Zoom digital (1.0 = tanpa zoom).
    pub zoom: f32,
}

/// Mode muatan (B6): fit (letterbox) atau fill (crop).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FitMode {
    #[default]
    Fit,
    Fill,
}

impl Default for CompositorConfig {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            fps: 30,
            fit: FitMode::Fit,
            mirror: false,
            rotate: 0,
            zoom: 1.0,
        }
    }
}
