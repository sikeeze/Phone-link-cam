# ADR 002: Sambungan Kabel (USB Tethering, Hotspot, ADB) — Fase 3B

Status: Diterima
Tanggal: 2026-11-04
Lingkup: README B9.2, B9.3, B9.4 (Connection modes)

## Konteks

Fase 3B harus menjalankan PhoneCam saat Wi-Fi rumah tidak ada atau tidak
terpercaya, menggunakan jalur kabel USB. Tiga mode bertarget:

1. USB tethering Android (HP berbagi jaringan ke PC; subnet 192.168.42.0/24)
2. USB Personal Hotspot iPhone (subnet 172.20.10.0/28)
3. Mode ADB (HP terhubung adb, browser HP mengakses 127.0.0.1:8080 lewat
   `adb reverse`, tidak butuh Wi-Fi sama sekali)

Ada juga Wi-Fi Direct/Hotspot HP yang dipakai PC sebagai klien — di luar lingkup
fase ini, lihat "Konsekuensi".

## Keputusan

### D1. Binary adb: deteksi PATH, fallback unduh sekali dengan izin eksplisit

Desktop memeriksa `adb` di PATH saat aplikasi mulai. Jika tidak ada dan pengguna
membuka mode ADB, tampilkan dialog: "adb tidak ditemukan. Unduh platform-tools
(~15 MB) dari Google?" Jika setuju, unduh sekali ke direktori data aplikasi,
ekstrak, dan simpat path tersebut untuk sesi berikutnya. Tidak ada binary yang
dibundel di repo atau binary installer.

Alasan: repo tetap kecil; binary adb spesifik OS/arch (Windows/macOS/Linux +
arm64) — membebani build dan rilis; mayoritas pengguna mode ADB sudah punya
platform-tools terpasang (biasanya developer). Izin eksplisit menghindari
unduhan yang tidak diharapkan.

### D2. Sertifikat HTTPS self-signed yang sama untuk semua mode

Tidak ada CA lokal yang dipasang ke HP. Server HTTPS memakai sertifikat
self-signed yang sudah dimiliki (fase pairing). Untuk mode kabel (tethering,
hotspot), pengguna tetap menerima dialog "Sertifikat tidak terpercaya" di
browser HP secara manual — sama seperti Wi-Fi. Safari iOS dan Chrome Android
mampu melanjutkan ke halaman yang tidak terpercaya.

Alasan: memasang CA lokal ke iOS memerlukan profil + Settings → trust, yang
berat secara UX dan harus diulang saat sertifikat berubah. HTTPS tetap
dibutuhkan (WebRTC `getUserMedia` dan Service Worker butuh secure context);
HTTPplain tidak mungkin di browser modern. Mode ADB tidak terkena: `adb reverse`
meneruskan 127.0.0.1, yang diperlakukan localhost sebagai secure context.

## Konsekuensi

- Mode tethering/hotspot tidak butuh kode baru di server — hanya deteksi
  antarmuka (`classify_subnet` sudah ada di `net.rs`) dan label status di UI.
- Mode ADB butuh: deteksi adb, `adb reversetcp:8080 8080` (plus port WebRTC),
  dan QR/pairing ke `127.0.0.1:8080`. `is_private_ipv4` menerima 127/8 (B9.2).
- Wi-Fi Direct/Hotspot HP ke PC di-{ponytail}: butuh mDNS atau DHCP server
  virtual di desktop — cakupan fase berikutnya jika diperlukan.
- Sertifikat: tidak ada perubahan; dokumentasi pengguna wajib menyebut dialog
  manual (lihat README §C).
