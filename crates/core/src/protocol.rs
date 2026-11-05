//! Pesan signaling JSON via WebSocket (README B4).
//! Semua pesan berbentuk `{ v: 1, type, id, ts, payload }`.
//! Pesan tidak dikenal atau di luar skema diabaikan dan dicatat pada level debug.

use std::collections::BTreeMap;

use crate::constants::limits::MAX_MESSAGE_BYTES;
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};

/// Versi protokol (B4: `v: 1`).
pub const PROTOCOL_VERSION: u32 = 1;

/// Arah pesan relatif terhadap desktop (PC).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    /// HP > PC
    FromPhone,
    /// PC > HP
    ToPhone,
    /// dua arah
    Both,
}

/// Satu pesan signaling. `type` diberi nama `type` lewat serde rename.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireMessage<P = serde_json::Value> {
    #[serde(default = "default_version")]
    pub v: u32,
    #[serde(rename = "type")]
    pub msg_type: String,
    /// ID unik per pesan (boleh kosong untuk pesan tanpa balasan).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Timestamp UNIX ms (diisi oleh pengirim).
    #[serde(default)]
    pub ts: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<P>,
}

fn default_version() -> u32 {
    PROTOCOL_VERSION
}

impl<P> WireMessage<P> {
    pub fn new(msg_type: impl Into<String>) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            msg_type: msg_type.into(),
            id: None,
            ts: 0,
            payload: None,
        }
    }

    pub fn with_payload(mut self, payload: P) -> Self {
        self.payload = Some(payload);
        self
    }

    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }
}

/// Nama-nama tipe pesan (B4). String literal stabil, jangan diubah tanpa ADR.
pub mod types {
    // HP > PC
    pub const HELLO: &str = "hello";
    pub const AUTH: &str = "auth";
    pub const SOURCE_UPDATE: &str = "source_update";
    // PC > HP
    pub const AUTH_OK: &str = "auth_ok";
    pub const AUTH_FAIL: &str = "auth_fail";
    pub const CMD_QUALITY: &str = "cmd_quality";
    pub const CMD_KEYFRAME: &str = "cmd_keyframe";
    pub const KEYFRAME_READY: &str = "keyframe_ready";
    pub const CMD_CAMERA: &str = "cmd_camera";
    pub const CMD_ROLE: &str = "cmd_role";
    // Dua arah
    pub const OFFER: &str = "offer";
    pub const ANSWER: &str = "answer";
    pub const ICE: &str = "ice";
    pub const PING: &str = "ping";
    pub const PONG: &str = "pong";
    pub const BYE: &str = "bye";
}

/// Kemampuan yang dilaporkan HP lewat `hello` (B4).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    #[serde(default)]
    pub zoom: bool,
    #[serde(default)]
    pub torch: bool,
    #[serde(default)]
    pub camera_switch: bool,
    #[serde(default)]
    pub webcodecs: bool,
}

/// Jalur transport media (B4 `transport`, B9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Transport {
    /// WebRTC + DTLS-SRTP lewat UDP (Wi-Fi, tethering, hotspot).
    #[default]
    #[serde(rename = "webrtc")]
    WebRtc,
    /// WebSocket + WebCodecs H.264 (mode ADB; adb reverse hanya TCP).
    #[serde(rename = "ws-webcodecs")]
    WsWebCodecs,
}

impl Transport {
    pub fn as_str(&self) -> &'static str {
        match self {
            Transport::WebRtc => "webrtc",
            Transport::WsWebCodecs => "ws-webcodecs",
        }
    }
}

/// Payload `hello` (HP > PC).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloPayload {
    pub device_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub user_agent: String,
    #[serde(default)]
    pub capabilities: Capabilities,
    #[serde(default)]
    pub transport: Transport,
}

/// Kredensial yang dikirim HP lewat `auth` (B2): PIN 6 digit atau token sesi 128-bit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthCredential {
    Pin(String),
    Token(String),
}

