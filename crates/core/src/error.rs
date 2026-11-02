//! Error codes pengguna dari README B8 dan B9.4, plus error internal bertipe.
//! Aturan A11: tidak ada unwrap/expect di kode non-test, error bertipe (thiserror),
//! setiap error yang menghadap pengguna punya kode dari B8/B9.4 dan pesan manusia.

use thiserror::Error;

/// Kode error yang menghadap pengguna (B8 + B9.4). Representasi string stabil
/// (dipakai di pesan auth_fail, UI desktop, dan PWA HP).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UserErrorCode {
    // B8
    BadPin,
    Locked,
    Expired,
    SessionFull,
    PermissionDenied,
    NoCamera,
    NetworkLost,
    VcamNotInstalled,
    VmicNotInstalled,
    FirewallBlocked,
    CertUntrusted,
    // B9.4
    UsbNotDetected,
    AdbMissing,
    AdbUnauthorized,
    TetherNoIp,
    TransportUnsupported,
    PortForwardFailed,
}

impl UserErrorCode {
    /// (judul, penjelasan satu kalimat, satu aksi perbaikan) per B8.
    /// Bahasa Indonesia untuk layar utama; versi EN dialihkan di layer UI.
    pub fn info(self) -> (&'static str, &'static str, &'static str) {
        match self {
            UserErrorCode::BadPin => (
                "PIN salah",
                "PIN yang kamu masukkan tidak cocok dengan yang ditampilkan di PC.",
                "Periksa PIN di layar PC lalu coba lagi.",
            ),
            UserErrorCode::Locked => (
                "Terlalu banyak percobaan",
                "Kamu sudah salah memasukkan PIN 5 kali.",
                "Tunggu 60 detik lalu coba lagi.",
            ),
            UserErrorCode::Expired => (
                "PIN kedaluwarsa",
                "PIN sudah berlaku lebih dari 5 menit.",
                "Minta PIN baru dari PC.",
            ),
            UserErrorCode::SessionFull => (
                "Sesi penuh",
                "Sudah ada 6 HP terhubung ke PC ini.",
                "Putuskan salah satu HP di PC lalu coba lagi.",
            ),
            UserErrorCode::PermissionDenied => (
                "Izin kamera ditolak",
                "Browser tidak diizinkan mengakses kamera atau mic.",
                "Buka pengaturan browser dan izinkan kamera untuk situs ini.",
            ),
            UserErrorCode::NoCamera => (
                "Tidak ada kamera",
                "Tidak ada kamera yang terdeteksi di HP.",
                "Pastikan kamera tidak sedang dipakai aplikasi lain.",
            ),
            UserErrorCode::NetworkLost => (
                "Koneksi terputus",
                "Sambungan ke PC terputus.",
                "Pastikan HP dan PC di jaringan yang sama lalu tekan Coba lagi.",
            ),
            UserErrorCode::VcamNotInstalled => (
                "Virtual camera belum terpasang",
                "Aplikasi lain belum bisa melihat kamera HP.",
                "Tekan Pasang virtual camera di PC.",
            ),
            UserErrorCode::VmicNotInstalled => (
                "Virtual mic belum terpasang",
                "Aplikasi lain belum bisa mendengar mic HP.",
                "Pasang virtual mic di PC.",
            ),
            UserErrorCode::FirewallBlocked => (
                "Diblokir firewall",
                "Windows Firewall menolak koneksi masuk ke PhoneCam.",
                "Izinkan PhoneCam melalui firewall.",
            ),
            UserErrorCode::CertUntrusted => (
                "Sertifikat tidak dipercaya",
                "Browser HP tidak mempercayai sertifikat PC.",
                "Buka alamat PC, terima peringatan sertifikat, lalu sambungkan kembali.",
            ),
            UserErrorCode::UsbNotDetected => (
                "USB tidak terdeteksi",
                "PC tidak melihat HP lewat kabel.",
                "Coba kabel lain yang mendukung data, dan cek port USB.",
            ),
            UserErrorCode::AdbMissing => (
                "ADB tidak ada",
                "PC tidak memiliki alat adb.",
                "Pasang platform-tools atau gunakan USB tethering.",
            ),
            UserErrorCode::AdbUnauthorized => (
                "USB debugging belum disetujui",
                "HP meminta persetujuan koneksi debug.",
                "Setujui dialog 'Izinkan debugging USB' di HP.",
            ),
            UserErrorCode::TetherNoIp => (
                "Tethering belum siap",
                "Antarmuka tethering belum mendapat alamat IP.",
                "Matikan lalu nyalakan USB tethering.",
            ),
            UserErrorCode::TransportUnsupported => (
                "Transport tidak didukung",
                "Browser HP tidak mendukung WebCodecs untuk mode kabel.",
                "Gunakan Wi-Fi atau USB tethering sebagai gantinya.",
            ),
            UserErrorCode::PortForwardFailed => (
                "Penerusan port gagal",
                "adb reverse tidak bisa meneruskan port 8080.",
                "Cabut lalu colok kabel, dan pastikan USB debugging aktif.",
            ),
        }
    }

