//! Backend Windows: virtual mic via WASAPI render ke perangkat input virtual.
//!
//! PhoneCam tidak membungkus driver kernel (A9: kode platform terbatas). Kita
//! memakai perangkat input virtual yang sudah terpasang di sistem, cara pakai
//! sama dengan vcam OBS (B8: VMIC_NOT_INSTALLED mengarahkan pengguna pasang).
//!
//! Deteksi: `list_virtual_inputs()` mencari perangkat capture aktif dengan
//! nama fisik perangkat virtual umum (VB-Audio Virtual Cable dsb.).
//! ponytail: bila nanti ada driver sendiri, ganti predicate nama saja.
// A9: unsafe COM Windows dibatasi hanya di modul ini.
#![allow(unsafe_code)]

use phonecam_core::error::{Error, Result};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::{
    eCapture, IAudioClient, IAudioRenderClient, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator,
    AUDCLNT_SHAREMODE_SHARED, DEVICE_STATE_ACTIVE, WAVEFORMATEX, WAVE_FORMAT_PCM,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ,
};
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;

/// Konversi windows::core::Result ke Result PhoneCam (A11: tidak panic).
trait WinResult<T> {
    fn win(self) -> Result<T>;
}

impl<T> WinResult<T> for windows::core::Result<T> {
    fn win(self) -> Result<T> {
        self.map_err(|e| Error::internal(format!("vmic WASAPI: {}", e)))
    }
}

/// Ekstrak string dari PROPVARIANT VT_LPWSTR.
fn prop_string(prop: &PROPVARIANT) -> Option<String> {
    unsafe {
        let s = prop.Anonymous.Anonymous.Anonymous.pwszVal;
        if s.is_null() {
            return None;
        }
        s.to_string().ok()
    }
}

/// Nama fisik perangkat input virtual yang kita dukung (B6/A9).
pub const VIRTUAL_MIC_NAMES: &[&str] = &[
    "VB-Audio Virtual Cable",
    "VB-Audio VAIO Virtual Cable",
    "VB-Audio Virtual Cable B",
    "CABLE Input",
    "Virtual Audio Cable",
    "VoiceMeeter Output",
    "VoiceMeeter AUX Output",
    "OBS Virtual Microphone",
    "Virtual Microphone",
];

/// Ambil daftar nama perangkat input (capture) yang cocok dengan virtual mic.
pub fn list_virtual_inputs() -> Result<Vec<String>> {
    let _guard = ComGuard::new()?;
    let enum_: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.win()?;
    let devices = unsafe { enum_.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE) }.win()?;
    let mut names = Vec::new();
    for i in 0..unsafe { devices.GetCount() }.win()? {
        let dev = unsafe { devices.Item(i) }.win()?;
        if let Some(name) = device_friendly_name(&dev)? {
            if is_virtual_input(&name) {
                names.push(name);
            }
        }
    }
    Ok(names)
}

/// Cek apakah sebuah nama perangkat adalah virtual mic yang kita dukung.
pub fn is_virtual_input(name: &str) -> bool {
    VIRTUAL_MIC_NAMES
        .iter()
        .any(|v| name.to_lowercase().contains(&v.to_lowercase()))
}

/// Ambil friendly name sebuah device endpoint (PKEY_Device_FriendlyName).
fn device_friendly_name(dev: &IMMDevice) -> Result<Option<String>> {
    let store: IPropertyStore = unsafe { dev.OpenPropertyStore(STGM_READ) }.win()?;
    let val = unsafe { store.GetValue(&PKEY_Device_FriendlyName) }.win()?;
    Ok(prop_string(&val))
}

/// Handle WASAPI render aktif ke perangkat input virtual.
pub struct WasapiRender {
    client: IAudioClient,
    render: IAudioRenderClient,
    channels: u16,
}

impl WasapiRender {
    /// Buka perangkat input virtual pertama yang cocok dan mulai stream.
    pub fn open(sample_rate: u32, channels: u16) -> Result<Self> {
        let _guard = ComGuard::new()?;
        let enum_: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.win()?;
        let devices = unsafe { enum_.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE) }.win()?;
        let count = unsafe { devices.GetCount() }.win()?;
        let target = (0..count)
            .filter_map(|i| unsafe { devices.Item(i) }.ok())
            .find(|d| {
                device_friendly_name(d)
                    .ok()
                    .flatten()
                    .is_some_and(|n| is_virtual_input(&n))
            })
            .ok_or_else(|| Error::user(phonecam_core::error::UserErrorCode::VmicNotInstalled))?;
        let client: IAudioClient = unsafe { target.Activate(CLSCTX_ALL, None) }.win()?;
        // B6: PCM 16-bit shared mode; 20 ms buffer.
        let fmt = WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_PCM as u16,
            nChannels: channels,
            nSamplesPerSec: sample_rate,
            nAvgBytesPerSec: sample_rate * channels as u32 * 2,
            nBlockAlign: channels * 2,
            wBitsPerSample: 16,
            cbSize: 0,
        };
        let buffer_100ns: i64 = 200_000; // 20 ms
        unsafe { client.Initialize(AUDCLNT_SHAREMODE_SHARED, 0, buffer_100ns, 0, &fmt, None) }
            .win()?;
        let render: IAudioRenderClient = unsafe { client.GetService() }.win()?;
        unsafe { client.Start() }.win()?;
        tracing::info!(
            target: "phonecam::vmic",
            sample_rate, channels,
            "vmic WASAPI render dimulai"
        );
        Ok(Self {
            client,
            render,
            channels,
        })
    }

    /// Tulis PCM i16_LE ke buffer render (hot path A7: tidak boleh memblokir).
    pub fn write(&mut self, pcm: &[u8]) -> Result<()> {
        let frame_count = (pcm.len() / (self.channels as usize * 2)) as u32;
        if frame_count == 0 {
            return Ok(());
        }
        let data = unsafe { self.render.GetBuffer(frame_count) }.win()?;
        let n = (frame_count as usize) * (self.channels as usize) * 2;
        unsafe {
            std::ptr::copy_nonoverlapping(pcm.as_ptr(), data, n.min(pcm.len()));
        }
        unsafe { self.render.ReleaseBuffer(frame_count, 0) }.win()
    }

    /// Hentikan stream.
    pub fn close(self) -> Result<()> {
        unsafe {
            let _ = self.client.Stop();
        }
        Ok(())
    }
}

/// CoInitializeEx per thread; Uninitialize saat drop (A11: tidak panic).
struct ComGuard {
    need_uninit: bool,
}

impl ComGuard {
    fn new() -> Result<Self> {
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        // S_FALSE / RPC_E_CHANGED_MODE berarti COM sudah aktif di thread ini.
        let need_uninit = hr.is_ok();
        if !need_uninit && hr != windows::Win32::Foundation::RPC_E_CHANGED_MODE {
            return Err(Error::internal("CoInitializeEx gagal"));
        }
        Ok(Self { need_uninit })
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.need_uninit {
            unsafe { CoUninitialize() };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn predicate_nama_virtual() {
        assert!(is_virtual_input("VB-Audio Virtual Cable"));
        assert!(is_virtual_input("VB-Audio VAIO Virtual Cable"));
        assert!(!is_virtual_input("Realtek High Definition Audio"));
    }
}
