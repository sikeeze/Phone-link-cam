// Evaluator kualitas adaptif (README B5).
//
// Aturan B5: turunkan satu tingkat jika packet loss lebih dari 5% atau RTT
// lebih dari 80 ms selama 3 detik; naikkan jika stabil 10 detik. Presenter
// (desktop) memakai ini untuk memutuskan kapan mengirim `cmd_quality`.
// Logika murni agar bisa diuji unit tanpa jaringan (A3).

use crate::protocol::QualityPreset;

/// Ambang B5.
pub mod thresholds {
    /// Loss di atas ini memicu penurunan (B5: 5%).
    pub const LOSS_PCT: f64 = 5.0;
    /// RTT di atas ini memicu penurunan (B5: 80 ms).
    pub const RTT_MS: u32 = 80;
    /// Durasi kondisi buruk sebelum turun (B5: 3 detik).
    pub const BAD_SECS: u64 = 3;
    /// Durasi stabil sebelum naik (B5: 10 detik).
    pub const GOOD_SECS: u64 = 10;
}

impl QualityState {
    /// Buat status dengan preset awal (B5: preset kualitas awal dari auth_ok).
    pub fn new(current: QualityPreset) -> Self {
        Self {
            current,
            ..Default::default()
        }
    }

    /// Preset saat ini (B5).
    pub fn current(&self) -> QualityPreset {
        self.current
    }
}

/// Tambahkan `rtt_ms` terbaru; kembalikan preset baru bila berubah (B5).
pub fn sample_rtt(state: &mut QualityState, rtt_ms: u32, now_ms: u64) -> Option<QualityPreset> {
    state.last_rtt_ms = Some(rtt_ms);
    evaluate(state, now_ms)
}

/// Catat urutan RTP; gap sequence dihitung sebagai loss (B5).
pub fn sample_seq(state: &mut QualityState, seq: u16, now_ms: u64) -> Option<QualityPreset> {
    if let Some(prev) = state.last_seq {
        let expected = prev.wrapping_add(1);
        if seq != expected {
            state.lost += seq.wrapping_sub(expected) as u32;
        }
    }
    state.last_seq = Some(seq);
    state.received = state.received.saturating_add(1);
    evaluate(state, now_ms)
}

/// Status kualitas satu sumber (B5).
#[derive(Debug, Clone, Default)]
pub struct QualityState {
    /// Preset saat ini.
    pub current: QualityPreset,
    last_rtt_ms: Option<u32>,
    last_seq: Option<u16>,
    received: u32,
    lost: u32,
    /// Milidetik pertama kali kondisi buruk terjadi (BERTAHAN).
    bad_since_ms: Option<u64>,
    /// Milidetik pertama kali kondisi stabil terjadi.
    good_since_ms: Option<u64>,
}

/// Kualitas dianggap buruk bila loss > 5% atau RTT > 80 ms (B5).
fn is_bad(state: &QualityState) -> bool {
    let loss_bad = state.received > 0
        && (state.lost as f64 / (state.received + state.lost) as f64) * 100.0
            > thresholds::LOSS_PCT;
    let rtt_bad = state.last_rtt_ms.unwrap_or(0) > thresholds::RTT_MS;
    loss_bad || rtt_bad
}

/// Evaluasi aturan B5; kembalikan preset baru bila transisi terjadi.
fn evaluate(state: &mut QualityState, now_ms: u64) -> Option<QualityPreset> {
    if is_bad(state) {
        state.good_since_ms = None;
        if state.bad_since_ms.is_none() {
            state.bad_since_ms = Some(now_ms);
        }
        let bad_for = now_ms.saturating_sub(state.bad_since_ms.unwrap_or(now_ms));
        if bad_for >= thresholds::BAD_SECS * 1000 {
            let next = state.current.step_down();
            if next != state.current {
                state.current = next;
                state.reset_window();
                return Some(next);
            }
        }
    } else {
        state.bad_since_ms = None;
        if state.good_since_ms.is_none() {
            state.good_since_ms = Some(now_ms);
        }
        let good_for = now_ms.saturating_sub(state.good_since_ms.unwrap_or(now_ms));
        if good_for >= thresholds::GOOD_SECS * 1000 {
            let next = state.current.step_up();
            if next != state.current {
                state.current = next;
                state.reset_window();
                return Some(next);
            }
        }
    }
    None
}

