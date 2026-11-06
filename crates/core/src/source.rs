//! Sumber (HP) yang terhubung ke sesi desktop (README B2/B4).
//!
//! Maksimal 6 sumber per sesi (B2). Sumber ke-7 ditolak dengan SESSION_FULL.

use std::collections::HashMap;

use crate::constants::limits::MAX_SOURCES;
use crate::error::{Error, Result, UserErrorCode};
use crate::protocol::{CameraFacing, QualityPreset, SourceRole, Transport};
use crate::session::SessionState;
use serde::{Deserialize, Serialize};

/// ID sumber unik per sesi desktop.
pub type SourceId = String;

/// Metadata sumber yang dilaporkan HP lewat `hello` (B4).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceInfo {
    pub device_id: String,
    pub name: String,
    pub user_agent: String,
    pub transport: Transport,
    /// Jalur koneksi (Wi-Fi, USB, Hotspot) untuk tag di UI (C4).
    #[serde(default)]
    pub connection_kind: ConnectionKind,
}

/// Tag jalur koneksi untuk daftar HP terhubung (C4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ConnectionKind {
    #[default]
    Wifi,
    Usb,
    Hotspot,
}

impl ConnectionKind {
    pub fn label_id(&self) -> &'static str {
        match self {
            ConnectionKind::Wifi => "Wi-Fi",
            ConnectionKind::Usb => "USB",
            ConnectionKind::Hotspot => "Hotspot",
        }
    }
}

/// Status dinamis sumber (B4 `source_update` + state B3).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SourceStatus {
    pub state: SessionStateDefault,
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
    /// RTT ke sumber dalam ms (B9.1 badge kualitas).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rtt_ms: Option<u32>,
    /// Thumbnail statis base64 JPEG (B6: snapshot 2 detik dari HP, tanpa
    /// decode paralel di PC — diambil dari elemen `video` lewat canvas).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumb: Option<String>,
}

/// serde default untuk SessionState (B3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub struct SessionStateDefault(pub SessionState);

/// Peran sumber: aktif (di-decode penuh) atau preview (resolusi kecil) (B6, fase 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub struct SourceRoleWrap(pub SourceRole);

/// Satu sumber dalam sesi desktop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub id: SourceId,
    pub info: SourceInfo,
    pub status: SourceStatus,
    /// Token reconnect 128-bit (B2).
    pub token: String,
    /// Preset kualitas saat ini (B5).
    pub quality: QualityPreset,
    /// Peran aktif/preview (B6).
    pub role: SourceRole,
}

impl Source {
    pub fn new(id: SourceId, info: SourceInfo, token: String) -> Self {
        Self {
            id,
            info,
            status: SourceStatus::default(),
            token,
            quality: QualityPreset::Balanced,
            // B6: default preview; sumber pertama dijadikan aktif saat pair.
            role: SourceRole::Preview,
        }
    }
}

/// Registry sumber untuk sesi desktop (B2: maks 6).
#[derive(Default)]
pub struct SourceRegistry {
    by_id: HashMap<SourceId, Source>,
    order: Vec<SourceId>,
}

