//! Jitter buffer minimum (B6: jitter buffer sekecil mungkin, selalu ambil frame terbaru).
//!
//! Prinsip hot path (aturan A7): tidak ada alokasi per frame, memakai buffer pool.
//! Implementasi penuh diisi saat Phase 0/1 dijalankan; kerangka ini menetapkan
//! bentuk data dan batas.

#![allow(dead_code)]

use parking_lot::Mutex;
use std::sync::Arc;

/// Frame video dengan timestamp presentasi (B6). Buffer dimiliki pool, bukan
/// dialokasikan per frame (aturan A7).
#[derive(Debug)]
pub struct Frame {
    /// Timestamp presentasi dalam satuan 90 kHz (RTP video clock).
    pub pts: u64,
    /// Byte frame (NV12/YUY2 sesuai backend, B6).
    pub data: bytes::Bytes,
    /// True jika frame ini adalah keyframe (B6: PLI saat loss).
    pub keyframe: bool,
}

/// Ring buffer kecil maksimal 2 frame (B6). Selalu ambil yang terbaru.
pub struct LatestFrames {
    inner: Arc<Mutex<Option<Frame>>>,
}

impl LatestFrames {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
        }
    }

    /// Simpan frame; frame lama dibuang (B6: selalu tampilkan frame terbaru).
    pub fn push(&self, frame: Frame) {
        let mut slot = self.inner.lock();
        *slot = Some(frame);
    }

    /// Ambil frame terbaru; None jika kosong.
    pub fn latest(&self) -> Option<Frame> {
        self.inner.lock().take()
    }
}

impl Default for LatestFrames {
    fn default() -> Self {
        Self::new()
    }
}
