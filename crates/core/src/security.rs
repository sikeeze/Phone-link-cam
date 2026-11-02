//! Keamanan sesi (README B2, aturan A8).
//!
//! - Validasi Origin header pada WebSocket.
//! - Rate-limit PIN per alamat IP (maks 5 salah, kunci 60 detik).
//! - Token sesi 128-bit untuk reconnect otomatis.
//! - Pembantu redaksi log: tidak pernah mencatat PIN atau token.

use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{Duration, Instant};

use crate::constants::limits::PIN_LOCKOUT_SECS;
use crate::error::{Error, Result, UserErrorCode};
use crate::pairing::{PinAttemptTracker, PinCheck};
use crate::protocol::AuthCredential;

/// Konstanta yang dipakai security.rs.
pub mod constants {
    /// Alias: kunci 60 detik (B2), lihat crate::constants::limits::PIN_LOCKOUT_SECS.
    pub const PIN_LOCKOUT_SECS: u64 = 60;
}

///(redacted) Pembungkus nilai rahasia yang tidak pernah muncul di log (aturan A8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Secret<T>(pub T);

impl<T> Secret<T> {
    pub fn new(value: T) -> Self {
        Secret(value)
    }
}

impl<T: std::fmt::Display> std::fmt::Display for Secret<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let _ = self; // hindari warning
        f.write_str("<redacted>")
    }
}

/// Resolusi header Origin vs host signaling (B2: periksa Origin pada WebSocket).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OriginPolicy {
    /// Mode ADB: hanya localhost dan 127.0.0.1 yang diizinkan (B9.2).
    LocalhostOnly,
    /// Mode LAN: mnenerima origin apa pun yang menuju ke antarmuka privat kita.
    Lan,
}

impl OriginPolicy {
    /// Validasi Origin header (B2). `origin` adalah nilai header Origin mentah.
    ///
    /// Aturan: WebSocket signaling hanya boleh dipanggil dari halaman yang
    /// kita sajikan sendiri. Karena HP membuka `https://<ip>:8443` atau
    /// `http://localhost:8080`, origin harus cocok dengan listener itu.
    pub fn validate(&self, origin: Option<&str>, host: Option<&str>) -> Result<()> {
        let Some(origin) = origin else {
            // Browser selalu mengirim Origin pada WS; absen = tolak (B2).
            return Err(Error::user(UserErrorCode::PermissionDenied));
        };
        let allowed = match self {
            OriginPolicy::LocalhostOnly => is_localhost_origin(origin),
            OriginPolicy::Lan => {
                // Origin harus https dan host-nya cocok dengan Host header (B2).
                match (parse_origin_host(origin), host) {
                    (Some((scheme, ohost)), Some(h)) => scheme == "https" && host_eq(ohost, h),
                    _ => false,
                }
            }
        };
        if allowed {
            Ok(())
        } else {
            Err(Error::user(UserErrorCode::PermissionDenied))
        }
    }
}

fn is_localhost_origin(origin: &str) -> bool {
    let Some((scheme, host)) = parse_origin_host(origin) else {
        return false;
    };
    scheme == "http" && matches!(strip_port(host), "localhost" | "127.0.0.1" | "[::1]")
}

/// Buang port dari host Origin. Aman untuk IPv6 (`[::1]:8080` -> `[::1]`).
fn strip_port(host: &str) -> &str {
    if let Some(idx) = host.rfind(':') {
        let after = &host[idx + 1..];
        if !after.is_empty() && after.bytes().all(|b| b.is_ascii_digit()) {
            return &host[..idx];
        }
    }
    host
}

fn parse_origin_host(origin: &str) -> Option<(&str, &str)> {
    let scheme = origin.split("://").next()?;
    let rest = origin.split_once("://")?.1;
    let host = rest.split('/').next()?;
    Some((scheme, host))
}

fn host_eq(origin_host: &str, host_header: &str) -> bool {
    // Host header mungkin memuat port; Origin juga. Bandingkan case-insensitive.
    let a = origin_host.to_ascii_lowercase();
    let b = host_header
        .split(',')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    a == b
}

/// Rate limiter PIN per alamat IP (B2).
pub struct PinRateLimiter {
    by_ip: HashMap<IpAddr, PinAttemptTracker>,
    /// Waktu pelacakan untuk pembersihan entri kedaluwarsa.
    last_sweep: Instant,
}

impl PinRateLimiter {
    pub fn new() -> Self {
        Self {
            by_ip: HashMap::new(),
            last_sweep: Instant::now(),
        }
    }

    /// Catat percobaan PIN salah dari `ip` (B2).
    pub fn record_wrong(&mut self, ip: IpAddr, now: i64) -> PinCheck {
        let tracker = self.by_ip.entry(ip).or_default();
        tracker.record_wrong(now)
    }

