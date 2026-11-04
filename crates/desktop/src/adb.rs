//! B9.2: deteksi & kontrol adb (Android Debug Bridge) untuk mode kabel.
//!
//! Strategi ADR-002 D1: deteksi `adb` di PATH; jika tidak ada, jangan
//! memerintahkan pelarian otomatis — UI bertanya izin unduh eksplisit.
//! Modul ini hanya mencari binary dan menjalankan perintah port-reverse.

use anyhow::{anyhow, Result};
use std::process::Command;

/// Path binary adb (bisa dari PATH atau hasil unduh yang dihashkan).
#[derive(Debug, Clone)]
pub struct Adb {
    bin: std::path::PathBuf,
}

/// Hasil pemeriksaan binary adb (B9.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdbState {
    /** `adb version` jalan — daemon bisa dimulai. */
    Ready,
    /** Binary ada tetapi tidak ada perangkat atau belum diizinkan (B9.4 ADB_UNAUTHORIZED). */
    Unauthorized,
}

impl Adb {
    /// Cari `adb` di PATH (B9.2). Mengembalikan None kalau tidak ada —
    /// panggilan UI yang menampilkan dialog unduh (ADR-002 D1).
    pub fn detect() -> Option<Self> {
        let ext = if cfg!(windows) { "adb.exe" } else { "adb" };
        let paths = std::env::var_os("PATH")?;
        for dir in std::env::split_paths(&paths) {
            let p = dir.join(ext);
            if p.is_file() {
                return Some(Self { bin: p });
            }
        }
        None
    }

    /// Pakai binary dari path unduhan (ADR-002 D1).
    pub fn from_path(bin: impl Into<std::path::PathBuf>) -> Self {
        Self { bin: bin.into() }
    }

    fn run(&self, args: &[&str]) -> Result<std::process::Output> {
        Command::new(&self.bin)
            .args(args)
            .output()
            .map_err(|e| anyhow!("gagal menjalankan adb: {e}"))
    }

    /// `adb version` — validasi binary hidup (B9.2).
    pub fn state(&self) -> Result<AdbState> {
        let out = self.run(&["version"])?;
        if !out.status.success() {
            return Err(anyhow!("adb version gagal"));
        }
        // `adb devices -l` untuk status perangkat.
        let devs = self.run(&["devices"])?;
        let txt = String::from_utf8_lossy(&devs.stdout);
        // Format: "List of devices attached\n<serial> <state>\n"
        let any = txt
            .lines()
            .skip(1)
            .any(|l| l.split_whitespace().nth(1) == Some("device"));
        let unauthorized = txt
            .lines()
            .skip(1)
            .any(|l| l.split_whitespace().nth(1) == Some("unauthorized"));
        if any {
            Ok(AdbState::Ready)
        } else if unauthorized {
            Ok(AdbState::Unauthorized)
        } else {
            Err(anyhow!("tidak ada perangkat adb"))
        }
    }

    /// `adb reverse tcp:<port> tcp:<port>` — HP mengakses PC via 127.0.0.1 (B9.2).
    /// Port WebRTC (sender) tidak bisa diteruskan dengan cara ini; ponytail:
    /// mode ADB memakai TURN/tidak ada ICE peer-reflexive — lihat README §D.
    pub fn reverse(&self, port: u16) -> Result<()> {
        let spec = format!("tcp:{port}");
        let out = self.run(&["reverse", &spec, &spec])?;
        if out.status.success() {
            Ok(())
        } else {
            Err(anyhow!(
                "adb reverse gagal: {}",
                String::from_utf8_lossy(&out.stderr)
            ))
        }
    }

    /// `adb reverse --remove tcp:<port>` — hapus forwarding (B9.2 poin 5:
    /// hapus saat sesi berakhir dan saat aplikasi ditutup).
    pub fn reverse_remove(&self, port: u16) -> Result<()> {
        let spec = format!("tcp:{port}");
        let out = self.run(&["reverse", "--remove", &spec])?;
        if out.status.success() {
            Ok(())
        } else {
            Err(anyhow!(
                "adb reverse --remove gagal: {}",
                String::from_utf8_lossy(&out.stderr)
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_returns_none_or_some_without_panic() {
        // B9.2: PATH tanpa adb harus None, bukan error/panic.
        let d = Adb::detect();
        if let Some(a) = d {
            // Kalau adb terpasang di mesin uji, binary harus ada.
            assert!(a.bin.is_file(), "{:?} harusnya file", a.bin);
        }
    }

    #[test]
    fn from_path_keeps_bin() {
        let a = Adb::from_path("/somewhere/adb");
        assert_eq!(a.bin, std::path::PathBuf::from("/somewhere/adb"));
    }
}
