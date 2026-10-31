//! Konstanta binding dari README bagian B1/B2/B4/B5.
//! Satu-satunya tempat angka-angka protokol didefinisikan.

pub mod port {
    /// Server HTTPS + WebSocket (B1). Jika bentrok, cari port kosong berikutnya.
    pub const HTTPS: u16 = 8443;
    /// Listener HTTP khusus mode ADB (B1/B9.2). Hanya 127.0.0.1, tidak pernah di-bind ke antarmuka lain.
    pub const ADB_HTTP: u16 = 8080;
    /// Rentang UDP media (ICE) (B1).
    pub const UDP_MEDIA_START: u16 = 40000;
    pub const UDP_MEDIA_END: u16 = 40100;
}

pub mod limits {
    /// Maksimum 6 sumber (HP) per sesi (B2).
    pub const MAX_SOURCES: usize = 6;
    /// Ukuran maksimum satu pesan signaling: 16 KB (B4).
    pub const MAX_MESSAGE_BYTES: usize = 16 * 1024;
    /// Maksimal 5 percobaan PIN salah per alamat IP (B2).
    pub const MAX_PIN_ATTEMPTS: u32 = 5;
    /// Lama kunci setelah percobaan PIN habis: 60 detik (B2).
    pub const PIN_LOCKOUT_SECS: u64 = 60;
    /// Maksimum pesan dalam antrean keluar per sambungan sebelum membuang pesan lama.
    pub const OUTBOUND_QUEUE: usize = 64;
}

pub mod timing {
    /// PIN berlaku 5 menit (B2).
    pub const PIN_TTL_SECS: i64 = 5 * 60;
    /// ping/pong tiap 2 detik, timeout 6 detik (B4).
    pub const PING_INTERVAL_SECS: u64 = 2;
    pub const PING_TIMEOUT_SECS: u64 = 6;
    /// Connection Manager: enumerasi tiap 3 detik (B9.1).
    pub const INTERFACE_POLL_SECS: u64 = 3;
}

pub mod quality {
    /// Preset kualitas (B5). Lihat protocol::QualityPreset.
    pub const PREVIEW_WIDTH: u32 = 320;
    pub const PREVIEW_HEIGHT: u32 = 180;
    pub const PREVIEW_FPS: u32 = 15;
    pub const PREVIEW_BITRATE_BPS: u32 = 250_000;

    /// Output virtual camera konstan (B5/B6): default 1280x720@30.
    pub const OUTPUT_DEFAULT_WIDTH: u32 = 1280;
    pub const OUTPUT_DEFAULT_HEIGHT: u32 = 720;
    pub const OUTPUT_DEFAULT_FPS: u32 = 30;

    /// Rentang bitrate adaptif: 1-6 Mbps (Target performa).
    pub const ADAPTIVE_MIN_BITRATE_BPS: u32 = 1_000_000;
    pub const ADAPTIVE_MAX_BITRATE_BPS: u32 = 6_000_000;
}

pub mod adapt {
    /// Turunkan satu tingkat jika packet loss > 5% atau RTT > 80 ms selama 3 detik (B5).
    pub const LOSS_THRESHOLD_PCT: f32 = 5.0;
    pub const RTT_THRESHOLD_MS: u32 = 80;
    pub const DOWNGRADE_WINDOW_SECS: u64 = 3;
    /// Naikkan jika stabil 10 detik (B5).
    pub const UPGRADE_STABLE_SECS: u64 = 10;
}

pub mod reconnect {
    /// Backoff eksponensial 0,5 s, 1 s, 2 s, 4 s, maks 8 s, total 60 s (B3).
    pub const BACKOFF_STEPS_MS: &[u64] = &[500, 1000, 2000, 4000, 8000];
    pub const BACKOFF_MAX_MS: u64 = 8000;
    pub const TOTAL_BUDGET_SECS: u64 = 60;
}
