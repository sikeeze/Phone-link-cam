//! Switcher multi-kamera (B6, fase 3): cut, crossfade 200 ms, slide 250 ms.
//! Sebelum switch: kirim cmd_keyframe dan tunggu keyframe (maks 500 ms).

use std::time::Duration;

/// Jenis transisi (B6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Transition {
    #[default]
    Cut,
    /// Crossfade 200 ms (B6).
    Fade,
    /// Slide 250 ms (B6).
    Slide,
}

impl Transition {
    pub fn duration(self) -> Duration {
        match self {
            Transition::Cut => Duration::ZERO,
            Transition::Fade => Duration::from_millis(200),
            Transition::Slide => Duration::from_millis(250),
        }
    }

    pub fn label_id(self) -> &'static str {
        match self {
            Transition::Cut => "Cut",
            Transition::Fade => "Fade",
            Transition::Slide => "Slide",
        }
    }
}

/// Maksimum menunggu keyframe sebelum switch (B6: 500 ms).
pub const KEYFRAME_WAIT: Duration = Duration::from_millis(500);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_match_b6() {
        assert_eq!(Transition::Cut.duration(), Duration::ZERO);
        assert_eq!(Transition::Fade.duration(), Duration::from_millis(200));
        assert_eq!(Transition::Slide.duration(), Duration::from_millis(250));
        assert_eq!(KEYFRAME_WAIT, Duration::from_millis(500));
    }
}