impl QualityState {
    /// Reset jendela pengukuran setelah transisi preset (B5).
    fn reset_window(&mut self) {
        self.received = 0;
        self.lost = 0;
        self.bad_since_ms = None;
        self.good_since_ms = None;
    }
}

impl QualityPreset {
    /// Turun satu tingkat (B5): High > Smooth > Balanced > Saving. Preview
    /// tidak ditarik ke bawah (sudah minimum).
    pub fn step_down(self) -> QualityPreset {
        match self {
            QualityPreset::High => QualityPreset::Smooth,
            QualityPreset::Smooth => QualityPreset::Balanced,
            QualityPreset::Balanced => QualityPreset::Saving,
            _ => self,
        }
    }

    /// Naik satu tingkat (B5): Saving > Balanced > Smooth > High.
    pub fn step_up(self) -> QualityPreset {
        match self {
            QualityPreset::Saving => QualityPreset::Balanced,
            QualityPreset::Balanced => QualityPreset::Smooth,
            QualityPreset::Smooth => QualityPreset::High,
            _ => self,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(s: u64) -> u64 {
        s * 1000
    }

    #[test]
    fn rtt_spike_then_sustained_bad_steps_down() {
        let mut st = QualityState {
            current: QualityPreset::High,
            ..Default::default()
        };
        // B5: RTT > 80 ms selama >3 detik → turun.
        assert_eq!(sample_rtt(&mut st, 120, ms(0)), None);
        assert_eq!(sample_rtt(&mut st, 120, ms(2)), None); // belum 3 detik
        assert_eq!(sample_rtt(&mut st, 120, ms(4)), Some(QualityPreset::Smooth));
        // B5: jendela direset; butuh 3 detik buruk lagi sebelum turun.
        assert_eq!(sample_rtt(&mut st, 130, ms(11)), None);
        assert_eq!(
            sample_rtt(&mut st, 130, ms(15)),
            Some(QualityPreset::Balanced)
        );
    }

    #[test]
    fn loss_over_five_pct_steps_down() {
        let mut st = QualityState {
            current: QualityPreset::High,
            ..Default::default()
        };
        // 100 paket berurutan (tidak ada loss).
        for seq in 0..100u16 {
            let _ = sample_seq(&mut st, seq, ms(0));
        }
        // Lompat ke seq 106: 6 paket hilang → 5,7% > 5% (B5).
        assert_eq!(sample_seq(&mut st, 106, ms(1)), None); // belum 3 detik
        assert_eq!(st.lost, 6);
        assert_eq!(sample_seq(&mut st, 107, ms(4)), Some(QualityPreset::Smooth));
    }

    #[test]
    fn stable_then_step_up() {
        let mut st = QualityState {
            current: QualityPreset::Saving,
            ..Default::default()
        };
        assert_eq!(sample_rtt(&mut st, 20, ms(0)), None);
        assert_eq!(sample_rtt(&mut st, 20, ms(9)), None); // belum 10 detik
        assert_eq!(
            sample_rtt(&mut st, 20, ms(11)),
            Some(QualityPreset::Balanced)
        );
    }

    #[test]
    fn step_up_capped_at_high() {
        let mut st = QualityState {
            current: QualityPreset::High,
            ..Default::default()
        };
        assert_eq!(sample_rtt(&mut st, 10, ms(12)), None);
    }

    #[test]
    fn step_down_capped_at_saving() {
        let mut st = QualityState {
            current: QualityPreset::Saving,
            ..Default::default()
        };
        assert_eq!(sample_rtt(&mut st, 200, ms(5)), None);
    }

    #[test]
    fn bad_resets_good_timer() {
        let mut st = QualityState::default();
        let _ = sample_rtt(&mut st, 20, ms(0));
        let _ = sample_rtt(&mut st, 20, ms(5));
        // RTT memburuk → timer stabil reset.
        let _ = sample_rtt(&mut st, 200, ms(6));
        let _ = sample_rtt(&mut st, 20, ms(16));
        assert_eq!(sample_rtt(&mut st, 20, ms(17)), None);
    }

    #[test]
    fn seq_wraps_correctly() {
        let mut st = QualityState::default();
        let _ = sample_seq(&mut st, u16::MAX - 1, ms(0));
        let _ = sample_seq(&mut st, u16::MAX, ms(0));
        let _ = sample_seq(&mut st, 0, ms(1)); // wrap: tidak ada loss
        assert_eq!(st.lost, 0);
    }
}