impl AuthCredential {
    /// Untuk log/telemetri: tidak pernah membocorkan nilai (aturan A8).
    pub fn redact(&self) -> &'static str {
        match self {
            AuthCredential::Pin(_) => "pin:<redacted>",
            AuthCredential::Token(_) => "token:<redacted>",
        }
    }
}

/// Payload `auth` (HP > PC).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthPayload {
    pub credential: AuthCredential,
}

/// Payload `auth_ok` (PC > HP, B2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthOkPayload {
    pub source_id: String,
    pub token: String,
    /// Preset kualitas awal (B5).
    pub quality: QualityPreset,
    /// B9.1: pindah otomatis ke jalur berikutnya saat putus diizinkan.
    #[serde(default)]
    pub auto_failover: bool,
    /// B9.1: URL jalur alternatif terurut per prioritas (kabel dulu).
    /// HP memakainya bila `auto_failover` aktif dan anggaran reconnect
    /// B3 di jalur sekarang sudah habis.
    #[serde(default)]
    pub alt_paths: Vec<String>,
}

/// Kode kegagalan auth (B4). String stabil.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AuthFailCode {
    BadPin,
    Locked,
    Expired,
    SessionFull,
}

impl AuthFailCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            AuthFailCode::BadPin => "BAD_PIN",
            AuthFailCode::Locked => "LOCKED",
            AuthFailCode::Expired => "EXPIRED",
            AuthFailCode::SessionFull => "SESSION_FULL",
        }
    }
}

/// Payload `auth_fail` (PC > HP).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthFailPayload {
    pub code: AuthFailCode,
    /// Detik hingga percobaan berikutnya diizinkan (hanya untuk LOCKED).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after: Option<u32>,
}

/// Payload `offer`/`answer` (dua arah, hanya transport webrtc).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SdpPayload {
    pub sdp: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
}

/// Payload `ice` (dua arah, hanya transport webrtc).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IcePayload {
    pub candidate: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sdp_mid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sdp_mline_index: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
}

/// Payload `source_update` (B4).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SourceUpdatePayload {
    pub source_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fps: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub facing: Option<CameraFacing>,
    /// 0-100
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub battery: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub torch: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoom: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Arah kamera (B4, `facing`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CameraFacing {
    #[default]
    Front,
    Back,
}

/// Preset kualitas (B5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum QualityPreset {
    /// 640x480 30 1 Mbps
    Saving,
    /// 1280x720 30 2,5 Mbps (default)
    #[default]
    Balanced,
    /// 1920x1080 30 5 Mbps
    High,
    /// 1280x720 60 4 Mbps
    Smooth,
    /// 320x180 15 250 kbps (HP non-aktif)
    Preview,
}

impl QualityPreset {
    pub fn params(self) -> (u32, u32, u32, u32) {
        // (width, height, fps, bitrate_bps)
        match self {
            QualityPreset::Saving => (640, 480, 30, 1_000_000),
            QualityPreset::Balanced => (1280, 720, 30, 2_500_000),
            QualityPreset::High => (1920, 1080, 30, 5_000_000),
            QualityPreset::Smooth => (1280, 720, 60, 4_000_000),
            QualityPreset::Preview => (320, 180, 15, 250_000),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            QualityPreset::Saving => "Hemat",
            QualityPreset::Balanced => "Seimbang",
            QualityPreset::High => "Tinggi",
            QualityPreset::Smooth => "Halus",
            QualityPreset::Preview => "Preview",
        }
    }
}

/// Payload `cmd_quality` (PC > HP).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CmdQualityPayload {
    pub quality: QualityPreset,
}

/// Payload `cmd_camera` (PC > HP) — kontrol jarak jauh (B4, fase 3).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CmdCameraPayload {
    pub source_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub facing: Option<CameraFacing>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub torch: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoom: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<FocusExposure>,
}