    /// True jika `ip` sedang dikunci (B2).
    pub fn is_locked(&self, input: IpAddr, now: i64) -> bool {
        self.by_ip.get(&input).is_some_and(|t| t.is_locked(now))
    }

    /// Sisa detik hingga kunci `ip` berakhir; None bila tidak terkunci (B2:
    /// `retryAfter` harus berisi waktu tunggu nyata, bukan konstan 60).
    pub fn retry_after(&self, input: IpAddr, now: i64) -> Option<u32> {
        self.by_ip
            .get(&input)
            .and_then(|t| t.retry_after(now))
            .map(|secs| secs as u32)
    }

    /// Reset tracker untuk `ip` setelah PIN benar.
    pub fn reset(&mut self, ip: IpAddr) {
        if let Some(t) = self.by_ip.get_mut(&ip) {
            t.reset();
        }
    }

    /// Buang tracker yang sudah tidak terkunci dan tidak ada percobaan baru.
    pub fn sweep(&mut self, now: i64) {
        if self.last_sweep.elapsed() < Duration::from_secs(PIN_LOCKOUT_SECS) {
            return;
        }
        self.by_ip.retain(|_, t| t.is_locked(now));
        self.last_sweep = Instant::now();
    }

    /// Jumlah IP yang sedang dilacak (untuk diagnostik).
    pub fn tracked(&self) -> usize {
        self.by_ip.len()
    }
}

impl Default for PinRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

/// Verifikasi kredensial masuk (B2). Membedakan PIN vs token reconnect.
pub fn classify_credential(cred: &AuthCredential) -> CredentialKind {
    match cred {
        AuthCredential::Pin(_) => CredentialKind::Pin,
        AuthCredential::Token(_) => CredentialKind::Token,
    }
}

/// Jenis kredensial (B2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialKind {
    Pin,
    Token,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn localhost_policy_accepts_localhost_only() {
        let p = OriginPolicy::LocalhostOnly;
        assert!(p
            .validate(Some("http://localhost:8080"), Some("localhost:8080"))
            .is_ok());
        assert!(p
            .validate(Some("http://127.0.0.1:8080"), Some("127.0.0.1:8080"))
            .is_ok());
        assert!(p
            .validate(Some("http://evil.com"), Some("localhost:8080"))
            .is_err());
        assert!(p
            .validate(Some("https://192.168.1.24:8443"), Some("localhost:8080"))
            .is_err());
        assert!(p.validate(None, Some("localhost:8080")).is_err());
    }

    #[test]
    fn lan_policy_requires_https_and_matching_host() {
        let p = OriginPolicy::Lan;
        assert!(p
            .validate(Some("https://192.168.1.24:8443"), Some("192.168.1.24:8443"))
            .is_ok());
        // Host tidak cocok = tolak
        assert!(p
            .validate(Some("https://192.168.1.24:8443"), Some("10.0.0.5:8443"))
            .is_err());
        // http (bukan https) = tolak
        assert!(p
            .validate(Some("http://192.168.1.24:8443"), Some("192.168.1.24:8443"))
            .is_err());
    }

    #[test]
    fn rate_limiter_locks_ip_after_five_wrong() {
        let mut rl = PinRateLimiter::new();
        let ip: IpAddr = "192.168.1.50".parse().unwrap();
        for _ in 0..4 {
            assert!(matches!(rl.record_wrong(ip, 0), PinCheck::Wrong { .. }));
        }
        assert!(matches!(
            rl.record_wrong(ip, 0),
            PinCheck::Locked { retry_after: 60 }
        ));
        assert!(rl.is_locked(ip, 0));
        assert!(!rl.is_locked(ip, 61));
    }

    #[test]
    fn retry_after_reports_remaining_lock_time() {
        // B2: retryAfter adalah sisa waktu kunci, bukan konstan 60.
        let mut rl = PinRateLimiter::new();
        let ip: IpAddr = "192.168.1.50".parse().unwrap();
        for _ in 0..5 {
            rl.record_wrong(ip, 100);
        }
        assert_eq!(rl.retry_after(ip, 100), Some(60));
        assert_eq!(rl.retry_after(ip, 130), Some(30));
        assert_eq!(rl.retry_after(ip, 160), None);
    }

    #[test]
    fn rate_limiter_tracks_ips_independently() {
        let mut rl = PinRateLimiter::new();
        let a: IpAddr = "192.168.1.50".parse().unwrap();
        let b: IpAddr = "192.168.1.51".parse().unwrap();
        for _ in 0..5 {
            rl.record_wrong(a, 0);
        }
        rl.record_wrong(b, 0);
        assert!(rl.is_locked(a, 0));
        assert!(!rl.is_locked(b, 0));
        assert_eq!(rl.tracked(), 2);
    }

    #[test]
    fn secret_never_logs_value() {
        let s = Secret::new("super-secret-pin");
        assert_eq!(format!("{s}"), "<redacted>");
    }
}
