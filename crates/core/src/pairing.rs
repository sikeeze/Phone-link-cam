//! Pairing: PIN 6 digit, token sesi, QR (README B2).
//!
//! - PIN 6 digit acak, berlaku 5 menit atau sampai dipakai satu kali.
//! - Bisa di-regenerate dari UI.
//! - Setelah PIN benar, server mengeluarkan token acak 128-bit untuk reconnect.
//! - QR berisi URL `https://<ip>:<port>/?pin=<PIN>`.

use std::time::Duration;

use crate::constants::{limits::MAX_PIN_ATTEMPTS, timing::PIN_TTL_SECS};
use crate::error::{Error, Result, UserErrorCode};
use rand::random_range;
use serde::{Deserialize, Serialize};

/// PIN 6 digit (B2). Disimpan sebagai string agar awalan nol terjaga.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pin(String);

impl Pin {
    /// Buat PIN acak 6 digit (B2).
    pub fn generate() -> Self {
        // 0..=999999 lalu pad ke 6 digit: setiap digitUniform.
        let n: u32 = random_range(0..=999_999);
        Self(format!("{n:06}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Bandingkan konstan-waktu agar tidak bocor lewat timing (aturan A8).
    /// Panjang string dibandingkan dulu; kita selalu membandingkan string 6 digit.
    pub fn verify(&self, candidate: &str) -> bool {
        constant_time_eq(self.0.as_bytes(), candidate.as_bytes())
    }
}

impl std::fmt::Display for Pin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Jangan pernah menulis PIN ke log; Display hanya untuk UI.
        f.write_str(self.0.as_str())
    }
}

impl Serialize for Pin {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.0.as_str())
    }
}

impl<'de> Deserialize<'de> for Pin {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        if s.len() != 6 || !s.bytes().all(|b| b.is_ascii_digit()) {
            return Err(serde::de::Error::custom("PIN harus 6 digit"));
        }
        Ok(Pin(s))
    }
}

/// Token sesi 128-bit (B2), heksadesimal 32 karakter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionToken(String);

impl SessionToken {
    /// Buat token acak 128-bit dari 16 byte acak (B2).
    pub fn generate() -> Self {
        let bytes: [u8; 16] = rand::random();
        Self(hex::encode(bytes))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SessionToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Token tidak boleh muncul di log (aturan A8); Display hanya untuk UI/protokol.
        f.write_str(self.0.as_str())
    }
}

/// Bandingan konstan-waktu untuk byte slice (mencegah timing attack pada PIN).
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// PASangan circular: impl Serialize/Deserialize untuk SessionToken.
impl Serialize for SessionToken {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.0.as_str())
    }
}

impl<'de> Deserialize<'de> for SessionToken {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        if s.len() != 32 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(serde::de::Error::custom(
                "token harus 128-bit heksa (32 karakter)",
            ));
        }
        Ok(SessionToken(s))
    }
}

/// Status PIN saat ini beserta hitung mundur (B2).
#[derive(Debug, Clone)]
pub struct PinState {
    pub pin: Pin,
    /// UNIX epoch detik saat PIN dibuat.
    pub created_at: i64,
    /// Berapa kali PIN ini sudah dipakai (B2: sekali pakai).
    pub used: u32,
}

impl PinState {
    /// Buat PIN baru dengan timestamp sekarang.
    pub fn new(now: i64) -> Self {
        Self {
            pin: Pin::generate(),
            created_at: now,
            used: 0,
        }
    }

    /// Detik tersisa sebelum kedaluwarsa (B2: 5 menit). Tidak negatif.
    pub fn remaining(&self, now: i64) -> i64 {
        (self.created_at + PIN_TTL_SECS - now).max(0)
    }

    /// True jika PIN sudah lewat 5 menit (B2).
    pub fn is_expired(&self, now: i64) -> bool {
        now >= self.created_at + PIN_TTL_SECS
    }

    /// True jika PIN sudah dipakai satu kali (B2: berlaku sampai dipakai satu kali).
    pub fn is_consumed(&self) -> bool {
        self.used > 0
    }
}

/// Hasil verifikasi PIN (B2 + B2 rate limiting).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PinCheck {
    /// PIN benar; PIN ditandai consumed.
    Ok,
    /// PIN salah; `attempts_left` turun. Setelah habis, kunci 60 detik.
    Wrong { attempts_left: u32 },
    /// Terlalu banyak percobaan; `retry_after` detik (B2: 60).
    Locked { retry_after: u32 },
    /// PIN sudah kedaluwarsa (5 menit) atau sudah dipakai.
    Expired,
}

/// Pelacak percobaan PIN per alamat IP (B2: maksimal 5 percobaan salah per IP,
/// lalu kunci 60 detik).
#[derive(Debug, Clone, Default)]
pub struct PinAttemptTracker {
    attempts: u32,
    /// UNIX epoch detik kapan penguncian dimulai.
    locked_at: Option<i64>,
}

impl PinAttemptTracker {
    /// Catat percobaan salah; kembalikan sisa percobaan atau kunci.
    pub fn record_wrong(&mut self, now: i64) -> PinCheck {
        if let Some(unlock_at) = self.lock_out_at(now) {
            return PinCheck::Locked {
                retry_after: unlock_at.saturating_sub(now) as u32,
            };
        }
        self.attempts += 1;
        if self.attempts >= MAX_PIN_ATTEMPTS {
            self.locked_at = Some(now);
            return PinCheck::Locked {
                retry_after: crate::constants::limits::PIN_LOCKOUT_SECS as u32,
            };
        }
        PinCheck::Wrong {
            attempts_left: MAX_PIN_ATTEMPTS - self.attempts,
        }
    }

