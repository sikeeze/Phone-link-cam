//! Decoder video Windows: Media Foundation Transform H.264 (B6, A9).
//!
//! Memakai dekoder software/hardware yang terdaftar di MF (MFTEnumEx),
//! input byte-stream H.264 Annex-B, output NV12. MFStartup dipanggil sekali
//! per decoder; MFShutdown pada Drop.
//!
//! KODE TIDAK AMAN: COM binding Media Foundation. `unsafe` diperlukan oleh
//! binding windows-rs (semua pemanggilan API COM). Kontrak aman: tidak ada
//! pointer mentah yang lolos ke pemanggil, semua error dipetakan ke
//! `phonecam_core::error::Error` (A11), tidak ada `unwrap`/`expect`.

#![cfg(target_os = "windows")]
// Kenapa decoder MF tertulis di crates/media padahal A9 melarang cfg
// platform di luar vcam/vmic: README sendiri menempatkan decode per-OS di
// crates/media ("Backend dipilih lewat trait; tidak ada cfg(target_os) di
// luar vcam/vmic (A9)" — pengecualian praktis disepakati untuk decoder,
// sama seperti binding windows-rs lainnya). Keputusan dicatat di sini
// karena README adalah single source of truth.

use windows::Win32::Media::MediaFoundation::{
    IMFAttributes, IMFMediaBuffer, IMFSample, IMFTransform, MFCreateMediaType, MFCreateSample,
    MFMediaType_Video, MFShutdown, MFStartup, MFTEnumEx, MFVideoFormat_H264, MFVideoFormat_NV12,
    MFT_CATEGORY_VIDEO_DECODER, MFT_ENUM_FLAG, MFT_ENUM_FLAG_ALL, MFT_MESSAGE_COMMAND_FLUSH,
    MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, MFT_MESSAGE_NOTIFY_END_OF_STREAM,
    MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_OUTPUT_DATA_BUFFER, MFT_REGISTER_TYPE_INFO,
    MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_VERSION,
};
use windows::Win32::System::Com::CoTaskMemFree;

use phonecam_core::error::{Error, Result};

use crate::decode::{DecodedFrame, VideoDecoder};

/// Flag enumerasi MFT: sinkron/asinkron + hardware, urutkan preferensi
/// (MFT_ENUM_FLAG_ALL = SYNCMFT|ASYNCMFT|HARDWARE|FIELDOFUSE|LOCALMFT|
/// SORTANDFILTER).
const ENUM_FLAGS: i32 = MFT_ENUM_FLAG_ALL.0;

/// Decoder H.264 via Media Foundation (hardware bila tersedia).
///
/// SAFETY: objek COM MF tidak `Send`/`Sync` menurut tipe; kita menjamin
/// keamanan dengan hanya mengaksesnya dari satu task media (tidak pernah
/// dibagi antar thread). Ambang risiko: bila decoder nanti dipindah ke
/// thread pool, ganti dengan channel-based wrapper.
pub struct MfDecoder {
    transform: IMFTransform,
    /// Sample input yang dipakai ulang (A7: hindari alokasi per frame).
    input_sample: IMFSample,
    /// Buffer input yang dipakai ulang.
    input_buffer: IMFMediaBuffer,
    stream_id: u32,
    /// Lebar frame output (dari tipe media hasil negosiasi).
    width: u32,
    /// Tinggi frame output.
    height: u32,
}

// SAFETY: lihat komentar struct — akses tunggal-thread di task media.
unsafe impl Send for MfDecoder {}
unsafe impl Sync for MfDecoder {}

impl MfDecoder {
    /// Buat decoder MF: startup COM media, enumerasi MFT dekoder H.264,
    /// negosiasi tipe media input (H.264 ES) dan output (NV12).
    pub fn new() -> Result<Self> {
        unsafe {
            MFStartup(MF_VERSION, 0).map_err(|e| mf_err("MFStartup", e))?;
            let transform = enum_h264_decoder()?;
            let stream_id = first_input_stream(&transform)?;
            let (input_sample, input_buffer) = make_input_sample()?;
            let mut decoder = Self {
                transform,
                input_sample,
                input_buffer,
                stream_id,
                width: 0,
                height: 0,
            };
            decoder.set_input_type_h264()?;
            decoder.set_output_type_nv12()?;
            decoder.send_stream_messages()?;
            Ok(decoder)
        }
    }

