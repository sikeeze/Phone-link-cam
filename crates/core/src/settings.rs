//! Pengaturan aplikasi (README C3.4) dengan persistence opsional.
//! Perubahan diterapkan langsung tanpa tombol Simpan (C3.4).

use crate::constants::{port, quality};
use crate::error::{Error, Result, UserErrorCode};
use serde::{Deserialize, Serialize};

/// Tema antarmuka (C1: ikut sistem, bisa diganti manual).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

/// Bahasa antarmuka (C3: Indonesia dan Inggris).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Locale {
    #[default]
    Id,
    En,
}

/// Mode muatan (fit) video: fit (letterbox) atau fill (crop) (B6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FitMode {
    #[default]
    Fit,
    Fill,
}

/// Sumber mikrofon (B6, fase 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MicSource {
    /// Mic laptop.
    #[default]
    Local,
    /// Mic HP aktif.
    Phone,
}

/// Resolusi output virtual camera (B6: ukuran konstan, tidak berubah saat kualitas berubah).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OutputResolution {
    #[default]
    P720,
    P1080,
}

impl OutputResolution {
    pub fn dims(self) -> (u32, u32) {
        match self {
            OutputResolution::P720 => (
                quality::OUTPUT_DEFAULT_WIDTH,
                quality::OUTPUT_DEFAULT_HEIGHT,
            ),
            OutputResolution::P1080 => (1920, 1080),
        }
    }
}

/// Pengaturan video (C3.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct VideoSettings {
    pub resolution: OutputResolution,
    /// fps output virtual camera.
    pub fps: u32,
    pub fit: FitMode,
    /// Mirror default untuk sumber baru.
    pub mirror: bool,
    /// Rotasi dalam derajat (kelipatan 90).
    pub rotate: u32,
}

/// Pengaturan audio (C3.4, fase 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AudioSettings {
    pub mic: MicSource,
    /// noiseSuppression opsional (B6).
    pub noise_suppression: bool,
}

/// Pengaturan jaringan (C3.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkSettings {
    /// Port HTTPS (B1: 8443 default, cari port kosong berikutnya jika bentrok).
    pub https_port: u16,
    /// Rentang UDP media (B1).
    pub udp_range: (u16, u16),
    /// Antarmuka yang dipilih secara manual; None = semua antarmuka privat (B1).
    pub interface_name: Option<String>,
}

impl Default for NetworkSettings {
    fn default() -> Self {
        Self {
            https_port: port::HTTPS,
            udp_range: (port::UDP_MEDIA_START, port::UDP_MEDIA_END),
            interface_name: None,
        }
    }
}

/// Pengaturan hotkey switcher (B7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HotkeySettings {
    /// Hotkey 1-6 memilih sumber saat aplikasi fokus (B7).
    pub number_keys_switch: bool,
    /// Global Ctrl+Alt+1..6 (B7: opsional, bisa dimatikan).
    pub global_keys: bool,
    /// Panah kiri/kanan mengganti sumber (B7).
    pub arrow_keys_switch: bool,
}

impl HotkeySettings {
    pub fn defaults() -> Self {
        Self {
            number_keys_switch: true,
            global_keys: false,
            arrow_keys_switch: true,
        }
    }
}

impl Default for HotkeySettings {
    fn default() -> Self {
        Self::defaults()
    }
}

/// Pengaturan umum (C3.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct GeneralSettings {
    pub theme: Theme,
    pub locale: Locale,
    /// Mulai bersama sistem (C3.4).
    pub autostart: bool,
}

/// Seluruh pengaturan aplikasi (C3.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Settings {
    pub general: GeneralSettings,
    pub video: VideoSettings,
    pub audio: AudioSettings,
    pub network: NetworkSettings,
    #[serde(default = "HotkeySettings::defaults")]
    pub hotkeys: HotkeySettings,
}

impl Settings {
    /// Muat pengaturan dari file JSON; kalau tidak ada, kembalikan default (C3.4).
    pub fn load_or_default(path: &std::path::Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                let s: Settings = serde_json::from_str(&text)?;
                Ok(s)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(Error::Io(e)),
        }
    }

    /// Simpan pengaturan ke file JSON (C3.4: diterapkan langsung).
    pub fn save(&self, path: &std::path::Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| Error::internal(format!("gagal membuat direktori pengaturan: {e}")))?;
        }
        let text = serde_json::to_string_pretty(self)?;
        std::fs::write(path, text)
            .map_err(|e| Error::internal(format!("gagal menyimpan pengaturan: {e}")))?;
        Ok(())
    }

    /// Validasi pengaturan (B1: port dan rentang UDP harus masuk akal).
    pub fn validate(&self) -> Result<()> {
        if self.network.https_port == 0 {
            return Err(Error::user(UserErrorCode::FirewallBlocked));
        }
        let (start, end) = self.network.udp_range;
        if start == 0 || end < start {
            return Err(Error::internal("rentang UDP tidak valid"));
        }
        if self.video.rotate % 90 != 0 {
            return Err(Error::internal("rotasi harus kelipatan 90 derajat"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_spec() {
        let s = Settings::default();
        assert_eq!(s.network.https_port, 8443);
        assert_eq!(s.network.udp_range, (40000, 40100));
        assert_eq!(s.video.resolution.dims(), (1280, 720));
        assert_eq!(s.video.fps, 0); // default fps diisi saat pipeline aktif
        assert_eq!(s.general.theme, Theme::System);
        assert_eq!(s.general.locale, Locale::Id);
        assert!(!s.hotkeys.global_keys); // B7: global default mati
        assert!(s.hotkeys.number_keys_switch);
    }

    #[test]
    fn roundtrip_json() {
        let s = Settings::default();
        let text = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&text).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn load_missing_file_returns_default() {
        let p = std::env::temp_dir().join("phonecam_test_settings_nonexistent.json");
        let s = Settings::load_or_default(&p).unwrap();
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let p = std::env::temp_dir().join("phonecam_test_settings_roundtrip.json");
        let mut s = Settings::default();
        s.video.mirror = true;
        s.network.https_port = 9443;
        s.save(&p).unwrap();
        let back = Settings::load_or_default(&p).unwrap();
        assert_eq!(back.video.mirror, true);
        assert_eq!(back.network.https_port, 9443);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn validation_catches_bad_values() {
        let mut s = Settings::default();
        s.network.https_port = 0;
        assert!(s.validate().is_err());
        let mut s = Settings::default();
        s.network.udp_range = (100, 50);
        assert!(s.validate().is_err());
        let mut s = Settings::default();
        s.video.rotate = 45;
        assert!(s.validate().is_err());
        assert!(Settings::default().validate().is_ok());
    }
}
