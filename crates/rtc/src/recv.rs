//! Penerima media per transport (B4: `webrtc` atau `ws-webcodecs`).

#![allow(dead_code)]

use phonecam_core::protocol::Transport;

use crate::{MediaReceiver, ReceiverConfig, ReceiverStats};

/// Perintah signaling yang harus dikirim ke HP lewat WebSocket (B4).
pub enum SignalOut {
    /// Balasan SDP (answer) untuk offer dari HP.
    Answer { sdp: String },
    /// Kandidat ICE lokal (trickle).
    Ice { candidate: String },
}

/// Penerima WebRTC (Wi-Fi, tethering, hotspot; B1).
///
/// Desktop bertindak sebagai answerer: HP membuat offer (getUserMedia),
/// desktop membalas answer. Kandidat ICE ditukar bolak-balik lewat `ice`.
pub struct WebRtcReceiver {
    config: ReceiverConfig,
}

impl WebRtcReceiver {
    pub fn new(config: ReceiverConfig) -> Self {
        Self { config }
    }

    /// UDP port range untuk ICE (B1: 40000-40100).
    pub fn udp_range(&self) -> (u16, u16) {
        self.config.udp_range
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
