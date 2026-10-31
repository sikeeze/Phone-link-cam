//! Penerima media per transport (B4: `webrtc` atau `ws-webcodecs`).
//!
//! Fase 0: kerangka. Implementasi WebRTC (webrtc-rs/str0m) dan WebSocket
//! biner + decode H.264 untuk mode ADB (B9.2) diisi saat fase berjalan.

#![allow(dead_code)]

use crate::{MediaReceiver, ReceiverConfig, ReceiverStats};
use phonecam_core::protocol::Transport;

/// Penerima WebRTC (Wi-Fi, tethering, hotspot; B1).
pub struct WebRtcReceiver {
    #[allow(unused)]
    config: ReceiverConfig,
}

impl WebRtcReceiver {
    pub fn new(config: ReceiverConfig) -> Self {
        Self { config }
    }
}

impl MediaReceiver for WebRtcReceiver {
    fn transport(&self) -> Transport {
        Transport::WebRtc
    }

    fn stats(&self) -> ReceiverStats {
        ReceiverStats::default()
    }
}

/// Penerima WebSocket + WebCodecs H.264 (mode ADB, B9.2).
pub struct WsWebCodecsReceiver {
    #[allow(unused)]
    config: ReceiverConfig,
}

impl WsWebCodecsReceiver {
    pub fn new(config: ReceiverConfig) -> Self {
        Self { config }
    }
}

impl MediaReceiver for WsWebCodecsReceiver {
    fn transport(&self) -> Transport {
        Transport::WsWebCodecs
    }

    fn stats(&self) -> ReceiverStats {
        ReceiverStats::default()
    }
}
