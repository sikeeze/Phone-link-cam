//! PhoneCam vmic: abstraksi virtual mic (README Tech stack, B6).
//!
//! Aturan A9: kode platform hanya di crate ini (dan crates/vcam).
//!
//! - Windows: driver audio virtual (bundel atau arahkan install)
//! - macOS: Audio Server plugin (mirip BlackHole)
//! - Linux: virtual sink/source PipeWire atau PulseAudio
#![allow(dead_code)]
// kerangka: backend per OS diisi oleh fase 2/4
// A9: unsafe terbatas pada modul wasapi (Windows). Platform lain tetap murni.
#![deny(unsafe_code)]

use phonecam_core::error::{Error, Result};

/// Kode unsafe Windows dibungkus rapi di modul ini (A9: platform terbatas).
#[cfg(target_os = "windows")]
#[path = "wasapi.rs"]
pub mod wasapi;

/// Format audio (B6: Opus 48 kHz, 20 ms).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VMicFormat {
    pub sample_rate: u32,
    pub channels: u16,
    /// Durasi frame audio dalam ms (B6: 20 ms).
    pub frame_ms: u32,
}

impl Default for VMicFormat {
    fn default() -> Self {
        Self {
            sample_rate: 48_000,
            channels: 1,
            frame_ms: 20,
        }
    }
}

/// Status pemasangan virtual mic (B8: VMIC_NOT_INSTALLED).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VMicStatus {
    #[default]
    Installed,
    NotInstalled,
}

/// Trait backend virtual mic per OS (A9).
#[async_trait::async_trait]
pub trait VMic: Send + Sync {
    fn name(&self) -> &'static str;
    async fn status(&self) -> Result<VMicStatus>;
    async fn install(&self) -> Result<()>;
    async fn start(&self, format: VMicFormat) -> Result<()>;
    /// Kirim PCM ke virtual mic. Hot path (aturan A7): tidak boleh memblokir.
    async fn push_pcm(&self, pcm: &[u8]) -> Result<()>;
    async fn stop(&self) -> Result<()>;
}

/// Backend tidak tersedia di OS ini (stub untuk kompilasi silang).
pub struct UnsupportedBackend;

#[async_trait::async_trait]
impl VMic for UnsupportedBackend {
    fn name(&self) -> &'static str {
        "unsupported"
    }

    async fn status(&self) -> Result<VMicStatus> {
        Err(Error::internal(
            "backend virtual mic tidak tersedia di OS ini",
        ))
    }

    async fn install(&self) -> Result<()> {
        Err(Error::internal(
            "backend virtual mic tidak tersedia di OS ini",
        ))
    }

    async fn start(&self, _format: VMicFormat) -> Result<()> {
        Err(Error::internal(
            "backend virtual mic tidak tersedia di OS ini",
        ))
    }
    async fn push_pcm(&self, _pcm: &[u8]) -> Result<()> {
        Err(Error::internal(
            "backend virtual mic tidak tersedia di OS ini",
        ))
    }
    async fn stop(&self) -> Result<()> {
        Ok(())
    }
}

// --- Windows: WASAPI render ke perangkat input virtual -------------------
//
// Handle COM (IAudioClient) tidak Send: render dijalankan di thread
// dedicated, hot path hanya mengirim PCM lewat channel (A7: tidak memblokir).

#[cfg(target_os = "windows")]
enum RendererCmd {
    /// PCM i16_LE 20 ms (B6).
    Pcm(bytes::Bytes),
    Stop,
}

/// Backend Windows memakai perangkat input virtual (VB-CABLE dsb.).
#[cfg(target_os = "windows")]
pub struct WindowsBackend {
    tx: parking_lot::Mutex<Option<std::sync::mpsc::Sender<RendererCmd>>>,
}

#[cfg(target_os = "windows")]
impl Default for WindowsBackend {
    fn default() -> Self {
        Self { tx: None.into() }
    }
}

#[cfg(target_os = "windows")]
#[async_trait::async_trait]
impl VMic for WindowsBackend {
    fn name(&self) -> &'static str {
        "wasapi"
    }

    async fn status(&self) -> Result<VMicStatus> {
        let names = wasapi::list_virtual_inputs()?;
        Ok(if names.is_empty() {
            VMicStatus::NotInstalled
        } else {
            VMicStatus::Installed
        })
    }

    async fn install(&self) -> Result<()> {
        // B8: pengguna memasang driver/aset sendiri (tidak dibundel, A9).
        Err(Error::user(
            phonecam_core::error::UserErrorCode::VmicNotInstalled,
        ))
    }

    async fn start(&self, format: VMicFormat) -> Result<()> {
        let mut lock = self.tx.lock();
        if lock.is_some() {
            return Ok(());
        }
        let (tx, rx) = std::sync::mpsc::channel::<RendererCmd>();
        // Hasil open() dikirim balik agar `start` melaporkan kegagalan
        // (mis. VMIC_NOT_INSTALLED) alih-alih thread mati diam-diam.
        let (init_tx, init_rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("phonecam-vmic".into())
            .spawn(move || Self::render_loop(format, rx, init_tx))
            .map_err(|e| Error::internal(format!("vmic thread gagal: {e}")))?;
        // Init WASAPI seketika; tunggu hasil (timeout besar tidak perlu).
        init_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| Error::internal(format!("vmic init tidak merespons: {e}")))??;
        *lock = Some(tx);
        Ok(())
    }

    async fn push_pcm(&self, pcm: &[u8]) -> Result<()> {
        // A7: hot path — kirim ke channel, tidak pernah memblokir.
        let lock = self.tx.lock();
        let Some(tx) = lock.as_ref() else {
            return Ok(());
        };
        tx.send(RendererCmd::Pcm(bytes::Bytes::copy_from_slice(pcm)))
            .map_err(|_| Error::internal("vmic renderer thread berhenti"))?;
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        let mut lock = self.tx.lock();
        if let Some(tx) = lock.take() {
            let _ = tx.send(RendererCmd::Stop);
        }
        Ok(())
    }
}

#[cfg(target_os = "windows")]
impl WindowsBackend {
    /// Loop render di thread COM dedicated. Berhenti saat channel ditutup
    /// atau perintah Stop diterima (A11: tidak panic).
    fn render_loop(
        format: VMicFormat,
        rx: std::sync::mpsc::Receiver<RendererCmd>,
        init: std::sync::mpsc::Sender<phonecam_core::error::Result<()>>,
    ) {
        let mut render = match wasapi::WasapiRender::open(format.sample_rate, format.channels) {
            Ok(r) => {
                let _ = init.send(Ok(()));
                r
            }
            Err(e) => {
                let _ = init.send(Err(e));
                return;
            }
        };
        while let Ok(cmd) = rx.recv() {
            match cmd {
                RendererCmd::Pcm(pcm) => {
                    if let Err(e) = render.write(&pcm) {
                        tracing::warn!(target: "phonecam::vmic", "vmic write error: {e:?}");
                    }
                }
                RendererCmd::Stop => break,
            }
        }
        let _ = render.close();
    }
}

/// Backend virtual mic untuk OS ini (A9).
#[cfg(target_os = "windows")]
pub fn platform_backend() -> Box<dyn VMic> {
    Box::new(WindowsBackend::default())
}

/// Backend virtual mic untuk OS ini (A9: non-Windows belum diimplementasi).
#[cfg(not(target_os = "windows"))]
pub fn platform_backend() -> Box<dyn VMic> {
    Box::new(UnsupportedBackend)
}