/// Pengaturan fokus/eksposur (B4, fase 3).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FocusExposure {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub point_x: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub point_y: Option<f32>,
}

/// Payload `cmd_role` (PC > HP) — `active` atau `preview` (B4, fase 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SourceRole {
    #[default]
    Active,
    Preview,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CmdRolePayload {
    pub source_id: String,
    pub role: SourceRole,
}

/// Payload `cmd_keyframe` (PC > HP, B6): minta frame kunci berikutnya
/// sebelum penggantian sumber aktif agar tidak ada frame beku/glitch.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CmdKeyframePayload {
    pub source_id: String,
}

/// Payload `keyframe_ready` (HP > PC, B6): frame kunci sudah dikirim;
/// `ts` envelope dipakai PC untuk menghitung waktu tunggu.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyframeReadyPayload {
    pub source_id: String,
}

/// Payload `bye` (dua arah).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ByePayload {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Peta payload mentah: pesan tidak dikenal tetap bisa dilihat isinya pada level debug.
pub type RawPayload = BTreeMap<String, serde_json::Value>;

/// Hasil decode pesan masuk: tipe + payload mentah (B4).
#[derive(Debug, Clone)]
pub struct DecodedMessage {
    /// Nilai field `type`.
    pub msg_type: String,
    /// Timestamp UNIX ms dari envelope (B4; dipakai pengukuran RTT, B5).
    pub ts: u64,
    /// Nilai field `payload` bila ada.
    pub payload: Option<RawPayload>,
}

/// Decode pesan mentah (B4). Memvalidasi versi protokol dan ukuran.
/// Payload di-verify lewat `payload_from_value` oleh pemakai.
pub fn parse_message(text: &str) -> Result<DecodedMessage> {
    if text.len() > MAX_MESSAGE_BYTES {
        return Err(Error::MessageTooLarge {
            limit: MAX_MESSAGE_BYTES,
        });
    }
    let msg: WireMessage<RawPayload> =
        serde_json::from_str(text).map_err(|e| Error::protocol(e.to_string()))?;
    if msg.v != PROTOCOL_VERSION {
        return Err(Error::protocol(format!(
            "versi protokol tidak didukung: {}",
            msg.v
        )));
    }
    Ok(DecodedMessage {
        msg_type: msg.msg_type,
        ts: msg.ts,
        payload: msg.payload,
    })
}

/// Buat pesan siap kirim dengan payload terserialize.
pub fn build<T: Serialize>(
    msg_type: &str,
    id: Option<&str>,
    payload: Option<&T>,
) -> Result<String> {
    build_at(msg_type, id, payload, now_ms())
}

/// Timestamp UNIX ms (B4: diisi oleh pengirim).
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Varian `build` dengan timestamp eksplisit (untuk pengujian).
pub fn build_at<T: Serialize>(
    msg_type: &str,
    id: Option<&str>,
    payload: Option<&T>,
    ts: u64,
) -> Result<String> {
    let payload_json = match payload {
        Some(p) => Some(serde_json::to_value(p).map_err(|e| Error::protocol(e.to_string()))?),
        None => None,
    };
    let msg = WireMessage {
        v: PROTOCOL_VERSION,
        msg_type: msg_type.to_string(),
        id: id.map(str::to_string),
        ts,
        payload: payload_json,
    };
    let text = serde_json::to_string(&msg).map_err(|e| Error::protocol(e.to_string()))?;
    if text.len() > MAX_MESSAGE_BYTES {
        return Err(Error::MessageTooLarge {
            limit: MAX_MESSAGE_BYTES,
        });
    }
    Ok(text)
}

