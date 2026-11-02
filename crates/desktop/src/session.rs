//! Sesi WebSocket signaling per koneksi HP (README B2/B3/B4).

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket};
use futures::{SinkExt, StreamExt};
use tokio::time::interval;

use phonecam_core::constants::limits::MAX_MESSAGE_BYTES;
use phonecam_core::pairing::PinCheck;
use phonecam_core::protocol::{
    self, types, AuthCredential, AuthFailCode, AuthFailPayload, CmdQualityPayload, HelloPayload,
    IcePayload, QualityPreset, SdpPayload,
};
use phonecam_core::source::{Source, SourceInfo};
use phonecam_media::decode::VideoDecoder;
use phonecam_rtc::audio_sink::RtpAudioSink;
use phonecam_rtc::peer::Peer;
use phonecam_rtc::recv::SignalOut;
use phonecam_rtc::{RtpCodecKind, TrackRemote};

use crate::app::{now_secs, AppState};

/// Pembatal pipeline: mengirim `true` saat di-drop sehingga sink RTP berhenti.
struct CancelGuard(tokio::sync::watch::Sender<bool>);

impl Drop for CancelGuard {
    fn drop(&mut self) {
        let _ = self.0.send(true);
    }
}

/// Pengukur kualitas per sumber (B5): RTT dari `pong` + loss dari gap
/// sequence RTP. Interaksi dengan HP hanya lewat `cmd_quality` saat preset
/// berubah.
#[derive(Default)]
pub struct QualityMonitor {
    state: phonecam_core::quality::QualityState,
}

impl QualityMonitor {
    /// Catat RTT (ms) dari `pong`; kembalikan preset bila berubah (B5).
    pub fn sample_rtt(&mut self, rtt_ms: u32, now_ms: u64) -> Option<QualityPreset> {
        phonecam_core::quality::sample_rtt(&mut self.state, rtt_ms, now_ms)
    }

    /// Catat sequence number RTP dari pipeline; hitung gap sebagai loss (B5).
    pub fn sample_seq(&mut self, seq: u16, now_ms: u64) -> Option<QualityPreset> {
        phonecam_core::quality::sample_seq(&mut self.state, seq, now_ms)
    }

    /// Buat payload `cmd_quality` (B4/B5).
    pub fn payload(&self) -> CmdQualityPayload {
        CmdQualityPayload {
            quality: self.state.current,
        }
    }
}

/// Interval heartbeat (B4: ping tiap 2 detik, timeout 6 detik).
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);
/// Timeout heartbeat: 3x interval (B4: 6 detik).
const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(6);

/// Sesi satu koneksi WS: state machine B3 mini (AUTH > LIVE > CLOSED).
pub struct Session {
    state: SessionPhase,
    /// source_id yang dimiliki koneksi ini; dipakai cleanup saat WS tutup (B2).
    source_id: Option<String>,
    /// Payload `hello` tersimpan; dipakai mengisi SourceInfo saat auth (B4).
    pending_hello: Option<HelloPayload>,
    /// Peer connection WebRTC (dibuat saat auth berhasil; B4).
    peer: Option<Peer>,
    /// Penerima ICE candidate dari peer connection agar bisa diteruskan ke HP.
    ice_rx: Option<tokio::sync::mpsc::UnboundedReceiver<SignalOut>>,
    /// Penerima track video dari peer connection (B6: jalur media).
    track_rx: Option<tokio::sync::mpsc::UnboundedReceiver<std::sync::Arc<dyn TrackRemote>>>,
    /// True bila HP memutus dengan `bye` eksplisit (B3: langsung CLOSED,
    /// tidak masuk RECONNECTING).
    bye_received: bool,
    /// Pengukur kualitas B5 untuk sumber ini (RTT + loss → cmd_quality).
    quality: QualityMonitor,
    /// Penerima sequence number RTP dari pipeline video (B5: pengukuran loss).
    seq_rx: Option<tokio::sync::mpsc::UnboundedReceiver<u16>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionPhase {
    /// Menunggu `hello` + `auth`.
    Authenticating,
    /// Sudah auth; pesan media/signaling aktif.
    Live,
    Closed,
}

impl Session {
    pub fn new() -> Self {
        Self {
            state: SessionPhase::Authenticating,
            source_id: None,
            pending_hello: None,
            peer: None,
            ice_rx: None,
            track_rx: None,
            bye_received: false,
            quality: QualityMonitor::default(),
            seq_rx: None,
        }
    }

