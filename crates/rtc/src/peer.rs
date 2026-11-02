//! Peer connection WebRTC desktop (answerer) per sumber (B1/B4/B6).

use std::sync::Arc;

use tokio::sync::mpsc;
use webrtc::media_stream::track_remote::TrackRemote;
use webrtc::peer_connection::{
    MediaEngine, PeerConnection, PeerConnectionBuilder, PeerConnectionEventHandler,
    RTCConfiguration, RTCIceCandidateInit, RTCPeerConnectionIceEvent, RTCSessionDescription,
};
use webrtc::rtp_transceiver::{RTCRtpTransceiverDirection, RTCRtpTransceiverInit};

use phonecam_core::error::{Error, Result};

use crate::recv::SignalOut;

/// Handler event peer connection. Meneruskan ICE kandidat ke channel signaling
/// dan track remote ke sink video.
struct PeerHandler {
    ice_out: mpsc::UnboundedSender<SignalOut>,
    track_out: mpsc::UnboundedSender<std::sync::Arc<dyn TrackRemote>>,
}

#[async_trait::async_trait]
impl PeerConnectionEventHandler for PeerHandler {
    async fn on_ice_candidate(&self, event: RTCPeerConnectionIceEvent) {
        // B1: hanya kandidat host yang relevan untuk mode LAN (tanpa STUN/TURN).
        let init = match event.candidate.to_json() {
            Ok(init) => init,
            Err(e) => {
                tracing::warn!(?e, "gagal encode kandidat ICE");
                return;
            }
        };
        let _ = self.ice_out.send(SignalOut::Ice {
            candidate: init.candidate,
        });
    }

    async fn on_track(&self, track: std::sync::Arc<dyn TrackRemote>) {
        // B6: track video dari HP diteruskan ke sink untuk depacketisasi.
        let _ = self.track_out.send(track);
    }
}

/// Peer connection WebRTC untuk satu sumber HP (B4).
pub struct Peer {
    pc: Arc<dyn PeerConnection>,
}

impl Peer {
    /// Buat peer connection answerer dengan transceiver video recv-only (B6).
    ///
    /// `bind_ip` harus IP antarmuka privat konkret (B1: tidak boleh 0.0.0.0).
    /// `udp_port == 0` berarti pilih port kosong pertama di rentang B1
    /// (40000-40100) lewat probe socket (B1: port auto-find free slot).
    pub async fn new(
        bind_ip: std::net::IpAddr,
        udp_port: u16,
        ice_out: mpsc::UnboundedSender<SignalOut>,
        track_out: mpsc::UnboundedSender<std::sync::Arc<dyn TrackRemote>>,
    ) -> Result<Self> {
        let port = if udp_port == 0 {
            find_free_udp_port(&bind_ip)?
        } else {
            udp_port
        };
        let mut m = MediaEngine::default();
        m.register_default_codecs()
            .map_err(|e| Error::internal(format!("gagal register codec: {e}")))?;

        let pc: Arc<dyn PeerConnection> = Arc::new(
            PeerConnectionBuilder::new()
                .with_configuration(RTCConfiguration::default())
                .with_media_engine(m)
                .with_handler(Arc::new(PeerHandler { ice_out, track_out })
                    as Arc<dyn PeerConnectionEventHandler>)
                .with_udp_addrs(vec![format!("{bind_ip}:{port}")])
                .build()
                .await
                .map_err(|e| Error::internal(format!("gagal membuat peer connection: {e}")))?,
        );

        // B4: video + audio recv-only (audio fase 2, B6 Opus 48 kHz mono).
        for kind in [
            rtc::rtp_transceiver::rtp_sender::RtpCodecKind::Video,
            rtc::rtp_transceiver::rtp_sender::RtpCodecKind::Audio,
        ] {
            pc.add_transceiver_from_kind(
                kind,
                Some(RTCRtpTransceiverInit {
                    direction: RTCRtpTransceiverDirection::Recvonly,
                    ..Default::default()
                }),
            )
            .await
            .map_err(|e| Error::internal(format!("gagal menambah transceiver: {e}")))?;
        }

        Ok(Self { pc })
    }