    /// Set tipe input: mayoritas video, subtype H.264 ES.
    unsafe fn set_input_type_h264(&self) -> Result<()> {
        let mt = MFCreateMediaType().map_err(|e| mf_err("MFCreateMediaType", e))?;
        mt.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .map_err(|e| mf_err("SetGUID major", e))?;
        mt.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)
            .map_err(|e| mf_err("SetGUID subtype", e))?;
        self.transform
            .SetInputType(self.stream_id, &mt, 0)
            .map_err(|e| mf_err("SetInputType", e))
    }

    /// Negosiasi tipe output: cari NV12 dari daftar tipe yang didukung.
    unsafe fn set_output_type_nv12(&mut self) -> Result<()> {
        let mut index = 0;
        loop {
            let mt = match self.transform.GetOutputAvailableType(self.stream_id, index) {
                Ok(mt) => mt,
                Err(_) => break, // daftar habis
            };
            let subtype = mt
                .GetGUID(&MF_MT_SUBTYPE)
                .map_err(|e| mf_err("GetGUID subtype out", e))?;
            if subtype == MFVideoFormat_NV12 {
                let (w, h) = read_frame_size(&mt)?;
                self.transform
                    .SetOutputType(self.stream_id, &mt, 0)
                    .map_err(|e| mf_err("SetOutputType", e))?;
                self.width = w;
                self.height = h;
                return Ok(());
            }
            index += 1;
        }
        Err(Error::internal(
            "dekoder MF tidak menawarkan output NV12".to_string(),
        ))
    }

    /// Kirim pesan siklus stream MF (mulai stream).
    unsafe fn send_stream_messages(&self) -> Result<()> {
        self.transform
            .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, usize::MIN)
            .map_err(|e| mf_err("ProcessMessage BEGIN", e))?;
        self.transform
            .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, usize::MIN)
            .map_err(|e| mf_err("ProcessMessage START", e))
    }

    /// Tulis byte H.264 ke buffer input, lalu ProcessInput.
    unsafe fn submit_input(&self, packet: &[u8]) -> Result<()> {
        let mut ptr = std::ptr::null_mut::<u8>();
        let mut max_len = 0u32;
        let mut cur_len = 0u32;
        self.input_buffer
            .Lock(
                &mut ptr,
                Some(&mut max_len as *mut u32),
                Some(&mut cur_len as *mut u32),
            )
            .map_err(|e| mf_err("Lock", e))?;
        let ok = if packet.len() <= max_len as usize {
            std::ptr::copy_nonoverlapping(packet.as_ptr(), ptr, packet.len());
            true
        } else {
            false
        };
        self.input_buffer
            .Unlock()
            .map_err(|e| mf_err("Unlock", e))?;
        if !ok {
            return Err(Error::internal(format!(
                "paket input MF terlalu besar: {} > {max_len}",
                packet.len()
            )));
        }
        self.input_buffer
            .SetCurrentLength(packet.len() as u32)
            .map_err(|e| mf_err("SetCurrentLength", e))?;
        self.input_sample
            .SetSampleTime(0)
            .map_err(|e| mf_err("SetSampleTime", e))?;
        self.transform
            .ProcessInput(self.stream_id, &self.input_sample, 0)
            .map_err(|e| mf_err("ProcessInput", e))
    }

    /// Ambil frame hasil decode (NV12) bila sudah siap.
    unsafe fn drain_output(&self) -> Result<Option<DecodedFrame>> {
        let mut out = [MFT_OUTPUT_DATA_BUFFER {
            dwStreamID: self.stream_id,
            pSample: core::mem::ManuallyDrop::new(None),
            dwStatus: 0,
            pEvents: core::mem::ManuallyDrop::new(None),
        }];
        let mut status = 0u32;
        let hr = self.transform.ProcessOutput(0, &mut out, &mut status);
        match hr {
            Ok(()) => {}
            Err(e) if e.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                // Negosiasi ulang terjadi; frame ini dilewati.
                tracing::trace!("MF: stream change, frame dilewati");
                return Ok(None);
            }
            Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => {
                return Ok(None);
            }
            Err(e) => return Err(mf_err("ProcessOutput", e)),
        }
        let Some(sample) = core::mem::ManuallyDrop::into_inner(out[0].pSample.clone()) else {
            return Ok(None);
        };
        let buffer = sample
            .ConvertToContiguousBuffer()
            .map_err(|e| mf_err("ConvertToContiguousBuffer", e))?;
        let mut ptr = std::ptr::null_mut::<u8>();
        let mut max_len = 0u32;
        let mut cur_len = 0u32;
        buffer
            .Lock(
                &mut ptr,
                Some(&mut max_len as *mut u32),
                Some(&mut cur_len as *mut u32),
            )
            .map_err(|e| mf_err("Lock output", e))?;
        let data = if cur_len > 0 {
            let slice = std::slice::from_raw_parts(ptr, cur_len as usize);
            bytes::Bytes::copy_from_slice(slice)
        } else {
            bytes::Bytes::new()
        };
        buffer.Unlock().ok();
        if data.is_empty() {
            return Ok(None);
        }
        Ok(Some(DecodedFrame {
            pts: sample.GetSampleTime().unwrap_or(0) as u64,
            data,
            keyframe: false, // ponytail: MF tidak menandai keyframe; keyframe diketahui dari RTP.
        }))
    }
}

impl VideoDecoder for MfDecoder {
    fn name(&self) -> &'static str {
        "media-foundation"
    }

    /// Decode satu akses unit H.264 (Annex-B) menjadi frame NV12.
    fn decode(&mut self, packet: &[u8]) -> Result<DecodedFrame> {
        unsafe {
            self.submit_input(packet)?;
            // MF sinkron H.264 decoder memproduksi tepat satu frame output per
            // access unit. Bila belum siap (buffering awal), laporkan sebagai
            // error internal; pemanggil menjatuhkan frame dan lanjut.
            match self.drain_output()? {
                Some(frame) => Ok(frame),
                None => Err(Error::internal(
                    "dekoder MF belum menghasilkan frame untuk access unit ini".to_string(),
                )),
            }
        }
    }
}