impl SourceRegistry {
    /// Tambah sumber; mengembalikan SESSION_FULL bila sudah 6 (B2).
    pub fn insert(&mut self, source: Source) -> Result<()> {
        if self.by_id.len() >= MAX_SOURCES && !self.by_id.contains_key(&source.id) {
            return Err(Error::user(UserErrorCode::SessionFull));
        }
        if !self.by_id.contains_key(&source.id) {
            self.order.push(source.id.clone());
        }
        // B6: sumber pertama otomatis menjadi aktif (fallback awal).
        let mut source = source;
        if !self.by_id.values().any(|s| s.role == SourceRole::Active) {
            source.role = SourceRole::Active;
        }
        self.by_id.insert(source.id.clone(), source);
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&Source> {
        self.by_id.get(id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Source> {
        self.by_id.get_mut(id)
    }

    pub fn remove(&mut self, id: &str) -> Option<Source> {
        self.order.retain(|o| o != id);
        self.by_id.remove(id)
    }

    /// Cari sumber berdasarkan token reconnect (B2/B3). Token bersifat rahasia
    /// (A8); pembandingan dilakukan di sisi server dalam registry.
    pub fn find_by_token(&self, token: &str) -> Option<&Source> {
        self.by_id.values().find(|s| s.token == token)
    }

    /// Tandai sumber sebagai aktif; sumber aktif lain diturunkan ke
    /// preview (B6: tepat satu aktif). Mengembalikan ID sumber yang
    /// diturunkan (untuk pengiriman `cmd_role`).
    pub fn set_active(&mut self, id: &str) -> Option<SourceId> {
        let mut demoted = None;
        for s in self.by_id.values_mut() {
            if s.id == id {
                s.role = SourceRole::Active;
            } else if s.role == SourceRole::Active {
                s.role = SourceRole::Preview;
                demoted = Some(s.id.clone());
            }
        }
        demoted
    }

    /// Sumber aktif saat ini (B6; bisa None bila semua preview/putus).
    pub fn active(&self) -> Option<&Source> {
        self.by_id.values().find(|s| s.role == SourceRole::Active)
    }

    /// Daftar sumber dalam urutan masuk (untuk switcher 1-6, B7).
    pub fn list(&self) -> Vec<&Source> {
        self.order
            .iter()
            .filter_map(|id| self.by_id.get(id))
            .collect()
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// True jika sudah mencapai batas 6 sumber (B2).
    pub fn is_full(&self) -> bool {
        self.by_id.len() >= MAX_SOURCES
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(id: &str) -> Source {
        Source::new(
            id.into(),
            SourceInfo {
                device_id: "dev".into(),
                name: "Pixel 8".into(),
                user_agent: "UA".into(),
                transport: Transport::WebRtc,
                connection_kind: ConnectionKind::Wifi,
            },
            "t".into(),
        )
    }

    #[test]
    fn insert_and_lookup() {
        let mut r = SourceRegistry::default();
        r.insert(src("s1")).unwrap();
        assert_eq!(r.len(), 1);
        assert!(r.get("s1").is_some());
        assert!(r.get("nope").is_none());
    }

    #[test]
    fn sixth_rejected_session_full() {
        let mut r = SourceRegistry::default();
        for i in 1..=6 {
            r.insert(src(&format!("s{i}"))).unwrap();
        }
        assert!(r.is_full());
        let err = r.insert(src("s7")).unwrap_err();
        assert!(matches!(
            err,
            Error::User {
                code: UserErrorCode::SessionFull,
                ..
            }
        ));
    }

    #[test]
    fn order_preserved_for_switcher() {
        let mut r = SourceRegistry::default();
        for id in ["b", "a", "c"] {
            r.insert(src(id)).unwrap();
        }
        let names: Vec<_> = r.list().into_iter().map(|s| s.id.clone()).collect();
        assert_eq!(names, vec!["b", "a", "c"]);
    }

    #[test]
    fn first_source_becomes_active() {
        let mut r = SourceRegistry::default();
        r.insert(src("s1")).unwrap();
        r.insert(src("s2")).unwrap();
        assert_eq!(r.active().map(|s| s.id.as_str()), Some("s1"));
        assert_eq!(r.get("s2").map(|s| s.role), Some(SourceRole::Preview));
    }

    #[test]
    fn set_active_demotes_previous() {
        let mut r = SourceRegistry::default();
        for id in ["s1", "s2", "s3"] {
            r.insert(src(id)).unwrap();
        }
        let demoted = r.set_active("s2");
        assert_eq!(demoted.as_deref(), Some("s1"));
        assert_eq!(r.active().map(|s| s.id.as_str()), Some("s2"));
        assert_eq!(r.get("s1").map(|s| s.role), Some(SourceRole::Preview));
    }

    #[test]
    fn remove_updates_order() {
        let mut r = SourceRegistry::default();
        for id in ["s1", "s2", "s3"] {
            r.insert(src(id)).unwrap();
        }
        r.remove("s2");
        let names: Vec<_> = r.list().into_iter().map(|s| s.id.clone()).collect();
        assert_eq!(names, vec!["s1", "s3"]);
        assert_eq!(r.len(), 2);
    }
}