    /// Terapkan offer dari HP dan buat answer (B4).
    pub async fn handle_offer(&self, sdp: String) -> Result<String> {
        let offer = RTCSessionDescription::offer(sdp)
            .map_err(|e| Error::protocol(format!("offer SDP tidak valid: {e}")))?;
        self.pc
            .set_remote_description(offer)
            .await
            .map_err(|e| Error::protocol(format!("gagal terapkan offer: {e}")))?;
        let answer = self
            .pc
            .create_answer(None)
            .await
            .map_err(|e| Error::protocol(format!("gagal membuat answer: {e}")))?;
        self.pc
            .set_local_description(answer.clone())
            .await
            .map_err(|e| Error::protocol(format!("gagal terapkan answer: {e}")))?;
        Ok(answer.sdp)
    }

    /// Terapkan kandidat ICE dari HP (trickle ICE, B4).
    pub async fn add_ice_candidate(&self, candidate: String) -> Result<()> {
        self.pc
            .add_ice_candidate(RTCIceCandidateInit {
                candidate,
                sdp_mid: None,
                sdp_mline_index: None,
                username_fragment: None,
                url: None,
            })
            .await
            .map_err(|e| Error::protocol(format!("kandidat ICE tidak valid: {e}")))
    }

    /// Tutup peer connection (B3: CLOSED).
    pub async fn close(&self) -> Result<()> {
        self.pc
            .close()
            .await
            .map_err(|e| Error::internal(format!("gagal menutup peer connection: {e}")))
    }
}

/// Cari port UDP kosong pertama di rentang B1 (40000-40100) pada `bind_ip`.
///
/// Memakai probe socket standar (bind lalu langsung drop). Race antar sesi
/// tetap mungkin terjadi dalam jangka pendek; executor ICE akan error dan
/// pemanggil bisa mencoba ulang — untuk MVP ini cukup (B1: "cari port kosong
/// berikutnya jika bentrok").
fn find_free_udp_port(bind_ip: &std::net::IpAddr) -> Result<u16> {
    use std::net::UdpSocket;

    for port in crate::UDP_MEDIA_RANGE {
        let Ok(sock) = UdpSocket::bind(((*bind_ip), port)) else {
            continue;
        };
        drop(sock);
        return Ok(port);
    }
    Err(Error::internal(format!(
        "tidak ada port UDP kosong di rentang {}-{} untuk {bind_ip}",
        crate::UDP_MEDIA_RANGE_START,
        crate::UDP_MEDIA_RANGE_END
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn peer_lifecycle_answer_and_close() {
        // B1/B4: peer answerer bisa dibuat di loopback, menerima offer dummy,
        // lalu ditutup tanpa error. Memakai SDP minimal yang gagal diparse
        // sebagai offer -> error protocol, bukan panic (A11).
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<SignalOut>();
        let (track_tx, _track_rx) =
            tokio::sync::mpsc::unbounded_channel::<std::sync::Arc<dyn TrackRemote>>();
        let peer = Peer::new(
            std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            0,
            tx,
            track_tx,
        )
        .await
        .expect("peer di loopback harusnya bisa dibuat");
        // SDP tidak valid -> error protocol (A11: no panic).
        let bad = peer.handle_offer("bukan-sdp".to_string()).await;
        assert!(bad.is_err(), "SDP tidak valid harusnya error");
        // Trickle ICE candidate tidak valid -> error, bukan panic.
        let bad_ice = peer.add_ice_candidate("garbage".to_string()).await;
        let _ = bad_ice;
        peer.close().await.expect("close harusnya sukses");
        // Tidak ada ICE keluar yang diharapkan dari lifecycle ini.
        assert!(rx.try_recv().is_err(), "tidak ada sinyal ICE keluar");
    }
}