    /// Loop utama: baca pesan, heartbeat, dan dispatch (B4).
    pub async fn run(mut self, ws: WebSocket, ctx: Arc<AppState>, ip: String) {
        let (mut tx, mut rx) = ws.split();
        let mut tick = interval(HEARTBEAT_INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // Autentikasi: tidak ada heartbeat sampai auth berhasil (B3).
        let mut last_msg = Instant::now();

        loop {
            // B5: konsumsi sequence number RTP yang dilaporkan pipeline video
            // (pengukuran packet loss). Dipisah dari select! agar hot path
            // media tidak membanjiri branch select utama.
            if let Some(seq_rx) = self.seq_rx.as_mut() {
                while let Ok(seq) = seq_rx.try_recv() {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);
                    if let Some(preset) = self.quality.sample_seq(seq, now) {
                        let payload = self.quality.payload();
                        if let Ok(text) = protocol::build(types::CMD_QUALITY, None, Some(&payload))
                        {
                            let _ = tx.send(Message::Text(text.into())).await;
                        }
                        if let Some(id) = self.source_id.as_deref() {
                            let mut sources = ctx.sources.write();
                            if let Some(s) = sources.get_mut(id) {
                                s.quality = preset;
                            }
                        }
                        tracing::info!(?preset, "kualitas adaptif: packet loss (B5)");
                    }
                }
            }
            tokio::select! {
                msg = rx.next() => match msg {
                    Some(Ok(Message::Text(text))) => {
                        last_msg = Instant::now();
                        // B4: batasi pesan masuk (16 KiB) sebelum parse.
                        if text.len() > MAX_MESSAGE_BYTES {
                            tracing::warn!(
                                len = text.len(),
                                "pesan masuk melebihi batas dari {ip}; ditutup"
                            );
                            break;
                        }
                        let res = self.handle_text(&text, &ctx, &ip, &mut tx).await;
                        if res.is_break() { break; }
                    }
                    Some(Ok(Message::Binary(_))) => {
                        // B4: pesan biner diabaikan (media lewat UDP/WebRTC).
                        tracing::debug!("mengabaiki pesan biner dari {ip}");
                    }
                    Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => {
                        last_msg = Instant::now();
                    }
                    Some(Ok(Message::Close(_))) => break,
                    Some(Err(e)) => {
                        tracing::warn!(?e, "error baca WS dari {ip}");
                        break;
                    }
                    None => break,
                },
                _ = tick.tick() => {
                    // B2: bersihkan tracker IP yang lock-nya sudah berakhir
                    // (mencegah by_ip HashMap tumbuh tanpa batas).
                    {
                        let mut rl = ctx.rate_limiter.write();
                        rl.sweep(now_secs());
                    }
                    // B6: bila track dari HP sudah sampai, jalankan pipeline
                    // media satu kali (video -> decode -> vcam; audio -> vmic).
                    if let Some(track_rx) = self.track_rx.as_mut() {
                        while let Ok(track) = track_rx.try_recv() {
                            let ctx2 = ctx.clone();
                            // B5: pipeline video melaporkan sequence number RTP
                            // kembali ke session untuk pengukuran packet loss.
                            let (seq_tx, seq_rx) =
                                tokio::sync::mpsc::unbounded_channel::<u16>();
                            if self.seq_rx.is_none() {
                                self.seq_rx = Some(seq_rx);
                            }
                            tokio::spawn(async move {
                                let kind = track.kind().await;
                                match kind {
                                    RtpCodecKind::Video => {
                                        Self::spawn_video(ctx2, track, seq_tx).await
                                    }
                                    RtpCodecKind::Audio => Self::spawn_audio(ctx2, track).await,
                                    _ => {}
                                }
                            });
                        }
                    }
                    // B4: teruskan ICE candidate dari peer connection ke HP.
                    if let Some(ice_rx) = self.ice_rx.as_mut() {
                        while let Ok(sig) = ice_rx.try_recv() {
                            let res = match sig {
                                SignalOut::Ice { candidate } => protocol::build(
                                    types::ICE,
                                    None,
                                    Some(&IcePayload {
                                        candidate,
                                        source_id: self.source_id.clone(),
                                        sdp_mid: None,
                                        sdp_mline_index: None,
                                    }),
                                ),
                                SignalOut::Answer { sdp } => protocol::build(
                                    types::ANSWER,
                                    None,
                                    Some(&SdpPayload {
                                        sdp,
                                        source_id: self.source_id.clone(),
                                    }),
                                ),
                            };
                            if let Ok(text) = res {
                                if tx.send(Message::Text(text.into())).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                    if self.state == SessionPhase::Live {
                        if last_msg.elapsed() > HEARTBEAT_TIMEOUT {
                            tracing::warn!("heartbeat timeout dari {ip}");
                            break;
                        }
                        match protocol::build::<()>(types::PING, None, None) {
                            Ok(text) => {
                                if tx.send(Message::Text(text.into())).await.is_err() {
                                    break;
                                }
                            }
                            // Tidak akan terjadi untuk pesan kosong, tetapi
                            // jangan kirim string kosong (B4: format pesan).
                            Err(e) => tracing::error!(?e, "gagal build ping"),
                        }
                    }
                }
            }
        }
        self.cleanup(&ctx, &ip).await;
    }

    /// Tangani satu pesan teks; kirim balasan ke `tx`.
    async fn handle_text(
        &mut self,
        text: &str,
        ctx: &Arc<AppState>,
        ip: &str,
        tx: &mut futures::stream::SplitSink<WebSocket, Message>,
    ) -> ControlFlow {
        let dec = match protocol::parse_message(text) {
            Ok(d) => d,
            Err(e) => {
                tracing::debug!(?e, "pesan tidak valid dari {ip}");
                // B4: pesan di luar skema diabaikan + log debug.
                return ControlFlow::Continue;
            }
        };
        match self.state {
            SessionPhase::Authenticating => self.handle_auth(&dec, ctx, ip, tx).await,
            SessionPhase::Live => self.handle_live(&dec, ctx, ip, tx).await,
            SessionPhase::Closed => ControlFlow::Break,
        }
    }

    /// Fase AUTH: hanya `hello` lalu `auth` (B4).
    async fn handle_auth(
        &mut self,
        dec: &protocol::DecodedMessage,
        ctx: &Arc<AppState>,
        ip: &str,
        tx: &mut futures::stream::SplitSink<WebSocket, Message>,
    ) -> ControlFlow {
        match dec.msg_type.as_str() {
            types::HELLO => {
                // B4: catat info perangkat; dipakai saat auth berhasil untuk
                // mengisi SourceInfo (bukan IP sebagai pengganti nama).
                let hello: HelloPayload = match dec.payload.as_ref() {
                    Some(p) => match serde_json::to_value(p) {
                        Ok(v) => match protocol::payload_from_value::<HelloPayload>(&v) {
                            Ok(h) => h,
                            Err(_) => {
                                tracing::debug!("hello payload tidak valid dari {ip}");
                                return ControlFlow::Continue;
                            }
                        },
                        Err(_) => {
                            tracing::debug!("hello payload tidak valid dari {ip}");
                            return ControlFlow::Continue;
                        }
                    },
                    None => {
                        tracing::debug!("hello tanpa payload dari {ip}");
                        return ControlFlow::Continue;
                    }
                };
                self.pending_hello = Some(hello);
                ControlFlow::Continue
            }
            types::PING => {
                // B4: heartbeat harus dibalas bahkan sebelum auth (keepalive).
                if let Ok(text) = protocol::build::<()>(types::PONG, None, None) {
                    let _ = tx.send(Message::Text(text.into())).await;
                }
                ControlFlow::Continue
            }
            types::AUTH => {
                let cred: AuthCredential = match dec.payload.as_ref() {
                    Some(p) => {
                        // B4: payload auth = {"credential": {...}}, bukan credential
                        // langsung. Ambil field-nya, lalu deserialize.
                        let cred_value = p.get("credential").cloned().unwrap_or_default();
                        match protocol::payload_from_value(&cred_value) {
                            Ok(c) => c,
                            Err(_) => return self.send_fail(tx, AuthFailCode::BadPin, None).await,
                        }
                    }
                    None => return self.send_fail(tx, AuthFailCode::BadPin, None).await,
                };
                // Rate limit dulu (B2): IP yang dikunci tidak boleh verifikasi.
                // Catatan: guard di-drop sebelum await agar future tetap Send.
                let ip_addr: std::net::IpAddr = ip
                    .parse()
                    .unwrap_or(std::net::IpAddr::V4([0, 0, 0, 0].into()));
                // B2: IP yang sedang dikunci tidak boleh verifikasi.
                // Catatan: guard di-drop sebelum await (tidak Send) — baca dulu,
                // tunggu hasilnya, baru await.
                let locked = {
                    let rl = ctx.rate_limiter.read();
                    rl.is_locked(ip_addr, now_secs())
                };
                if locked {
                    // B2: retryAfter = sisa waktu kunci nyata, bukan konstan 60.
                    let retry_after = ctx
                        .rate_limiter
                        .read()
                        .retry_after(ip_addr, now_secs())
                        .unwrap_or(phonecam_core::constants::limits::PIN_LOCKOUT_SECS as u32);
                    return self
                        .send_fail(tx, AuthFailCode::Locked, Some(retry_after))
                        .await;
                }
                // Verifikasi PIN/token (B2).
                match self.verify_credential(&cred, ctx, ip) {
                    VerifyOutcome::Ok { source_id, token } => {
                        // B2: reset tracker untuk IP ini setelah berhasil.
                        ctx.rate_limiter.write().reset(ip_addr);
                        // B3: sumber kembali LIVE (reconnect lewat token) atau
                        // baru saja pair lewat PIN.
                        {
                            let mut sources = ctx.sources.write();
                            if let Some(s) = sources.get_mut(&source_id) {
                                s.status.state = phonecam_core::source::SessionStateDefault(
                                    phonecam_core::session::SessionState::Live,
                                );
                            }
                        }
                        let msg = protocol::build(
                            types::AUTH_OK,
                            None,
                            Some(&phonecam_core::protocol::AuthOkPayload {
                                source_id: source_id.clone(),
                                token,
                                quality: phonecam_core::protocol::QualityPreset::Balanced,
                            }),
                        );
                        if let Ok(text) = msg {
                            let _ = tx.send(Message::Text(text.into())).await;
                        }
                        // B4: siapkan peer connection WebRTC (answerer) setelah
                        // auth berhasil. Kegagalan tidak fatal — HP tetap bisa
                        // mencoba offer; error dibalas per pesan.
                        // B1: bind ke IP antarmuka privat yang menuju HP.
                        let (ice_tx, ice_rx) = tokio::sync::mpsc::unbounded_channel::<SignalOut>();
                        let (track_tx, track_rx) = tokio::sync::mpsc::unbounded_channel::<
                            std::sync::Arc<dyn TrackRemote>,
                        >();
                        let bind_ip = phonecam_core::net::bind_ip_for_peer(
                            &ip.parse()
                                .unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)),
                        );
                        match Peer::new(bind_ip, 0, ice_tx, track_tx).await {
                            Ok(peer) => {
                                self.peer = Some(peer);
                                self.ice_rx = Some(ice_rx);
                                self.track_rx = Some(track_rx);
                            }
                            Err(e) => {
                                tracing::warn!(?e, "gagal membuat peer connection untuk {ip}");
                            }
                        }
                        // B2: catat source_id milik koneksi ini; dipakai cleanup
                        // saat WS tutup agar slot ke-6 tidak bocor.
                        self.source_id = Some(source_id);
                        self.state = SessionPhase::Live;
                        ControlFlow::Continue
                    }
                    VerifyOutcome::BadPin => {
                        // B2: catat percobaan salah; kunci bila sudah 5 kali.
                        let locked = {
                            let mut rl = ctx.rate_limiter.write();
                            match rl.record_wrong(ip_addr, now_secs()) {
                                PinCheck::Locked { retry_after } => Some(retry_after),
                                _ => None,
                            }
                        };
                        match locked {
                            Some(retry_after) => {
                                self.send_fail(tx, AuthFailCode::Locked, Some(retry_after))
                                    .await
                            }
                            None => self.send_fail(tx, AuthFailCode::BadPin, None).await,
                        }
                    }
                    VerifyOutcome::Expired => self.send_fail(tx, AuthFailCode::Expired, None).await,
                    VerifyOutcome::SessionFull => {
                        self.send_fail(tx, AuthFailCode::SessionFull, None).await
                    }
                }
            }
            _ => {
                // B4: pesan tidak dikenal diabaikan + log debug.
                tracing::debug!(t = %dec.msg_type, "pesan tidak dikenal dari {ip}");
                ControlFlow::Continue
            }
        }
    }

    /// Jalankan pipeline video (depacketize -> decode -> vcam, B6).
    ///
    /// `seq_tx` menerima sequence number RTP per frame untuk pengukuran
    /// packet loss adaptif (B5).
    async fn spawn_video(
        ctx: Arc<AppState>,
        track: Arc<dyn TrackRemote>,
        seq_tx: tokio::sync::mpsc::UnboundedSender<u16>,
    ) {
        let vcam = match ctx.vcam_or_start().await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(?e, "gagal memulai vcam untuk sumber");
                return;
            }
        };
        let decoder: Box<dyn VideoDecoder> = match phonecam_media::decode_win::MfDecoder::new() {
            Ok(d) => Box::new(d),
            Err(e) => {
                tracing::warn!(?e, "dekoder MF tidak tersedia");
                return;
            }
        };
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        let _cancel_guard = CancelGuard(cancel_tx);
        if let Err(e) = crate::pipeline::run(track, decoder, vcam, cancel_rx, seq_tx).await {
            tracing::warn!(?e, "pipeline video berakhir dengan error");
        }
    }