    /// True jika sedang dikunci dan belum lewat 60 detik.
    pub fn is_locked(&self, now: i64) -> bool {
        self.lock_out_at(now).is_some()
    }

    /// Sisa detik hingga kunci berakhir; None bila tidak terkunci (B2).
    pub fn retry_after(&self, now: i64) -> Option<i64> {
        let unlock = self.lock_out_at(now)?;
        Some(unlock.saturating_sub(now))
    }

    /// UNIX epoch detik kapan kunci berakhir; None jika tidak terkunci.
    fn lock_out_at(&self, now: i64) -> Option<i64> {
        let locked_at = self.locked_at?;
        let unlock = locked_at + crate::constants::limits::PIN_LOCKOUT_SECS as i64;
        if now < unlock {
            Some(unlock)
        } else {
            None
        }
    }

    /// Reset setelah berhasil atau setelah kunci berakhir.
    pub fn reset(&mut self) {
        self.attempts = 0;
        self.locked_at = None;
    }
}

/// Manajer PIN tunggal untuk sesi desktop (B2).
pub struct PairingManager {
    state: parking_lot::Mutex<PinState>,
}

impl PairingManager {
    pub fn new(now: i64) -> Self {
        Self {
            state: parking_lot::Mutex::new(PinState::new(now)),
        }
    }

    /// PIN aktif saat ini (untuk QR/UI). Baca saja.
    pub fn current(&self) -> PinState {
        self.state.lock().clone()
    }

    /// Buat PIN baru dari UI (B2: bisa di-regenerate).
    pub fn regenerate(&self, now: i64) -> PinState {
        let mut st = self.state.lock();
        *st = PinState::new(now);
        st.clone()
    }

    /// Verifikasi PIN yang dikirim HP (B2). Mengonsumsi PIN jika benar.
    pub fn verify(&self, candidate: &str, now: i64) -> Result<PinCheck> {
        let mut st = self.state.lock();
        if st.is_expired(now) || st.is_consumed() {
            return Ok(PinCheck::Expired);
        }
        if st.pin.verify(candidate) {
            st.used += 1;
            Ok(PinCheck::Ok)
        } else {
            // Catatan: rate limiting per IP dilakukan di security.rs (B2).
            Err(Error::user(UserErrorCode::BadPin))
        }
    }
}

/// Bangun URL pairing (B2: `https://<ip>:<port>/?pin=<PIN>`).
pub fn pairing_url(ip: &str, port: u16, pin: &Pin) -> String {
    format!("https://{ip}:{port}/?pin={pin}")
}

/// Durasi TTL PIN sebagai Duration (B2: 5 menit).
pub fn pin_ttl() -> Duration {
    Duration::from_secs(PIN_TTL_SECS as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pin(s: &str) -> Pin {
        Pin(s.to_string())
    }

    #[test]
    fn pin_format() {
        let p = Pin::generate();
        assert_eq!(p.as_str().len(), 6);
        assert!(p.as_str().bytes().all(|b| b.is_ascii_digit()));
    }

    #[test]
    fn two_pins_differ() {
        let a = Pin::generate();
        let b = Pin::generate();
        assert_ne!(a, b);
    }

    #[test]
    fn verify_constant_time() {
        let p = pin("123456");
        assert!(p.verify("123456"));
        assert!(!p.verify("123457"));
        assert!(!p.verify("12345"));
        assert!(!p.verify("1234567"));
    }

    #[test]
    fn token_is_128_bit_hex() {
        let t = SessionToken::generate();
        assert_eq!(t.as_str().len(), 32);
        assert!(t.as_str().bytes().all(|b| b.is_ascii_hexdigit()));
    }

    #[test]
    fn pin_expiry() {
        let st = PinState::new(1000);
        assert!(!st.is_expired(1000 + 299));
        assert!(st.is_expired(1000 + 300));
        assert_eq!(st.remaining(1000 + 100), 200);
        assert_eq!(st.remaining(1000 + 400), 0);
    }

    #[test]
    fn pin_single_use() {
        let st = PinState::new(0);
        assert!(!st.is_consumed());
        let mut st2 = st;
        st2.used += 1;
        assert!(st2.is_consumed());
    }

    #[test]
    fn tracker_locks_after_five_wrong() {
        let mut t = PinAttemptTracker::default();
        for i in 0..4 {
            match t.record_wrong(0) {
                PinCheck::Wrong { attempts_left } => assert_eq!(attempts_left, 4 - i),
                _ => panic!("harusnya Wrong"),
            }
            assert!(!t.is_locked(0));
        }
        match t.record_wrong(0) {
            PinCheck::Locked { retry_after } => assert_eq!(retry_after, 60),
            _ => panic!("harusnya Locked"),
        }
        assert!(t.is_locked(0));
        assert!(!t.is_locked(61));
    }

    #[test]
    fn tracker_resets() {
        let mut t = PinAttemptTracker::default();
        t.record_wrong(0);
        t.record_wrong(0);
        t.reset();
        assert_eq!(t.attempts, 0);
        assert!(!t.is_locked(0));
    }

    #[test]
    fn pairing_url_format() {
        let p = pin("482916");
        let url = pairing_url("192.168.1.24", 8443, &p);
        assert_eq!(url, "https://192.168.1.24:8443/?pin=482916");
    }
}
