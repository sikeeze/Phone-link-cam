//! Pipeline media desktop (README B6): track WebRTC -> depacketisasi ->
//! decode -> compositor -> virtual camera.
//!
//! Satu task per sumber. Frame decoded NV12 diteruskan ke backend virtual
//! camera. A7 hot path: tidak ada IO disk, tidak ada alokasi per frame di
//! luar buffer MF, logging hanya level trace/error.

use std::sync::Arc;

use phonecam_media::decode::VideoDecoder;
use phonecam_rtc::pipeline::{RtpVideoSink, VideoFrame};
use phonecam_rtc::TrackRemote;
use phonecam_vcam::{VCam, VCamFormat, VCamPixelFormat};

/// Format output virtual camera default (B6: ukuran konstan 1280x720).
const OUTPUT_WIDTH: u32 = 1280;
const OUTPUT_HEIGHT: u32 = 720;
const OUTPUT_FPS: u32 = 30;

/// Jalankan pipeline untuk satu sumber hingga track berakhir atau dibatalkan.
///
/// `decoder` disuntikkan agar bisa diganti mock di test; `vcam` sudah aktif
/// (start) sebelum pipeline berjalan — format konstan dipegang pemanggil.
pub async fn run(
    track: Arc<dyn TrackRemote>,
    mut decoder: Box<dyn VideoDecoder>,
    vcam: Arc<dyn VCam>,
    cancel: tokio::sync::watch::Receiver<bool>,
    seq_tx: tokio::sync::mpsc::UnboundedSender<u16>,
) -> phonecam_core::Result<()> {
    // Channel frame: batas antrian 2 frame (B6 ring buffer kecil), pesan
    // lengkap selalu diambil paling baru.
    let (frame_tx, mut frame_rx) = tokio::sync::mpsc::channel::<VideoFrame>(2);

    // Task sink: polling track RTP -> VideoFrame.
    let sink_cancel = cancel.clone();
    let sink = tokio::spawn(async move { RtpVideoSink::run(track, frame_tx, sink_cancel).await });

    loop {
        // B6: ambil frame terbaru; bila antrian masih menampung frame lain,
        // buang yang sudah stale dan pakai yang terbaru.
        let frame = match frame_rx.recv().await {
            Some(f) => match frame_rx.try_recv() {
                Ok(newer) => newer,
                _ => f,
            },
            None => break,
        };

        let decoded = match decoder.decode(&frame.data) {
            Ok(f) => f,
            Err(e) => {
                tracing::trace!(?e, "decode gagal; frame dibuang");
                continue;
            }
        };

        // B5: laporkan sequence number ke session (pengukuran packet loss).
        // Pengiriman gagal hanya bila session sudah selesai — diabaikan.
        let _ = seq_tx.send(frame.seq);

        // B6: ukuran output vcam konstan (1280x720). MF decoder dinegosiasikan
        // ke NV12 dengan ukuran stream; ponytail: scaling ke geometry target
        // menyusul saat decoder mendukung resolusi dinamis — MVP push apa
        // adanya bila ukuran sudah cocok.
        if let Err(e) = vcam.push_frame(&decoded.data).await {
            tracing::warn!(?e, "gagal push frame ke vcam");
        }
    }

    let _ = sink.await;
    Ok(())
}

/// Mulai vcam dengan format output konstan (B6: tidak pernah berubah saat
/// kualitas berubah).
pub async fn start_vcam() -> phonecam_core::Result<Arc<dyn VCam>> {
    let backend = phonecam_vcam::platform_backend();
    backend
        .start(VCamFormat {
            width: OUTPUT_WIDTH,
            height: OUTPUT_HEIGHT,
            fps: OUTPUT_FPS,
            pixel_format: VCamPixelFormat::Nv12,
        })
        .await?;
    Ok(Arc::from(backend))
}
