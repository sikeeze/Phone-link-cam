# PhoneCam

Turn your Android or iPhone camera into a low-latency virtual webcam for Windows, macOS and Linux. Pair by QR code with no phone app, connect over Wi-Fi, USB cable or hotspot, use the phone as a virtual microphone, and switch between multiple phone cameras. Works with Zoom, Meet, Discord, OBS and browser apps.

> **Status:** early development. Semua fitur di bawah adalah rencana sampai fasenya selesai dan terukur. Angka latensi dan performa adalah **target**, bukan hasil yang sudah terbukti.

> **Untuk AI agent:** baca seluruh README ini sebelum mengerjakan tugas apa pun. Bagian [Panduan untuk AI Agent](#panduan-untuk-ai-agent) adalah sumber kebenaran tunggal. Jika ada yang ambigu, berhenti dan tanya.

## Daftar isi

- [Fitur](#fitur)
- [Cara kerja](#cara-kerja)
- [Jalur koneksi](#jalur-koneksi)
- [Tech stack](#tech-stack)
- [Struktur proyek](#struktur-proyek)
- [Target performa](#target-performa)
- [Risiko yang diketahui](#risiko-yang-diketahui)
- [Panduan untuk AI Agent](#panduan-untuk-ai-agent)
- [Roadmap](#roadmap)
- [Lisensi](#lisensi)

## Fitur

| Area | Fitur | Fase |
|---|---|---|
| Pairing | Desktop menampilkan QR, URL, dan PIN 6 digit. HP cukup membuka website, tanpa install app | 1 |
| Kamera | Kamera depan/belakang, resolusi 480p-1080p, 30/60 fps, mirror/flip/rotate | 1 |
| Virtual camera | Muncul sebagai kamera di Zoom, Meet, Discord, OBS, OmeTV, Umingle, dll | 1 |
| Latensi rendah | Media langsung HP ke PC, target 80-150 ms (Wi-Fi) dan 40-100 ms (kabel) | 0-1 |
| Zoom | Zoom hardware di HP (jika browser mendukung) + zoom digital di desktop | 1 |
| Mic dari HP | Pilih mic laptop atau mic HP, tampil sebagai mic virtual | 2 |
| Stabilitas | Auto reconnect, bitrate adaptif, indikator baterai dan sinyal | 2 |
| Multi-kamera | Beberapa HP dalam satu sesi, switch angle lewat klik/hotkey, transisi cut/fade/slide | 3 |
| Jalur koneksi | Wi-Fi, USB tethering, USB ADB, USB Personal Hotspot, Hotspot | 3B |
| Kontrol jarak jauh | Dari desktop: kualitas, torch, kamera depan/belakang, fokus/exposure | 3 |
| Ekstra (nanti) | Background blur, filter warna, overlay teks/logo | - |
| UI | Motion halus di semua tombol, kartu, dan transisi layar (desktop dan web) | semua |

## Cara kerja

```
[HP Android/iOS - browser]                        [PhoneCam Desktop (Win/Linux/macOS)]
 getUserMedia (kamera+mic)                          |- Web server HTTPS + WebSocket signaling
 buka https://<ip PC>:8443       --WebRTC/UDP-->    |- WebRTC receiver (per HP)
 (mode ADB: http://localhost:8080 via WebSocket)    |- Decoder (hardware bila ada)
                                                    |- Scene switcher / compositor
                                                    |- Virtual Camera  --> Zoom, OmeTV, Umingle, dll
                                                    |- Virtual Mic     --> aplikasi apa pun
```

Prinsip utama:

- Desktop app sekaligus server yang menyajikan website pairing. Tidak butuh server eksternal dan tidak butuh internet.
- Video tidak lewat server perantara: HP mengirim video yang di-encode hardware oleh browser langsung ke desktop, tanpa transcoding, jitter buffer sekecil mungkin.
- Jalur WebRTC memakai kandidat ICE jaringan lokal saja (tanpa STUN/TURN).
- Multi-kamera hemat resource: HP yang tidak aktif hanya mengirim resolusi kecil untuk thumbnail, hanya HP aktif yang di-decode penuh.

## Jalur koneksi

Semua jalur memakai PIN yang sama, state machine yang sama, dan pesan signaling yang sama. Yang berbeda hanya cara HP menjangkau PC dan jalur media. Angka latensi adalah target yang harus diukur.

| Jalur | Cara kerja | HP | OS PC | Target latensi |
|---|---|---|---|---|
| Wi-Fi (LAN) | HP membuka `https://<ip PC>:8443`, media WebRTC lewat UDP | Android, iOS | Semua | 80-150 ms |
| USB tethering (Android) | Aktifkan USB tethering, PC mendapat antarmuka jaringan baru (umumnya 192.168.42.x). Alur sama seperti LAN, lewat kabel | Android | Windows, Linux. macOS tidak didukung bawaan, sarankan mode ADB | 40-100 ms |
| USB ADB (Android) | `adb reverse` meneruskan port HP ke PC, HP membuka `http://localhost:8080`. Media lewat WebSocket + WebCodecs karena `adb reverse` hanya meneruskan TCP | Android dengan USB debugging | Semua | 40-100 ms |
| USB Personal Hotspot (iPhone) | Sambungkan kabel, aktifkan Personal Hotspot, PC mendapat antarmuka jaringan (umumnya 172.20.10.x). Windows butuh Apple Devices atau iTunes untuk driver. HTTPS tetap wajib | iOS | macOS, Windows, Linux (usbmuxd/ipheth) | 40-100 ms |
| Hotspot | PC atau HP menyalakan hotspot. Cocok jika router memblokir antar perangkat | Android, iOS | Semua | 80-150 ms |
| Remote lewat internet | Relay TURN dan signaling milik sendiri. Ditunda ke fase 7 (ada konsekuensi privasi dan biaya server) | Semua | Semua | Tergantung internet |

Tidak dipakai: Bluetooth (bandwidth tidak cukup) dan Wi-Fi Direct (tidak bisa diakses dari browser). PC yang dikabel ke router dengan HP di Wi-Fi 5 GHz tetap jalur LAN biasa dan biasanya paling stabil.

## Tech stack

| Bagian | Pilihan | Alasan |
|---|---|---|
| Core (jaringan, WebRTC, pipeline) | Rust: tokio, axum, webrtc-rs | Ringan, cepat, satu binary lintas OS |
| UI desktop | Tauri 2 (webview bawaan OS) | RAM jauh lebih kecil dibanding Electron, UI web mudah dianimasikan |
| Website pairing di HP | PWA: TypeScript + Vite, Preact atau vanilla | Tanpa install, bundle kecil |
| Animasi | CSS transitions/springs + Motion One, hanya transform dan opacity | 60 fps, hemat CPU/GPU |
| Decode | Hardware: Media Foundation/D3D11 (Win), VideoToolbox (macOS), VAAPI (Linux); fallback software | CPU rendah |

Virtual device per OS:

| OS | Virtual camera | Virtual mic |
|---|---|---|
| Windows | Windows 11: Media Foundation Virtual Camera (`MFCreateVirtualCamera`). Windows 10: DirectShow filter | Driver audio virtual (bundel atau arahkan install) |
| macOS | CoreMediaIO Camera Extension (butuh Apple Developer ID, signing, notarization) | Audio Server plugin (mirip BlackHole) |
| Linux | v4l2loopback (kernel module, install sekali) | Virtual sink/source PipeWire atau PulseAudio |

## Struktur proyek

```
phonecam/
  crates/
    core/         # sesi, signaling, state, Connection Manager
    rtc/          # WebRTC receiver, jitter, adaptasi bitrate
    media/        # decode, scaler, compositor, switcher
    vcam/         # abstraksi virtual camera (win/mac/linux)
    vmic/         # abstraksi virtual mic
  app/            # Tauri UI desktop (TypeScript)
  web/            # PWA pairing untuk HP
  installer/      # skrip packaging per OS
  docs/           # ADR dan catatan pengukuran
```

## Target performa

- Latensi glass-to-glass: 80-150 ms di Wi-Fi 5 GHz (720p30), 40-100 ms lewat kabel.
- CPU desktop: di bawah 5% untuk 1 kamera 720p30 dengan hardware decode, di bawah 10% untuk 1 aktif + 2 preview.
- RAM di bawah 150 MB, installer di bawah 30 MB, cold start di bawah 2 detik, pairing di bawah 5 detik sejak scan QR.
- Cara menjaga latensi: tanpa B-frame, keyframe berkala dan PLI saat loss, jitter buffer minimum, bitrate adaptif 1-6 Mbps, buang frame lama (selalu tampilkan frame terbaru), audio Opus 20 ms.

## Risiko yang diketahui

1. **iOS Safari mewajibkan HTTPS** untuk kamera/mic, jadi `http://192.168.x.x` tidak berfungsi. Opsi: (a) CA lokal yang di-trust lewat profil iOS, (b) domain pairing dengan sertifikat asli yang mengarah ke IP lokal, (c) self-signed dan pengguna menerima peringatan (sering tidak stabil di iOS). Prototipe pakai (c), target akhir (b). Keputusan lewat ADR.
2. **Browser menyamarkan IP lokal dengan mDNS** pada kandidat ICE. Desktop harus bisa me-resolve mDNS atau memakai alamat peer signaling sebagai kandidat.
3. **Layar HP mati menghentikan stream.** Pakai Screen Wake Lock API dan beri petunjuk agar halaman tetap di foreground.
4. **Zoom hardware lewat browser** hanya di sebagian perangkat (Chrome Android). Di iOS pakai zoom digital.
5. **Driver virtual** butuh izin admin/signing dan berbeda per OS. Kerjakan satu OS dulu.
6. **Wi-Fi 2,4 GHz padat** menaikkan latensi. Sarankan 5 GHz atau kabel.
7. **Mode ADB** butuh USB debugging aktif dan browser HP yang mendukung WebCodecs.

---

## Panduan untuk AI Agent

Bagian ini ditulis untuk agent coding. Semua yang ada di sini mengikat.

### A. Aturan wajib

```
ROLE
You are a senior engineer implementing PhoneCam. This README is the single source of truth. Product: a cross-platform desktop app (Windows, Linux, macOS) that turns Android/iOS phone cameras into a low-latency virtual webcam and optional virtual microphone. Phones connect through a web page served by the desktop app (no phone app).

NON-NEGOTIABLE RULES
1. Never invent APIs, crate names, function signatures or version numbers. Verify every dependency against its real docs or source before use. If you cannot verify something, write UNVERIFIED and ask.
2. Never change the stack, protocol or UI tokens without approval. Propose changes as a short ADR in docs/adr/NNNN-title.md and wait.
3. One phase and one task at a time. No bonus features. No refactors outside the current task.
4. Every task ends with: build passes, tests pass, lint and format pass, a short how-to-run note, and measured numbers (latency, CPU, RAM) when the task touches the media path.
5. Never claim something works unless you ran it. Label every claim VERIFIED (ran it), NOT VERIFIED (could not run) or ASSUMED.
6. If the spec is ambiguous or missing a detail, stop and ask ONE concise question with your recommended default. Never guess silently.
7. Hot path rules (media thread): no disk IO, no per-frame logging above trace level, no per-frame heap allocation (use buffer pools), no UI calls, no blocking locks. Always render the newest frame and drop stale frames.
8. Security rules: LAN only, no telemetry, no cloud calls, validate every incoming message against the schema, bound every buffer and queue, rate-limit PIN attempts, never log PINs or tokens.
9. Platform code lives only behind traits in crates/vcam and crates/vmic. No cfg(target_os) outside those crates and the installer.
10. UI must use design tokens only. No hard-coded colors, radii, durations or font sizes in components.
11. Errors: no unwrap/expect in non-test code, typed errors (thiserror), every user-facing error has a code from section B8/B9.4 and a human message.
12. Small commits, conventional commit messages, one logical change each.

WORKFLOW FOR EVERY TASK
Step 1 restate the task and acceptance criteria in your own words. Step 2 list files you will touch. Step 3 implement. Step 4 run build, tests, lint. Step 5 report using the format below.

REPORT FORMAT
- Summary (3 lines max)
- Files changed
- How to run
- Evidence: commands run and their results (VERIFIED / NOT VERIFIED)
- Measurements: latency, CPU, RAM (if relevant)
- Open questions and assumptions

DEFINITION OF DONE
All acceptance criteria of the task pass, no TODO left in shipped code, docs updated, manual test checklist for the affected OS updated.

FORBIDDEN
Disabling tests to make a build pass. Adding dependencies without a one-line justification. Copying code of unknown license. Telemetry. Auto-updating without user consent. Storing media to disk by default.
```

### B. Spesifikasi protokol dan perilaku

#### B1. Jaringan dan endpoint

| Item | Nilai |
|---|---|
| Server HTTPS + WebSocket | port 8443 (cari port kosong berikutnya jika bentrok) |
| Listener HTTP khusus mode ADB | `127.0.0.1:8080`, tidak pernah di-bind ke antarmuka lain |
| Rentang UDP media (ICE) | 40000-40100 |
| Route | `/` (halaman pairing HP), `/assets/*` (statis, di-embed di binary), `/ws` (signaling), `/health` (JSON status, hanya dari LAN) |
| Bind | hanya antarmuka privat (10/8, 172.16/12, 192.168/16, link-local), termasuk subnet tethering dan hotspot. Jangan bind ke publik |
| ICE | hanya kandidat host, tanpa STUN/TURN. Tangani kandidat mDNS (.local) |

#### B2. Pairing dan keamanan

- PIN 6 digit acak, berlaku 5 menit atau sampai dipakai satu kali, bisa di-regenerate dari UI.
- Maksimal 5 percobaan salah per alamat IP, lalu kunci 60 detik (`retryAfter` dikirim ke klien).
- Setelah PIN benar, server mengeluarkan session token acak 128-bit untuk reconnect otomatis selama sesi desktop hidup.
- Maksimal 6 sumber (HP) per sesi. Sumber ke-7 ditolak dengan `SESSION_FULL`.
- Periksa header Origin pada WebSocket. Media memakai DTLS-SRTP.
- QR berisi URL `https://<ip>:<port>/?pin=<PIN>` agar pairing satu scan.

#### B3. State machine sesi (per sumber)

`IDLE` > `AUTHENTICATING` > `CONNECTING` > `LIVE` > `DEGRADED` (bitrate/fps turun) > `RECONNECTING` (retry eksponensial 0,5 s, 1 s, 2 s, 4 s, maks 8 s, total 60 s) > `CLOSED`.

Transisi harus diuji unit. UI desktop dan web HP hanya menampilkan state, tidak menyimpan state sendiri.

#### B4. Pesan signaling (JSON via WebSocket)

| Pesan | Arah | Isi utama |
|---|---|---|
| `hello` | HP > PC | deviceId, nama perangkat, userAgent, kemampuan (zoom, torch, kamera, webcodecs), `transport` (`webrtc` atau `ws-webcodecs`) |
| `auth` | HP > PC | pin atau token |
| `auth_ok` | PC > HP | sourceId, token, preset kualitas awal |
| `auth_fail` | PC > HP | code (`BAD_PIN`, `LOCKED`, `EXPIRED`, `SESSION_FULL`), retryAfter |
| `offer` / `answer` | dua arah | SDP (hanya transport `webrtc`) |
| `ice` | dua arah | kandidat (hanya transport `webrtc`) |
| `source_update` | HP > PC | resolusi, fps, facing, baterai, torch, zoom, nama |
| `cmd_quality` | PC > HP | preset (B5) |
| `cmd_keyframe` | PC > HP | minta keyframe (dipanggil sebelum switch) |
| `cmd_camera` | PC > HP | facing, torch, zoom, fokus/exposure jika didukung |
| `cmd_role` | PC > HP | `active` atau `preview` (menentukan resolusi) |
| `ping` / `pong` | dua arah | tiap 2 detik, timeout 6 detik |
| `bye` | dua arah | alasan |

Semua pesan berbentuk `{ v: 1, type, id, ts, payload }`. Pesan tidak dikenal atau di luar skema diabaikan dan dicatat pada level debug. Ukuran maksimum satu pesan 16 KB.

#### B5. Preset kualitas

| Preset | Resolusi | FPS | Bitrate target |
|---|---|---|---|
| Hemat | 640x480 | 30 | 1 Mbps |
| Seimbang (default) | 1280x720 | 30 | 2,5 Mbps |
| Tinggi | 1920x1080 | 30 | 5 Mbps |
| Halus | 1280x720 | 60 | 4 Mbps |
| Preview (HP non-aktif) | 320x180 | 15 | 250 kbps |

Adaptasi otomatis: turunkan satu tingkat jika packet loss lebih dari 5% atau RTT lebih dari 80 ms selama 3 detik, naikkan jika stabil 10 detik. Jangan pernah mengubah ukuran output virtual camera, karena aplikasi seperti Zoom dapat macet saat renegosiasi.

#### B6. Pipeline media

1. Terima RTP (atau frame WebSocket pada mode ADB), jitter buffer minimum, decode (hardware dulu, software fallback).
2. Frame masuk ke ring buffer kecil (maksimal 2 frame), selalu ambil yang terbaru.
3. Compositor: scale ke ukuran output tetap (default 1280x720 30 fps, opsi 1920x1080), mode fit (letterbox) atau fill (crop), transform mirror/rotate/zoom digital.
4. Switcher: cut, crossfade 200 ms, slide 250 ms. Sebelum switch kirim `cmd_keyframe` dan tunggu keyframe (maks 500 ms), baru ganti.
5. Output ke virtual camera dengan format konstan (NV12 atau YUY2 sesuai backend). Jika tidak ada sumber: frame hitam atau placeholder PhoneCam.
6. Audio: Opus 48 kHz, 20 ms. `echoCancellation` dan `autoGainControl` dimatikan secara default di HP, `noiseSuppression` opsional. Sumber mic: mic laptop atau salah satu HP. Output ke virtual mic dengan buffer kecil (tambahan latensi target di bawah 40 ms), sinkron dengan video.

#### B7. Hotkey switcher

Saat aplikasi fokus: tombol 1-6 memilih sumber. Global (opsional, bisa dimatikan): Ctrl+Alt+1 sampai 6. Panah kiri/kanan mengganti ke sumber sebelumnya/berikutnya. Semua hotkey bisa diubah di Pengaturan.

#### B8. Kode error pengguna

`BAD_PIN`, `LOCKED`, `EXPIRED`, `SESSION_FULL`, `PERMISSION_DENIED`, `NO_CAMERA`, `NETWORK_LOST`, `VCAM_NOT_INSTALLED`, `VMIC_NOT_INSTALLED`, `FIREWALL_BLOCKED`, `CERT_UNTRUSTED`. Setiap kode punya judul, penjelasan satu kalimat, dan satu aksi perbaikan di UI.

#### B9. Mode koneksi (jalur transport)

Tabel jalur ada di bagian [Jalur koneksi](#jalur-koneksi).

**B9.1 Connection Manager**

- Saat start dan tiap 3 detik: enumerasi antarmuka jaringan privat dan perangkat USB, tampilkan sebagai daftar jalur dengan status `Aktif`, `Terdeteksi`, `Perlu setup`, atau `Tidak tersedia`.
- Server bind ke semua antarmuka privat yang terdeteksi. Listener HTTP `127.0.0.1:8080` khusus mode ADB.
- Satu QR dan satu URL per jalur, URL memakai IP antarmuka jalur itu.
- Prioritas otomatis: kabel, lalu Wi-Fi 5 GHz, lalu hotspot. Opsi Pindah otomatis jika koneksi putus (default mati). Saat pindah, sesi lanjut dengan token yang sama dan sumber melakukan reconnect (B3).
- Ukur RTT per jalur setelah terhubung dan tampilkan badge kualitas: Baik (di bawah 30 ms), Cukup (30-80 ms), Buruk (di atas 80 ms).
- Satu sesi boleh mencampur jalur, misalnya HP A lewat kabel dan HP B lewat Wi-Fi.

**B9.2 Mode ADB**

1. Binary adb: bundel platform-tools resmi (lisensi Apache 2.0) atau deteksi adb di PATH. Keputusan lewat ADR. Jangan mengunduh apa pun tanpa sepengetahuan pengguna.
2. Deteksi dengan `adb devices` tiap 3 detik. Status unauthorized menghasilkan `ADB_UNAUTHORIZED` dengan petunjuk menyetujui dialog di HP.
3. Setelah perangkat terdeteksi jalankan `adb reverse tcp:8080 tcp:8080`. Halaman HP dibuka di `http://localhost:8080` (Chrome menganggap localhost sebagai secure context, jadi tanpa masalah sertifikat). Membuka halaman otomatis lewat `adb shell am start` hanya jika pengguna menekan tombolnya.
4. Transport `ws-webcodecs`: HP meng-encode H.264 dengan WebCodecs (latensi rendah, tanpa B-frame, keyframe tiap 1-2 detik) dan mengirim lewat WebSocket binary, PC men-decode. Jika browser tidak mendukung WebCodecs, tampilkan `TRANSPORT_UNSUPPORTED` dan sarankan USB tethering atau Wi-Fi.
5. Hapus forwarding saat sesi berakhir dan saat aplikasi ditutup (`adb reverse --remove`).
6. Keamanan: listener localhost tidak butuh sertifikat tetapi PIN tetap wajib, tolak Host header selain `localhost` dan `127.0.0.1`.

**B9.3 Tethering dan hotspot**

Tidak ada transport baru, hanya deteksi antarmuka baru dan penampilan IP-nya. Kenali subnet umum (192.168.42.0/24 Android, 172.20.10.0/28 iPhone) tetapi jangan bergantung pada nama antarmuka: terima semua subnet privat. Untuk iPhone, strategi sertifikat HTTPS yang sama dengan Wi-Fi berlaku.

**B9.4 Kode error tambahan**

`USB_NOT_DETECTED`, `ADB_MISSING`, `ADB_UNAUTHORIZED`, `TETHER_NO_IP`, `TRANSPORT_UNSUPPORTED`, `PORT_FORWARD_FAILED`.

Catatan: kabel menghilangkan variasi Wi-Fi tetapi tidak otomatis menjamin latensi lebih rendah. Ukur setiap jalur.

### C. Spesifikasi UI aplikasi desktop

#### C1. Design tokens

| Token | Gelap (default) | Terang |
|---|---|---|
| bg | `#0B0D12` | `#F6F7FA` |
| surface | `#12151C` | `#FFFFFF` |
| surface-2 | `#1A1E28` | `#EEF0F5` |
| border | `#262B38` | `#E3E6EE` |
| text | `#E8EAF0` | `#12151C` |
| text-muted | `#8A91A5` | `#5B6275` |
| accent | `#6C5CFF` | `#5B4BF0` |
| live (aktif/terhubung) | `#22D3A6` | `#0FA37F` |
| warn | `#F5A524` | `#C47F0A` |
| danger | `#F04438` | `#D12C20` |

- Font: Inter (fallback system-ui), angka dan IP pakai JetBrains Mono. Ukuran: 12, 14 (body), 16, 20, 28. Bobot: 400 dan 600 saja.
- Radius: 8 (kontrol), 12 (kartu), 20 (panel besar), 999 (pill).
- Spasi: kelipatan 4 (4, 8, 12, 16, 24, 32).
- Durasi: fast 120 ms, base 200 ms, slow 320 ms. Easing standar `cubic-bezier(0.22, 1, 0.36, 1)`. Spring: stiffness 400, damping 30.
- Ikon: satu set outline (misalnya Lucide), stroke 1,75 px, ukuran 16/20/24.
- Tema: ikut sistem, bisa diganti manual.

#### C2. Komponen dan motion

| Komponen | Perilaku motion |
|---|---|
| Tombol | Hover: naik 1 px dan terang 4% (fast). Tekan: scale 0,97. Fokus keyboard: ring accent 2 px. Loading: spinner menggantikan ikon tanpa lompatan lebar |
| Toggle | Knob bergeser dengan spring, warna track crossfade (base) |
| Kartu kamera | Muncul fade + naik 8 px (base), urutan berubah memakai layout animation, hover menaikkan border ke accent 40% |
| Kamera aktif | Ring live 2 px dengan denyut lembut (opacity 0,6 ke 1, 2 detik) |
| Status pill | Titik berdenyut: hijau terhubung, kuning menurun, merah terputus; teks crossfade |
| Toast | Masuk dari bawah kanan (slow), keluar fade, otomatis hilang 4 detik, pause saat hover |
| Dialog | Backdrop fade, panel scale 0,96 ke 1 + fade (base) |
| Pindah layar | Crossfade + geser horizontal 12 px (base), indikator sidebar meluncur ke item baru |
| Slider | Thumb membesar saat drag, nilai muncul di tooltip |
| Skeleton | Shimmer lambat 1,6 detik saat memuat |

Aturan: animasikan hanya `transform` dan `opacity`. Hormati `prefers-reduced-motion` (ganti dengan fade 80 ms). Target 60 fps. Preview video tidak boleh ikut dianimasi.

#### C3. Layout dan layar

Jendela default 1100x720, minimum 900x600. Sidebar kiri 72 px (ikon: Beranda, Kamera, Studio, Pengaturan), status pill dan versi di bagian bawah sidebar.

1. **Beranda (pairing).** Selector segmented Wi-Fi / Kabel USB / Hotspot di bawah judul. Kartu besar: QR 220 px, URL (mono, tombol salin), PIN 6 digit besar dengan hitung mundur melingkar dan tombol Buat PIN baru. Di bawahnya daftar HP terhubung (kosong: ilustrasi sederhana dan teks Pindai QR dengan HP kamu). Banner kecil jika virtual camera belum terpasang, dengan tombol Pasang.
2. **Kamera.** Grid kartu (2-3 kolom). Tiap kartu: thumbnail live 16:9, nama perangkat (bisa diedit), baterai, sinyal, preset kualitas, tombol Jadikan aktif, menu titik tiga (putuskan, ganti nama, kontrol jarak jauh).
3. **Studio.** Kiri atas preview output besar (16:9) dengan label LIVE. Kanan panel kontrol (zoom, mirror, rotate, fit/fill, transisi, durasi transisi, pilihan mic). Bawah baris switcher berisi thumbnail sumber bernomor 1-6, kamera aktif diberi ring live. Indikator latensi dan fps kecil di pojok preview.
4. **Pengaturan.** Umum (tema, bahasa, mulai bersama sistem), Video (resolusi output, fps, fit/fill), Audio (sumber mic, noise suppression), Jaringan (port, antarmuka, rentang UDP), Hotkey, Tentang. Perubahan diterapkan langsung tanpa tombol Simpan, ditandai toast kecil.
5. **Onboarding pertama kali.** Tiga langkah: pasang virtual camera/mic, izinkan firewall, tes dengan HP. Progress dot, tombol Lanjut dan Lewati.
6. **Keadaan kosong dan error.** Semua error memakai kode B8/B9.4: ikon, judul, satu kalimat, satu tombol aksi.

Teks antarmuka: bahasa Indonesia dan Inggris, kalimat pendek, tanpa jargon teknis di layar utama.

#### C4. Layar Jalur koneksi

- **Layar Koneksi:** grid kartu 2x2 (Wi-Fi, Kabel USB Android, Kabel USB iPhone, Hotspot). Tiap kartu: judul, badge status (`Aktif`, `Terdeteksi`, `Perlu setup`, `Tidak tersedia`), satu kalimat penjelasan, label Target latensi (bukan klaim hasil), dan satu tombol aksi (Gunakan, Siapkan, atau Panduan). Kartu terpilih berborder accent. Di bawahnya toggle Pindah otomatis jika koneksi putus dan urutan prioritas jalur.
- **Wizard kabel:** stepper 3 langkah per jenis HP. Android: aktifkan Opsi Pengembang dan USB debugging, sambungkan kabel, setujui dialog di HP. iPhone: sambungkan kabel, ketuk Percayai komputer ini, aktifkan Personal Hotspot. Progress dot dan tombol Lanjut.
- **Motion:** saat perangkat kabel terdeteksi, ikon kabel berdenyut satu kali dan toast "Pixel 8 terdeteksi lewat kabel" masuk dari kanan bawah. Kartu jalur naik 1 px saat hover, badge status crossfade saat berubah. Indikator selector segmented meluncur ke pilihan baru (base).
- **Daftar HP terhubung:** tiap baris diberi tag jalur (Wi-Fi, USB, Hotspot) dan badge kualitas RTT.

### D. Spesifikasi UI website di HP (PWA)

Pakai token warna dan durasi yang sama dengan desktop. Mobile-first, lebar 360-430 px, area sentuh minimal 48 px, hormati safe area (`env(safe-area-inset-*)`), portrait dan landscape.

1. **Pair.** Logo kecil, judul Hubungkan ke PC, 6 kotak PIN (auto-fokus, tempel didukung, langsung terisi jika datang dari QR). Tombol Hubungkan penuh lebar. PIN salah: kotak bergetar horizontal 240 ms dan pesan satu baris.
2. **Izin.** Penjelasan singkat mengapa kamera/mic dibutuhkan, tombol Izinkan. Jika ditolak: panduan singkat membuka izin di Safari atau Chrome.
3. **Pengaturan awal.** Kamera depan/belakang (segmented control), preset kualitas, toggle Kirim mic, nama perangkat. Preview lokal kecil dengan transisi saat ganti kamera.
4. **Streaming.** Preview mengisi layar, overlay minimal: status pill, baterai, indikator sinyal, tombol Stop (tahan 1 detik atau konfirmasi), zoom (1x, 2x, slider), ganti kamera, torch jika didukung. Overlay auto-hide 3 detik, muncul saat disentuh. Wake Lock aktif, tampilkan petunjuk jika halaman masuk background.
5. **Terhubung ulang / error.** Overlay spinner dan pesan, hitung mundur retry, tombol Coba lagi. Kode error sama seperti B8/B9.4 dengan kalimat ramah.

Motion web sama dengan C2 untuk tombol, toggle, toast, dan transisi layar. Transisi preview ke layar penuh memakai scale + fade 200 ms. Ukuran bundle di bawah 150 KB gzip, tanpa framework berat, tanpa gambar eksternal.

Mode kabel (ADB): halaman HP dibuka di `http://localhost:8080` dengan tampilan yang sama, status pill diberi tag Kabel. Jika browser tidak mendukung WebCodecs, tampilkan `TRANSPORT_UNSUPPORTED` dengan saran memakai Wi-Fi atau USB tethering.

### E. Prompt per fase

Berikan satu per satu setelah aturan di bagian A dipatuhi.

**Phase 0 - Latency spike**

```
PHASE 0 - LATENCY SPIKE
Goal: prove glass-to-glass latency before building anything else.
Build: Rust axum HTTPS server (self-signed cert), WebSocket signaling per section B4 (hello, auth, offer, answer, ice only), minimal web page that sends getUserMedia video via WebRTC, a desktop window that shows the received video.
Measure: phone shows a running millisecond clock; point the phone camera at the desktop screen showing both the clock and the received video; compute the difference over 30 samples. Report median and p95 for 480p30, 720p30, 1080p30 on 5 GHz Wi-Fi.
Acceptance: works on one Android phone and one iPhone; median at 720p30 under 150 ms or a written list of causes and fixes; no crash during a 10 minute run.
Out of scope: UI polish, virtual camera, audio.
```

**Phase 1 - Single camera MVP**

```
PHASE 1 - SINGLE CAMERA MVP
Goal: one phone appears as a virtual camera in a real call app.
Build: PIN + QR pairing (B2), session state machine (B3) with unit tests, desktop UI screens Home and Studio (single source) per section C using tokens and the motion table, phone PWA screens Pair, Permission, Setup, Streaming (section D), camera facing, presets (B5), mirror/rotate, digital zoom, virtual camera backend for ONE operating system first (use the OS named in the task).
Acceptance: phone pairs in under 5 s from QR scan; camera appears in Zoom, Google Meet and OBS; output format stays constant when quality changes; 3 wrong PINs show the shake animation and 5 lock for 60 s; reduced-motion mode works; CPU under 5% at 720p30 with hardware decode (report numbers).
Out of scope: audio, multiple phones, connection modes other than Wi-Fi.
```

**Phase 2 - Audio and stability**

```
PHASE 2 - AUDIO AND STABILITY
Goal: usable for long calls.
Build: Opus audio from phone, mic source selection (laptop or phone), virtual mic backend for the same OS, A/V sync, auto-reconnect with exponential backoff (B3), adaptive quality (B5), battery and signal indicators, Wake Lock on the phone page.
Acceptance: audio delay added by PhoneCam under 40 ms (report), lip-sync visibly correct, Wi-Fi off for 10 s then on recovers without user action, 60 minute soak test with no memory growth above 5%.
```

**Phase 3 - Multi camera**

```
PHASE 3 - MULTI CAMERA
Goal: switch angles between several phones.
Build: up to 6 sources, cmd_role active/preview with low-res previews (B5), switcher with cut/crossfade/slide, keyframe-before-switch (B6), hotkeys (B7), Cameras screen, remote controls (cmd_camera), rename devices.
Acceptance: 3 phones connected; switching shows no frozen frame, green flash or resolution jump; CPU under 10% with 1 active + 2 preview (report); disconnecting the active phone falls back to the previous source or placeholder without crashing the virtual camera.
```

**Phase 3B - Connection modes (kabel dan lainnya)**

```
PHASE 3B - CONNECTION MODES
Goal: add selectable connection paths as specified in section B9.
Build: Connection Manager (network interface and USB device enumeration, per-path status, one QR and URL per path, priority order, optional auto-failover that is OFF by default); USB tethering and USB hotspot support (new interface detection only, same WebRTC transport); ADB mode (adb detection, adb reverse lifecycle, HTTP listener bound to 127.0.0.1 only, transport ws-webcodecs with H.264 WebCodecs on the phone and decode on the desktop); new error codes from B9.4; the Connection screen from section C4; RTT-based quality badge.
Ask first (write an ADR and wait): bundle adb or require it installed; iOS HTTPS certificate strategy.
Acceptance: each path pairs and streams on at least one real phone; hello reports the transport; the same PIN flow works on every path; unplugging the cable moves the source to RECONNECTING and replugging recovers within 10 s; the 127.0.0.1 listener is unreachable from another machine (automated test); adb reverse is removed on session end and on exit (test); latency is measured per path and reported against the B9 targets; the Wi-Fi path has no regression.
Out of scope: remote internet mode.
```

**Phase 4 - All operating systems**

```
PHASE 4 - ALL OPERATING SYSTEMS
Goal: Windows, Linux, macOS complete.
Build: remaining vcam/vmic backends (Windows 11 Media Foundation virtual camera with DirectShow fallback for Windows 10, macOS CoreMediaIO Camera Extension, Linux v4l2loopback and PipeWire/Pulse virtual mic), onboarding screen, driver installer helpers, per-OS manual test checklist.
Acceptance: checklist passes on each OS; clear in-app message with a fix action when a driver is missing (VCAM_NOT_INSTALLED, VMIC_NOT_INSTALLED).
```

**Phase 5-6 - Polish and release**

```
PHASE 5-6 - POLISH AND RELEASE
Build: finish all motion per C2, accessibility (keyboard navigation, focus rings, contrast AA, reduced motion), performance pass to reach the budgets in the Target performa section (CPU, RAM under 150 MB, installer under 30 MB, cold start under 2 s), installers (MSI or NSIS, DMG, AppImage and deb), signing and notarization notes, update check with user consent, user docs and troubleshooting (firewall, Wi-Fi client isolation, iOS HTTPS certificate steps).
Acceptance: every budget has a measured number in the release notes; fresh-machine install test on each OS.
```

### F. Daftar cek uji manual

- **Pairing:** scan QR, ketik PIN manual, PIN salah, PIN kedaluwarsa, sesi penuh.
- **Kamera:** depan/belakang, rotasi HP, ganti preset saat live, zoom, torch, mirror.
- **Aplikasi tujuan:** Zoom, Google Meet, Discord, OBS, browser (OmeTV, Umingle) memilih PhoneCam sebagai kamera dan mic.
- **Jaringan:** matikan Wi-Fi 10 detik, ganti dari 5 GHz ke 2,4 GHz, firewall menolak, AP isolation aktif (harus muncul pesan yang jelas).
- **Kabel:** cabut dan colok kabel saat live, USB debugging belum disetujui, adb tidak ada, tethering tanpa IP, iPhone belum Percayai komputer.
- **Multi-kamera:** 3 HP, switch cepat 20 kali, cabut HP aktif, tambah HP baru saat live, campur jalur (satu kabel, satu Wi-Fi).
- **Sumber daya:** CPU, RAM, suhu HP setelah 30 menit streaming, penurunan baterai HP.
- **UI:** mode gelap/terang, reduced motion, resize jendela ke ukuran minimum, keyboard-only, layar HP kecil (360 px) dan besar (430 px).

#### F.1 Daftar cek per-OS (Phase 4)

Tes dijalankan per OS. Target: Windows 10/11 di sini; Linux dan macOS menunggu mesin target.

- **Windows (OBS Virtual Camera):**
  - [ ] `GET /api/devices` melaporkan `vcam: installed` setelah OBS VC dipasang; `not_installed` sebelumnya.
  - [ ] Banner onboarding muncul saat `vcam_installed=false`, tombol Pasang memunculkan instruksi terstruktur (judul/penjelasan/aksi), bukan crash.
  - [ ] Dengan OBS VC terpasang: Zoom, Google Meet, Discord, OBS, dan browser (OmeTV, Umingle) melihat kamera "PhoneCam".
  - [ ] Virtual mic (WASAPI) muncul sebagai input di aplikasi tujuan; `VMIC_NOT_INSTALLED` bila tidak ada.
  - [ ] Tray: tutup window menyembunyikan ke tray (proses tetap hidup), menu Tampilkan memunculkan kembali, menu Keluar menutup aplikasi.
  - [ ] Firewall menolak koneksi HTTPS: pesan `FIREWALL_BLOCKED` muncul, bukan crash.
  - [ ] Cold start di bawah 2 detik (B2), RAM di bawah 150 MB saat idle (section D budget).
- **Linux (rencana):** v4l2loopback untuk vcam, PipeWire/Pulse virtual sink untuk vmic. Checklist sama dengan Windows, tambah: modul `v4l2loopback` tidak terpasang -> pesan instalasi jelas.
- **macOS (rencana):** CoreMediaIO Camera Extension untuk vcam, Core Audio virtual device untuk vmic. Checklist sama, tambah: izin Screen Recording dan Camera Extension install flow.


## Roadmap

| Fase | Hasil | Kriteria selesai |
|---|---|---|
| 0. Spike | HP kirim video via WebRTC ke desktop, tampil di jendela | Latensi terukur di bawah 150 ms |
| 1. MVP | Pairing QR/PIN, 1 kamera, virtual camera di 1 OS | Muncul dan jalan di Zoom |
| 2. Audio dan stabilitas | Mic HP, virtual mic, auto reconnect, bitrate adaptif | Audio sinkron, stabil 1 jam |
| 3. Multi-kamera | Banyak HP, switcher, hotkey, transisi, kontrol jarak jauh | 3 HP jalan, switch tanpa glitch |
| 3B. Mode koneksi | USB tethering, USB ADB, Personal Hotspot, Hotspot, Connection Manager | Tiap jalur terukur dan lolos kriteria fase |
| 4. Lintas OS | Windows, Linux, macOS lengkap dengan signing | Semua OS lolos uji manual |
| 5. Polish | Motion, aksesibilitas, optimasi CPU/RAM | Target performa tercapai |
| 6. Rilis | Installer, auto-update, dokumentasi, beta | Uji beta dengan pengguna nyata |
| 7. Opsional | Remote lewat internet (relay), background blur, overlay | Keputusan terpisah |

## Lisensi

Belum ditentukan. Pilih lisensi (misalnya MIT atau Apache-2.0) sebelum repo dipublikasikan, dan cek lisensi setiap dependensi, termasuk adb (Apache 2.0) bila dibundel.
