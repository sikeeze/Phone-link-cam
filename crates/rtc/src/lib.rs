//! PhoneCam WebRTC receiver (README bagian "Cara kerja", B6, Phase 0).
//!
//! Fase 0 (kerangka): tipe dan trait untuk menerima RTP dari HP via WebRTC.
//! Implementasi webrtc-rs/str0m diisi saat Phase 0 dijalankan.
#![allow(dead_code)] // kerangka: diisi oleh fase berikutnya
#![forbid(unsafe_code)]

use phonecam_core::protocol::Transport;

pub mod jitter;
pub mod recv;

/// Konfigurasi penerima WebRTC per sumber (B1/B5).
#[derive(Debug, Clone)]
pub struct ReceiverConfig {
    /// Rentang UDP media (B1: 40000-40100).
    pub udp_range: (u16, u16),
    /// Hanya kandidat host, tanpa STUN/TURN (B1).
    pub host_candidates_only: bool,
    /// Tangani kandidat mDNS (.local) (B1, risiko #2).
    pub resolve_mdns: bool,
}

impl Default for ReceiverConfig {
    fn default() -> Self {
        Self {
            udp_range: (40000, 40100),
            host_candidates_only: true,
            resolve_mdns: true,
        }
    }
}

/// Statistik penerima untuk adaptasi bitrate (B5) dan badge RTT (B9.1).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ReceiverStats {
    /// Rata-rata RTT dalam ms.
    pub rtt_ms: Option<u32>,
    /// Packet loss dalam persen (0-100).
    pub packet_loss_pct: Option<f32>,
    /// Bitrate masuk saat ini (bps).
    pub bitrate_bps: Option<u64>,
    /// Jumlah frame yang diterima.
    pub frames_received: u64,
    /// Jumlah frame yang dibuang karena usang (B6: selalu tampilkan frame terbaru).
    pub frames_dropped_stale: u64,
}

/// Trait penerima media per sumber. Implementasi per transport ada di recv.
pub trait MediaReceiver: Send + Sync {
    /// Transport yang didukung (B4).
    fn transport(&self) -> Transport;
    /// Statistik terbaru (B5/B9.1).
    fn stats(&self) -> ReceiverStats;
}