    /// String stabil untuk protokol dan log. Tidak pernah memuat PIN/token.
    pub fn as_str(&self) -> &'static str {
        match self {
            UserErrorCode::BadPin => "BAD_PIN",
            UserErrorCode::Locked => "LOCKED",
            UserErrorCode::Expired => "EXPIRED",
            UserErrorCode::SessionFull => "SESSION_FULL",
            UserErrorCode::PermissionDenied => "PERMISSION_DENIED",
            UserErrorCode::NoCamera => "NO_CAMERA",
            UserErrorCode::NetworkLost => "NETWORK_LOST",
            UserErrorCode::VcamNotInstalled => "VCAM_NOT_INSTALLED",
            UserErrorCode::VmicNotInstalled => "VMIC_NOT_INSTALLED",
            UserErrorCode::FirewallBlocked => "FIREWALL_BLOCKED",
            UserErrorCode::CertUntrusted => "CERT_UNTRUSTED",
            UserErrorCode::UsbNotDetected => "USB_NOT_DETECTED",
            UserErrorCode::AdbMissing => "ADB_MISSING",
            UserErrorCode::AdbUnauthorized => "ADB_UNAUTHORIZED",
            UserErrorCode::TetherNoIp => "TETHER_NO_IP",
            UserErrorCode::TransportUnsupported => "TRANSPORT_UNSUPPORTED",
            UserErrorCode::PortForwardFailed => "PORT_FORWARD_FAILED",
        }
    }
}

impl std::fmt::Display for UserErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("kesalahan protokol: {0}")]
    Protocol(String),

    #[error("pesan melebihi batas {limit} byte")]
    MessageTooLarge { limit: usize },

    #[error("error pengguna: {code:?} ({code})")]
    User {
        code: UserErrorCode,
        message: String,
    },

    #[error("error internal: {0}")]
    Internal(String),

    #[error("error jaringan: {0}")]
    Net(String),

    #[error("error serialisasi: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("error I/O: {0}")]
    Io(#[from] std::io::Error),

    #[error("error TLS: {0}")]
    Tls(String),
}

impl Error {
    pub fn user(code: UserErrorCode) -> Self {
        let message = code.info().1.to_string();
        Error::User { code, message }
    }

    pub fn protocol(msg: impl Into<String>) -> Self {
        Error::Protocol(msg.into())
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Error::Internal(msg.into())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_code_has_info_triple() {
        // B8: setiap kode punya judul, penjelasan satu kalimat, dan satu aksi.
        for code in [
            UserErrorCode::BadPin,
            UserErrorCode::Locked,
            UserErrorCode::Expired,
            UserErrorCode::SessionFull,
            UserErrorCode::PermissionDenied,
            UserErrorCode::NoCamera,
            UserErrorCode::NetworkLost,
            UserErrorCode::VcamNotInstalled,
            UserErrorCode::VmicNotInstalled,
            UserErrorCode::FirewallBlocked,
            UserErrorCode::CertUntrusted,
            UserErrorCode::UsbNotDetected,
            UserErrorCode::AdbMissing,
            UserErrorCode::AdbUnauthorized,
            UserErrorCode::TetherNoIp,
            UserErrorCode::TransportUnsupported,
            UserErrorCode::PortForwardFailed,
        ] {
            let (title, explain, action) = code.info();
            assert!(!title.is_empty(), "title kosong untuk {code:?}");
            assert!(!explain.is_empty(), "explanation kosong untuk {code:?}");
            assert!(!action.is_empty(), "action kosong untuk {code:?}");
            assert_eq!(code.as_str(), code.as_str().to_uppercase());
        }
    }

    #[test]
    fn codes_are_stable_strings() {
        assert_eq!(UserErrorCode::BadPin.as_str(), "BAD_PIN");
        assert_eq!(UserErrorCode::SessionFull.as_str(), "SESSION_FULL");
        assert_eq!(UserErrorCode::AdbUnauthorized.as_str(), "ADB_UNAUTHORIZED");
    }
}
