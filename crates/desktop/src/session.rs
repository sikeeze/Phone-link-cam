//! Sesi WebSocket signaling per koneksi HP (README B2/B3/B4).

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket};
use futures::{SinkExt, StreamExt};
use tokio::time::interval;

use phonecam_core::constants::limits::MAX_MESSAGE_BYTES;
use phonecam_core::pairing::PinCheck;
use phonecam_core::protocol::{
    self, types, AuthCredential, AuthFailCode, AuthFailPayload, HelloPayload, IcePayload,
    SdpPayload,
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
                            tokio::spawn(async move {
                                let kind = track.kind().await;
                                match kind {
                                    RtpCodecKind::Video => Self::spawn_video(ctx2, track).await,
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
    async fn spawn_video(ctx: Arc<AppState>, track: Arc<dyn TrackRemote>) {
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
        if let Err(e) = crate::pipeline::run(track, decoder, vcam, cancel_rx).await {
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
        let _ = ctx;
        match dec.msg_type.as_str() {
            types::PING => {
                // B4: ping > pong otomatis.
                if let Ok(text) = protocol::build::<()>(types::PONG, None, None) {
                    let _ = tx.send(Message::Text(text.into())).await;
                }
                ControlFlow::Continue
            }
            types::BYE => {
                tracing::info!(ip, "HP memutuskan (bye)");
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
            AuthCredential::Token(_) => {
                // ponytail: token reconnect diaktifkan saat UI desktop menyimpan
                // token per sumber (fase 2). Saat ini token tidak diterima.
                VerifyOutcome::BadPin
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
        // B2: hapus sumber dari registry saat WS tertutup (lewat source_id).
        // Tanpa ini, registry akan terus mengisi slot 6 dan sesi menjadi
        // permanen penuh setelah 6 koneksi.
        if let Some(id) = self.source_id.take() {
            ctx.sources.write().remove(&id);
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