/// Decode payload dari serde_json::Value.
pub fn payload_from_value<T: for<'de> Deserialize<'de>>(value: &serde_json::Value) -> Result<T> {
    serde_json::from_value(value.clone())
        .map_err(|e| Error::protocol(format!("payload tidak valid: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_hello() {
        let hello = HelloPayload {
            device_id: "abc123".into(),
            name: "Pixel 8".into(),
            user_agent: "Mozilla/5.0".into(),
            capabilities: Capabilities {
                zoom: true,
                torch: true,
                camera_switch: true,
                webcodecs: false,
            },
            transport: Transport::WebRtc,
        };
        let text = build(types::HELLO, None, Some(&hello)).unwrap();
        let m = parse_message(&text).unwrap();
        assert_eq!(m.msg_type, types::HELLO);
        let p = m.payload.expect("payload ada");
        assert_eq!(p["device_id"], "abc123");
        assert_eq!(p["transport"], "webrtc");
        assert_eq!(p["capabilities"]["zoom"], true);
    }

    #[test]
    fn roundtrip_auth_fail() {
        let fail = AuthFailPayload {
            code: AuthFailCode::Locked,
            retry_after: Some(60),
        };
        let text = build(types::AUTH_FAIL, None, Some(&fail)).unwrap();
        let m = parse_message(&text).unwrap();
        assert_eq!(m.msg_type, types::AUTH_FAIL);
        let p = m.payload.expect("payload ada");
        assert_eq!(p["code"], "LOCKED");
        assert_eq!(p["retry_after"], 60);
        let decoded: AuthFailPayload =
            payload_from_value(&serde_json::to_value(&p).unwrap()).unwrap();
        assert_eq!(decoded.code, AuthFailCode::Locked);
        assert_eq!(decoded.retry_after, Some(60));
    }

    #[test]
    fn unknown_message_is_rejected() {
        // Pesan tidak dikenal harus bisa di-parse strukturnya, tetapi pemakai
        // crate ini yang memutuskan mengabaikannya (B4: diabaikan + log debug).
        let text = r#"{"v":1,"type":"future_msg","ts":123}"#;
        let m = parse_message(text).unwrap();
        assert_eq!(m.msg_type, "future_msg");
        assert!(m.payload.is_none());
    }

    #[test]
    fn wrong_version_rejected() {
        let text = r#"{"v":2,"type":"hello","ts":1}"#;
        assert!(parse_message(text).is_err());
    }

    #[test]
    fn message_over_limit_rejected() {
        let big = "x".repeat(MAX_MESSAGE_BYTES + 1);
        let text = format!(r#"{{"v":1,"type":"hello","ts":1,"payload":{{"x":"{big}"}}}}"#);
        let err = parse_message(&text).unwrap_err();
        assert!(matches!(err, Error::MessageTooLarge { .. }));
    }

    #[test]
    fn credential_redaction() {
        assert_eq!(
            AuthCredential::Pin("123456".into()).redact(),
            "pin:<redacted>"
        );
        assert_eq!(
            AuthCredential::Token("abcd".into()).redact(),
            "token:<redacted>"
        );
    }

    #[test]
    fn build_fills_sender_timestamp_b4() {
        // B4: `ts` diisi oleh pengirim, bukan 0.
        let text = build::<()>(types::PING, None, None).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let ts = v["ts"].as_u64().expect("ts harus angka ms");
        assert!(ts > 0, "ts harus terisi epoch ms, dapat {ts}");
    }

    #[test]
    fn build_at_sets_explicit_timestamp() {
        let text = build_at::<()>(types::PONG, None, None, 123).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["ts"].as_u64(), Some(123));
    }

    #[test]
    fn quality_preset_params_match_b5() {
        assert_eq!(QualityPreset::Balanced.params(), (1280, 720, 30, 2_500_000));
        assert_eq!(QualityPreset::Preview.params(), (320, 180, 15, 250_000));
        assert_eq!(QualityPreset::High.params(), (1920, 1080, 30, 5_000_000));
        assert_eq!(QualityPreset::Smooth.params(), (1280, 720, 60, 4_000_000));
        assert_eq!(QualityPreset::Saving.params(), (640, 480, 30, 1_000_000));
    }
}
