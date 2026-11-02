//! Sink video RTP: polling track remote, depacketize H.264, kumpulkan frame.
//!
//! README B6 poin 1-2: terima RTP, jitter buffer minimum, frame masuk ke
//! ring buffer kecil (maks 2 frame) untuk selalu mengambil yang terbaru.
//! ponytail: ring buffer 2 frame di konsumen (pipeline desktop); sink ini
//! hanya merakit akses unit jadi frame demi menjaga `frame_ready` pada
//! marker bit (RFC 6184).

use std::sync::Arc;

use bytes::Bytes;
use rtc::rtp::codec::h264::H264Packet;
use rtc::rtp::packetizer::Depacketizer;
use tokio::sync::mpsc;
use webrtc::media_stream::track_remote::{TrackRemote, TrackRemoteEvent};

use phonecam_core::error::Result;

/// NAL type SPS (7): permulaan blok parameter, memaksa decode dari awal.
const NAL_TYPE_SPS: u8 = 7;
/// NAL type IDR (5): frame kunci penuh.
const NAL_TYPE_IDR: u8 = 5;

/// Frame video lengkap hasil depacketisasi (Annex-B).
#[derive(Debug, Clone)]
pub struct VideoFrame {
    /// Posisi frame (timestamp RTP 90 kHz).
    pub pts: u32,
    /// Sequence number RTP pertama frame (B5: pengukuran packet loss).
    pub seq: u16,
    /// Frame mulai dari IDR (keyframe) — penentu saat ganti sumber (B6).
    pub keyframe: bool,
    /// Byte stream Annex-B: SPS/PPS/IDR siap diterima decoder.
    pub data: Bytes,
}

/// Sink yang mem-poll satu track remote dan menerbitkan `VideoFrame`.
pub struct RtpVideoSink;

impl RtpVideoSink {
    /// Jalankan pemungut paket hingga track berakhir. Memakai satu `H264Packet`
    /// per sink karena buffer FU-A bersifat internal (memori fungsional, A7).
    ///
    /// `frame_tx` menerima frame; channel bertindak sebagai batas antrian
    /// (A8: bound queue) sehingga konsumen yang lambat tidak menumpuk memori.
    pub async fn run(
        track: Arc<dyn TrackRemote>,
        frame_tx: mpsc::Sender<VideoFrame>,
        mut cancel: tokio::sync::watch::Receiver<bool>,
    ) -> Result<()> {
        let mut depacketizer = H264Packet::default();
        let mut acc = FrameAssembler::default();

        loop {
            tokio::select! {
                _ = cancel.changed() => {
                    if *cancel.borrow() {
                        tracing::debug!("sink video dihentikan (cancel)");
                        return Ok(());
                    }
                }
                event = track.poll() => {
                    let Some(event) = event else {
                        tracing::debug!("track berakhir; sink selesai");
                        return Ok(());
                    };
                    let TrackRemoteEvent::OnRtpPacket(packet) = event else {
                        continue;
                    };
                    let pts = packet.header.timestamp;
                    let marker = packet.header.marker;
                    let nalu_type = packet.payload.first().map(|b| b & 0x1F).unwrap_or(0);
                    let starts_idr = nalu_type == NAL_TYPE_IDR || nalu_type == NAL_TYPE_SPS;

                    let piece = match depacketizer.depacketize(&packet.payload) {
                        Ok(b) => b,
                        Err(e) => {
                            // Paket rusak: buang perakitan berjalan, jangan crash (A11).
                            tracing::trace!(?e, "depacketize gagal; frame dibuang");
                            acc.clear();
                            continue;
                        }
                    };
                    if !acc.push(pts, packet.header.sequence_number, marker, starts_idr, piece) {
                        continue;
                    }
                    let Some(frame) = acc.take() else { continue; };
                    if frame_tx.send(frame).await.is_err() {
                        tracing::debug!("konsumen frame hilang; sink selesai");
                        return Ok(());
                    }
                }
            }
        }
    }
}