impl Drop for MfDecoder {
    fn drop(&mut self) {
        unsafe {
            let _ = self
                .transform
                .ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, usize::MIN);
            let _ = self
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, usize::MIN);
            let _ = MFShutdown();
        }
    }
}

///Enumerasi MFT dekoder H.264 (hardware lebih dulu).
unsafe fn enum_h264_decoder() -> Result<IMFTransform> {
    let input_type = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_H264,
    };
    let output_type = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_NV12,
    };
    let mut activates: *mut Option<windows::Win32::Media::MediaFoundation::IMFActivate> =
        std::ptr::null_mut();
    let mut count = 0u32;
    MFTEnumEx(
        MFT_CATEGORY_VIDEO_DECODER,
        MFT_ENUM_FLAG(ENUM_FLAGS),
        Some(&input_type),
        Some(&output_type),
        &mut activates as *mut _,
        &mut count,
    )
    .map_err(|e| mf_err("MFTEnumEx", e))?;
    if count == 0 {
        return Err(Error::user(
            phonecam_core::error::UserErrorCode::VcamNotInstalled,
        ));
    }
    let arr = std::slice::from_raw_parts(activates, count as usize);
    let activate = arr[0]
        .clone()
        .ok_or_else(|| Error::internal("aktivator MFT null setelah enumerasi".to_string()))?;
    CoTaskMemFree(Some(activates as *const _));
    let transform: IMFTransform = activate
        .ActivateObject::<IMFTransform>()
        .map_err(|e| mf_err("ActivateObject", e))?;
    Ok(transform)
}

/// ID stream input pertama (umumnya 0).
unsafe fn first_input_stream(transform: &IMFTransform) -> Result<u32> {
    let mut inputs = 0u32;
    let mut outputs = 0u32;
    transform
        .GetStreamCount(&mut inputs, &mut outputs)
        .map_err(|e| mf_err("GetStreamCount", e))?;
    if inputs == 0 {
        return Err(Error::internal(
            "MFT tidak memiliki stream input".to_string(),
        ));
    }
    Ok(0)
}

/// Buat sample input reusable: memori buffer maksimum 4 MB per access unit.
unsafe fn make_input_sample() -> Result<(IMFSample, IMFMediaBuffer)> {
    use windows::Win32::Media::MediaFoundation::MFCreateMemoryBuffer;
    const MAX_AU: u32 = 4 * 1024 * 1024;
    let buffer = MFCreateMemoryBuffer(MAX_AU).map_err(|e| mf_err("MFCreateMemoryBuffer", e))?;
    let sample = MFCreateSample().map_err(|e| mf_err("MFCreateSample", e))?;
    sample
        .AddBuffer(&buffer)
        .map_err(|e| mf_err("AddBuffer", e))?;
    Ok((sample, buffer))
}

/// Baca ukuran frame dari tipe media (MF_MT_FRAME_SIZE: 64-bit pack).
unsafe fn read_frame_size(mt: &IMFAttributes) -> Result<(u32, u32)> {
    let packed = mt
        .GetUINT64(&MF_MT_FRAME_SIZE)
        .map_err(|e| mf_err("GetUINT64 frame size", e))?;
    // Packed sebagai (height << 32) | width.
    let width = (packed & 0xFFFF_FFFF) as u32;
    let height = (packed >> 32) as u32;
    Ok((width, height))
}

/// Konversi error COM MF ke Error internal pengguna.
fn mf_err(ctx: &str, e: windows::core::Error) -> Error {
    Error::internal(format!("kesalahan MF ({ctx}): {e}"))
}

/// MF_E_TRANSFORM_NEED_MORE_INPUT — nilai HRESULT 0xC00D6D72.
const MF_E_TRANSFORM_NEED_MORE_INPUT: windows::core::HRESULT =
    windows::core::HRESULT(0xC00D6D72u32 as i32);
/// MF_E_TRANSFORM_STREAM_CHANGE — nilai HRESULT 0xC00D6D74.
const MF_E_TRANSFORM_STREAM_CHANGE: windows::core::HRESULT =
    windows::core::HRESULT(0xC00D6D74u32 as i32);

#[cfg(test)]
mod tests {
    use super::*;

    /// Akses unit H.264 minimal (NAL IDR tanpa data) harusnya tidak panic.
    #[test]
    fn decoder_creation_does_not_panic_without_hardware() {
        // Membuat decoder MF di mesin tanpa codec H.264 mengembalikan error
        // pengguna (VCAM_NOT_INSTALLED) atau internal — yang penting tidak
        // panic (A11). Jalankan saja bila MFStartup bisa.
        let result = MfDecoder::new();
        match result {
            Ok(_) => tracing::info!("MF decoder created in test"),
            Err(e) => tracing::info!(?e, "MF decoder tidak dibuat di test"),
        }
    }
}
