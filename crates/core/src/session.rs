//! State machine sesi per sumber (README B3).
//!
//! `IDLE > AUTHENTICATING > CONNECTING > LIVE > DEGRADED > RECONNECTING > CLOSED`
//!
//! Transisi harus diuji unit. UI desktop dan web HP hanya menampilkan state,
//! tidak menyimpan state sendiri.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};

/// State sesi per sumber (B3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SessionState {
    Idle,
    Authenticating,
    Connecting,
    Live,
    /// Bitrate/fps turun (B5).
    Degraded,
    /// Retry eksponensial (B3).
    Reconnecting,
    Closed,
}

impl SessionState {
    pub fn as_str(&self) -> &'static str {
        match self {
            SessionState::Idle => "IDLE",
            SessionState::Authenticating => "AUTHENTICATING",
            SessionState::Connecting => "CONNECTING",
            SessionState::Live => "LIVE",
            SessionState::Degraded => "DEGRADED",
            SessionState::Reconnecting => "RECONNECTING",
            SessionState::Closed => "CLOSED",
        }
    }

    /// Label ramah pengguna (UI desktop/PWA hanya menampilkan, B3).
    pub fn label_id(&self) -> &'static str {
        match self {
            SessionState::Idle => "Menganggur",
            SessionState::Authenticating => "Mengautentikasi",
            SessionState::Connecting => "Menghubungkan",
            SessionState::Live => "Langsung",
            SessionState::Degraded => "Menurun",
            SessionState::Reconnecting => "Menghubungkan ulang",
            SessionState::Closed => "Tertutup",
        }
    }

    /// True jika state ini masih dalam sesi aktif (bukan terminal).
    pub fn is_open(&self) -> bool {
        !matches!(self, SessionState::Closed)
    }
}

impl std::fmt::Display for SessionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Transisi yang diizinkan oleh state machine (B3).
///
/// IDLE > AUTHENTICATING > CONNECTING > LIVE > DEGRADED > RECONNECTING > CLOSED
/// DEGRADED boleh balik ke LIVE; RECONNECTING boleh kembali ke CONNECTING
/// (retry) atau ke CLOSED setelah anggaran 60 detik habis (B3).
pub fn transition(from: SessionState, to: SessionState) -> Result<SessionState> {
    use SessionState::*;
    let allowed = matches!(
        (from, to),
        (Idle, Authenticating)
            | (Authenticating, Connecting)
            | (Authenticating, Closed)
            | (Connecting, Live)
            | (Connecting, Reconnecting)
            | (Connecting, Closed)
            | (Live, Degraded)
            | (Live, Reconnecting)
            | (Live, Closed)
            | (Degraded, Live)
            | (Degraded, Reconnecting)
            | (Degraded, Closed)
            | (Reconnecting, Connecting)
            | (Reconnecting, Closed)
            | (Closed, Idle)
            | (Idle, Closed)
    );
    if !allowed {
        return Err(Error::protocol(format!(
            "transisi state tidak valid: {from} -> {to}"
        )));
    }
    Ok(to)
}

/// Backoff retry eksponensial (B3): 0.5 s, 1 s, 2 s, 4 s, maks 8 s, total 60 s.
/// `attempt` dimulai dari 0. Mengembalikan durasi tunggu untuk percobaan ini,
/// atau None jika anggaran total sudah habis.
pub fn backoff_delay(attempt: u32, elapsed_ms: u64) -> Option<u64> {
    if elapsed_ms >= crate::constants::reconnect::TOTAL_BUDGET_SECS * 1000 {
        return None;
    }
    let steps = crate::constants::reconnect::BACKOFF_STEPS_MS;
    let idx = (attempt as usize).min(steps.len().saturating_sub(1));
    let base = steps[idx];
    Some(base.min(crate::constants::reconnect::BACKOFF_MAX_MS))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allowed(f: SessionState, t: SessionState) -> bool {
        transition(f, t).is_ok()
    }

    #[test]
    fn happy_path() {
        use SessionState::*;
        assert!(allowed(Idle, Authenticating));
        assert!(allowed(Authenticating, Connecting));
        assert!(allowed(Connecting, Live));
        assert!(allowed(Live, Closed));
    }

    #[test]
    fn degraded_is_recoverable() {
        use SessionState::*;
        assert!(allowed(Live, Degraded));
        assert!(allowed(Degraded, Live));
        assert!(allowed(Degraded, Reconnecting));
        assert!(allowed(Reconnecting, Connecting));
    }

    #[test]
    fn invalid_transitions_rejected() {
        use SessionState::*;
        assert!(!allowed(Idle, Live));
        assert!(!allowed(Authenticating, Live));
        assert!(!allowed(Live, Authenticating));
        assert!(!allowed(Closed, Live));
        assert!(!allowed(Reconnecting, Live));
    }

    #[test]
    fn any_state_may_close() {
        use SessionState::*;
        for s in [
            Idle,
            Authenticating,
            Connecting,
            Live,
            Degraded,
            Reconnecting,
        ] {
            assert!(allowed(s, Closed), "{s} harus bisa ke CLOSED");
        }
        assert!(!allowed(Closed, Closed));
    }

    #[test]
    fn backoff_matches_b3() {
        assert_eq!(backoff_delay(0, 0), Some(500));
        assert_eq!(backoff_delay(1, 0), Some(1000));
        assert_eq!(backoff_delay(2, 0), Some(2000));
        assert_eq!(backoff_delay(3, 0), Some(4000));
        assert_eq!(backoff_delay(4, 0), Some(8000));
        assert_eq!(backoff_delay(9, 0), Some(8000));
        assert_eq!(backoff_delay(0, 60_000), None);
        assert_eq!(backoff_delay(3, 59_999), Some(4000));
    }

    #[test]
    fn labels_present() {
        for s in [
            SessionState::Idle,
            SessionState::Authenticating,
            SessionState::Connecting,
            SessionState::Live,
            SessionState::Degraded,
            SessionState::Reconnecting,
            SessionState::Closed,
        ] {
            assert!(!s.label_id().is_empty());
            assert_eq!(s.as_str(), s.as_str().to_uppercase());
        }
    }
}