/// Perakit akses unit: kumpulkan NAL dengan timestamp yang sama hingga
/// marker bit menandakan akhir frame (RFC 6184 section 5.1).
#[derive(Default)]
struct FrameAssembler {
    pts: u32,
    /// Sequence number RTP pertama frame (B5: pengukuran packet loss).
    seq: u16,
    marker: bool,
    keyframe: bool,
    buf: bytes::BytesMut,
    started: bool,
    /// NAL pertama dari frame berikutnya, ditahan saat pts melompat tanpa
    /// marker agar tidak tertelan ke frame yang sedang dirakit.
    staged: Option<StagedNal>,
}

/// NAL yang ditangguhkan menunggu frame baru.
#[derive(Clone)]
struct StagedNal {
    pts: u32,
    seq: u16,
    marker: bool,
    starts_idr: bool,
    piece: Bytes,
}

impl FrameAssembler {
    /// Tambah NAL; kembalikan `true` saat frame sudah lengkap.
    fn push(&mut self, pts: u32, seq: u16, marker: bool, starts_idr: bool, piece: Bytes) -> bool {
        if !self.started {
            self.pts = pts;
            self.seq = seq;
            self.marker = marker;
            self.keyframe = starts_idr;
            self.buf.clear();
            self.started = true;
        } else if pts != self.pts {
            // Timestamp melompat tanpa marker: frame sebelumnya anggap selesai
            // (beberapa klien tidak selalu menyetel marker).
            self.marker = true;
            // Kembalikan piece ini ke pemanggil lewat buffer sementara:
            // frame baru dimulai dengan piece ini.
            self.staged = Some(StagedNal {
                pts,
                seq,
                marker,
                starts_idr,
                piece,
            });
            return true;
        }
        self.buf.extend_from_slice(&piece);
        if marker {
            self.marker = true;
        }
        self.keyframe |= starts_idr;
        self.marker && self.started
    }

    fn take(&mut self) -> Option<VideoFrame> {
        if !self.started || self.buf.is_empty() {
            return None;
        }
        let frame = VideoFrame {
            pts: self.pts,
            seq: self.seq,
            keyframe: self.keyframe,
            data: self.buf.split().freeze(),
        };
        self.started = false;
        self.keyframe = false;
        // Mulai frame baru dari NAL yang ditahan, bila ada.
        if let Some(staged) = self.staged.take() {
            self.pts = staged.pts;
            self.seq = staged.seq;
            self.marker = staged.marker;
            self.keyframe = staged.starts_idr;
            self.buf.extend_from_slice(&staged.piece);
            self.started = true;
        }
        Some(frame)
    }

    fn clear(&mut self) {
        self.started = false;
        self.staged = None;
        self.buf.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nal(type_byte: u8, payload: &[u8]) -> Bytes {
        let mut b = bytes::BytesMut::with_capacity(1 + payload.len());
        b.extend_from_slice(&[type_byte]);
        b.extend_from_slice(payload);
        b.freeze()
    }

    #[test]
    fn assembler_collects_until_marker() {
        let mut a = FrameAssembler::default();
        // NAL pertama dengan marker=false: frame belum lengkap.
        assert!(!a.push(100, 2, false, false, nal(1, &[1, 2])));
        // NAL kedua dengan marker=true: frame lengkap.
        assert!(a.push(100, 1, true, false, nal(1, &[3])));
        let f = a.take().expect("frame harus lengkap");
        assert_eq!(f.pts, 100);
        assert!(!f.keyframe);
        // NAL(1,[1,2]) = 3 byte + NAL(1,[3]) = 2 byte -> 5 byte.
        assert_eq!(f.data.len(), 5);
    }

    #[test]
    fn assembler_detects_keyframe() {
        let mut a = FrameAssembler::default();
        a.push(100, 2, false, true, nal(7, &[9]));
        a.push(100, 2, false, true, nal(5, &[10]));
        a.push(100, 1, true, false, nal(5, &[11]));
        let f = a.take().unwrap();
        assert!(f.keyframe);
    }

    #[test]
    fn pts_jump_without_marker_flushes() {
        let mut a = FrameAssembler::default();
        a.push(100, 2, false, false, nal(1, &[1]));
        // Marker=false tapi pts melompat: frame pertama keluar.
        assert!(a.push(200, 2, false, false, nal(1, &[2])));
        let f = a.take().unwrap();
        assert_eq!(f.pts, 100);
    }

    #[test]
    fn empty_assembler_returns_none() {
        let mut a = FrameAssembler::default();
        assert!(a.take().is_none());
    }
}
