//! Sink audio RTP: polling track remote, decode Opus -> PCM i16_LE.
//!
//! RFC 7587: payload RTP Opus adalah satu paket Opus per paket RTP
//! (tanpa header tambahan), jadi tidak butuh depacketizer khusus.
//! B6: Opus 48 kHz mono 20 ms -> 960 sample/frame.

use std::sync::Arc;

use opus1::{Channels, OpusDecoder, SampleRate};
use tokio::sync::mpsc;

use phonecam_core::error::{Error, Result};
use webrtc::media_stream::track_remote::{TrackRemote, TrackRemoteEvent};

/// Frame PCM hasil decode (i16_LE, 48 kHz mono sesuai B6).
#[derive(Debug, Clone)]
pub struct AudioFrame {
    /// Posisi frame (timestamp RTP; clock rate 48 kHz untuk Opus).
    pub pts: u32,
    /// PCM little-endian siap dikirim ke virtual mic.
    pub pcm: bytes::Bytes,
}

/// Sink yang mem-poll satu track audio dan menerbitkan `AudioFrame`.
pub struct RtpAudioSink;

impl RtpAudioSink {
    /// Jalankan decode hingga track berakhir. Pesan dengan payload kosong
    /// dianggap silence (B6: DTX) dan dilewati (A11: tidak panic).
    pub async fn run(
        track: Arc<dyn TrackRemote>,
        frame_tx: mpsc::Sender<AudioFrame>,
        mut cancel: tokio::sync::watch::Receiver<bool>,
    ) -> Result<()> {
        // B6: Opus 48 kHz mono, 20 ms -> 960 sample.
        let mut decoder = OpusDecoder::new(SampleRate::Hz48000, Channels::Mono)
            .map_err(|e| Error::internal(format!("opus decoder init: {e:?}")))?;
        let mut pcm_buf = vec![0i16; 960];

        loop {
            tokio::select! {
                _ = cancel.changed() => {
                    if *cancel.borrow() {
                        tracing::debug!("sink audio dihentikan (cancel)");
                        return Ok(());
                    }
                }
                event = track.poll() => {
                    let Some(event) = event else {
                        tracing::debug!("track audio berakhir; sink selesai");
                        return Ok(());
                    };
                    let TrackRemoteEvent::OnRtpPacket(packet) = event else {
                        continue;
                    };
                    // DTX / silence: payload kosong, tidak ada yang didecode.
                    if packet.payload.is_empty() {
                        continue;
                    }
                    let n = match decoder.decode_i16(&packet.payload, pcm_buf.len(), &mut pcm_buf) {
                        Ok(n) => n,
                        Err(e) => {
                            // Paket rusak: lewati, jangan hentikan stream (A11).
                            tracing::trace!(?e, "opus decode gagal; paket dibuang");
                            continue;
                        }
                    };
                    let mut out = bytes::BytesMut::with_capacity(n * 2);
                    for s in &pcm_buf[..n] {
                        out.extend_from_slice(&s.to_le_bytes());
                    }
                    let frame = AudioFrame { pts: packet.header.timestamp, pcm: out.freeze() };
                    if frame_tx.send(frame).await.is_err() {
                        tracing::debug!("konsumen audio hilang; sink selesai");
                        return Ok(());
                    }
                }
            }
        }
    }
}