    /// Jalankan pipeline audio (Opus -> PCM -> vmic, B6).
    async fn spawn_audio(ctx: Arc<AppState>, track: Arc<dyn TrackRemote>) {
        let vmic = match ctx.vmic_or_start().await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(?e, "gagal memulai vmic untuk sumber");
                return;
            }
        };
        let (frame_tx, mut frame_rx) = tokio::sync::mpsc::channel(8);
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        let _cancel_guard = CancelGuard(cancel_tx);
        let sink = tokio::spawn(async move {
            if let Err(e) = RtpAudioSink::run(track, frame_tx, cancel_rx).await {
                tracing::warn!(?e, "sink audio berakhir dengan error");
            }
        });
        while let Some(frame) = frame_rx.recv().await {
            if let Err(e) = vmic.push_pcm(&frame.pcm).await {
                tracing::warn!(?e, "vmic push gagal");
                break;
            }
        }
        let _ = sink.await;
    }

    /// Fase LIVE: pesan signaling media (B4).
    async fn handle_live(
        &mut self,
        dec: &protocol::DecodedMessage,
        ctx: &Arc<AppState>,
        ip: &str,
        tx: &mut futures::stream::SplitSink<WebSocket, Message>,
    ) -> ControlFlow {
        match dec.msg_type.as_str() {
            types::PING => {
                // B4: ping > pong otomatis.
                if let Ok(text) = protocol::build::<()>(types::PONG, None, None) {
                    let _ = tx.send(Message::Text(text.into())).await;
                }
                ControlFlow::Continue
            }
            types::PONG => {
                // B5: RTT dari timestamp envelope `pong` (B4: ts diisi pengirim).
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                let rtt = now.saturating_sub(dec.ts);
                if let Some(preset) = self.quality.sample_rtt(rtt as u32, now) {
                    // B5: preset berubah → kirim cmd_quality + update state.
                    let payload = self.quality.payload();
                    if let Ok(text) = protocol::build(types::CMD_QUALITY, None, Some(&payload)) {
                        let _ = tx.send(Message::Text(text.into())).await;
                    }
                    if let Some(id) = self.source_id.as_deref() {
                        let mut sources = ctx.sources.write();
                        if let Some(s) = sources.get_mut(id) {
                            s.quality = preset;
                            // B3: DEGRADED saat turun, LIVE saat naik kembali.
                            let st = if rtt > phonecam_core::quality::thresholds::RTT_MS as u64 {
                                phonecam_core::session::SessionState::Degraded
                            } else {
                                phonecam_core::session::SessionState::Live
                            };
                            s.status.state = phonecam_core::source::SessionStateDefault(st);
                        }
                    }
                    tracing::info!(?preset, rtt, "kualitas adaptif (B5)");
                }
                ControlFlow::Continue
            }
            types::BYE => {
                tracing::info!(ip, "HP memutuskan (bye)");
                self.bye_received = true;
                ControlFlow::Break
            }
            types::OFFER => {
                // B4: HP mengirim SDP offer; desktop membalas answer (B6).
                let Some(peer) = self.peer.as_ref() else {
                    tracing::warn!("offer dari {ip} tapi peer connection belum siap");
                    return ControlFlow::Continue;
                };
                let sdp: SdpPayload = match dec.payload.as_ref() {
                    Some(p) => match serde_json::to_value(p) {
                        Ok(v) => match protocol::payload_from_value::<SdpPayload>(&v) {
                            Ok(s) => s,
                            Err(_) => {
                                tracing::debug!("offer payload tidak valid dari {ip}");
                                return ControlFlow::Continue;
                            }
                        },
                        Err(_) => return ControlFlow::Continue,
                    },
                    None => return ControlFlow::Continue,
                };
                match peer.handle_offer(sdp.sdp).await {
                    Ok(answer_sdp) => {
                        let payload = SdpPayload {
                            sdp: answer_sdp,
                            source_id: self.source_id.clone(),
                        };
                        if let Ok(text) = protocol::build(types::ANSWER, None, Some(&payload)) {
                            let _ = tx.send(Message::Text(text.into())).await;
                        }
                    }
                    Err(e) => {
                        tracing::warn!(?e, "gagal proses offer dari {ip}");
                    }
                }
                ControlFlow::Continue
            }
            types::ICE => {
                // B4: trickle ICE — terapkan kandidat dari HP.
                let Some(peer) = self.peer.as_ref() else {
                    return ControlFlow::Continue;
                };
                let ice: IcePayload = match dec.payload.as_ref() {
                    Some(p) => match serde_json::to_value(p) {
                        Ok(v) => match protocol::payload_from_value::<IcePayload>(&v) {
                            Ok(i) => i,
                            Err(_) => return ControlFlow::Continue,
                        },
                        Err(_) => return ControlFlow::Continue,
                    },
                    None => return ControlFlow::Continue,
                };
                if let Err(e) = peer.add_ice_candidate(ice.candidate).await {
                    tracing::debug!(?e, "kandidat ICE ditolak dari {ip}");
                }
                ControlFlow::Continue
            }
            types::SOURCE_UPDATE => {
                // B4: metadata sumber (resolusi, baterai, dll).Status dipakai
                // fase 3; di sini hanya dicatat.
                tracing::debug!(t = %dec.msg_type, "source_update dari {ip}");
                ControlFlow::Continue
            }
            _ => {
                tracing::debug!(t = %dec.msg_type, "pesan tidak dikenal dari {ip}");
                ControlFlow::Continue
            }
        }
    }

    /// Verifikasi PIN atau token (B2). Token reconnect: 128-bit, per sesi desktop.
    fn verify_credential(
        &self,
        cred: &AuthCredential,
        ctx: &Arc<AppState>,
        ip: &str,
    ) -> VerifyOutcome {
        let now = now_secs();
        match cred {
            AuthCredential::Pin(pin) => match ctx.pairing.verify(pin, now) {
                Ok(PinCheck::Ok) => {
                    let token = phonecam_core::pairing::SessionToken::generate();
                    let source_id = uuid::Uuid::new_v4().to_string();
                    // B4: SourceInfo dari payload `hello` (bukan IP sebagai
                    // pengganti nama). Fallback ke IP bila hello tidak ada.
                    let hello = self.pending_hello.as_ref();
                    let info = SourceInfo {
                        device_id: hello
                            .map(|h| h.device_id.clone())
                            .unwrap_or_else(|| ip.to_string()),
                        name: hello
                            .map(|h| h.name.clone())
                            .filter(|n| !n.trim().is_empty())
                            .unwrap_or_else(|| format!("HP {ip}")),
                        user_agent: hello.map(|h| h.user_agent.clone()).unwrap_or_default(),
                        transport: hello.map(|h| h.transport).unwrap_or_default(),
                        connection_kind: phonecam_core::source::ConnectionKind::Wifi,
                    };
                    let source = Source::new(source_id.clone(), info, token.as_str().to_string());
                    match ctx.sources.write().insert(source) {
                        Ok(()) => VerifyOutcome::Ok {
                            source_id,
                            token: token.as_str().to_string(),
                        },
                        Err(_) => VerifyOutcome::SessionFull,
                    }
                }
                Ok(PinCheck::Expired) => VerifyOutcome::Expired,
                Ok(PinCheck::Locked { .. }) => VerifyOutcome::BadPin,
                Ok(PinCheck::Wrong { .. }) => VerifyOutcome::BadPin,
                Err(_) => VerifyOutcome::BadPin,
            },
            AuthCredential::Token(tok) => {
                // B2/B3: token reconnect valid selama sesi desktop hidup.
                // Sumber tetap di registry saat WS putus (state RECONNECTING);
                // token tidak dikenal ditolak sebagai BAD_PIN (B2).
                let sources = ctx.sources.read();
                match sources.find_by_token(tok) {
                    Some(src) => VerifyOutcome::Ok {
                        source_id: src.id.clone(),
                        token: src.token.clone(),
                    },
                    None => VerifyOutcome::BadPin,
                }
            }
        }
    }

    async fn send_fail(
        &self,
        tx: &mut futures::stream::SplitSink<WebSocket, Message>,
        code: AuthFailCode,
        retry_after: Option<u32>,
    ) -> ControlFlow {
        let payload = AuthFailPayload { code, retry_after };
        if let Ok(text) = protocol::build(types::AUTH_FAIL, None, Some(&payload)) {
            let _ = tx.send(Message::Text(text.into())).await;
        }
        ControlFlow::Continue
    }

    async fn cleanup(&mut self, ctx: &Arc<AppState>, _ip: &str) {
        if self.state == SessionPhase::Closed {
            return;
        }
        self.state = SessionPhase::Closed;
        // B4: tutup peer connection (stop driver UDP/DTLS).
        if let Some(peer) = self.peer.take() {
            if let Err(e) = peer.close().await {
                tracing::debug!(?e, "error menutup peer connection");
            }
        }
        // B2/B3: sumber ditangani sesuai alasan putus.
        if let Some(id) = self.source_id.take() {
            if self.bye_received {
                // `bye` eksplisit:HP menutup sesi sendiri → CLOSED langsung.
                ctx.sources.write().remove(&id);
            } else {
                // WS putus tanpa `bye` (mis. Wi-Fi hilang) → RECONNECTING.
                // Source tetap di registry; token reconnect masih berlaku
                // (B2: selama sesi desktop hidup). Anggaran 60 detik dengan
                // backoff eksponensial (B3); bila habis → CLOSED + hapus.
                let st = ctx.sources.write().get_mut(&id).map(|s| {
                    s.status.state = phonecam_core::source::SessionStateDefault(
                        phonecam_core::session::SessionState::Reconnecting,
                    )
                });
                if st.is_none() {
                    // Sumber sudah tidak ada (dihapus pihak lain) — selesai.
                    return;
                }
                let ctx2 = ctx.clone();
                tokio::spawn(async move {
                    let start = std::time::Instant::now();
                    let deadline = start
                        + std::time::Duration::from_secs(
                            phonecam_core::constants::reconnect::TOTAL_BUDGET_SECS,
                        );
                    let mut attempt = 0u32;
                    // B3: backoff 0.5 s, 1 s, 2 s, 4 s, maks 8 s; total 60 s.
                    while std::time::Instant::now() < deadline {
                        let Some(wait_ms) = phonecam_core::session::backoff_delay(
                            attempt,
                            start.elapsed().as_millis() as u64,
                        ) else {
                            break;
                        };
                        tokio::time::sleep(std::time::Duration::from_millis(wait_ms)).await;
                        attempt += 1;
                        // B3: sumber keluar dari RECONNECTING bila HP berhasil
                        // reconnect lewat token (state Live) atau dihapus.
                        let still_reconnecting = {
                            let sources = ctx2.sources.read();
                            sources
                                .get(&id)
                                .map(|s| {
                                    s.status.state.0
                                        == phonecam_core::session::SessionState::Reconnecting
                                })
                                .unwrap_or(false)
                        };
                        if !still_reconnecting {
                            return;
                        }
                    }
                    // Anggaran habis: tutup sesi sumber (B3).
                    if ctx2.sources.write().remove(&id).is_some() {
                        tracing::info!(%id, "reconnect timeout; sumber ditutup (B3)");
                    }
                });
            }
        }
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

enum VerifyOutcome {
    Ok { source_id: String, token: String },
    BadPin,
    Expired,
    SessionFull,
}

/// Hasil dispatch pesan: lanjut atau putuskan koneksi.
#[must_use]
enum ControlFlow {
    Continue,
    Break,
}

impl ControlFlow {
    fn is_break(&self) -> bool {
        matches!(self, ControlFlow::Break)
    }
}
