/** Status wizard kabel (C4): langkah ke-n, platform HP. */
export interface WizardState {
  platform: "android" | "iphone";
  step: number;
}

/**
 * Wizard kabel (README C4): stepper 3 langkah per jenis HP.
 * Android: Opsi Pengembang + USB debugging, sambungkan kabel, setujui dialog.
 * iPhone: sambungkan kabel, ketuk Percayai komputer, aktifkan Personal Hotspot.
 *
 * ponytail: deteksi otomatis hanya untuk langkah terakhir Android (status
 * `adb.ready` dari /api/connections); langkah lain manual karena tidak ada
 * API web yang bisa membaca setelan HP.
 */
export const ANDROID_STEPS = [
  "Buka Setelan > Tentang ponsel, ketuk Nomor pembuat 7 kali untuk mengaktifkan Opsi Pengembang.",
  "Buka Opsi Pengembang, nyalakan Debugging USB, lalu sambungkan kabel USB ke PC.",
  'Setujui dialog "Izinkan debugging USB" yang muncul di layar HP.',
];

export const IPHONE_STEPS = [
  "Sambungkan kabel USB dari iPhone ke PC.",
  "Ketuk Percayai komputer ini di layar HP, masukkan kode sandi.",
  "Buka Pengaturan > Personal Hotspot, nyalakan toggle-nya agar PC terhubung (172.20.10.x).",
];
