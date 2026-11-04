/**
 * Bentuk state desktop dari endpoint `/api/state` (crates/desktop/src/server.rs).
 * Field wajib cocok dengan serialisasi Rust; `??` memberi default aman bila
 * PC menjalankan versi lebih lama.
 */
export interface DesktopState {
  pin: string;
  pin_remaining_secs: number;
  pair_url: string;
  qr_url: string;
  vcam_installed: boolean;
  sources: SourceRow[];
}

export interface SourceRow {
  id: string;
  name: string;
  connection_kind: string;
  state: string;
  battery: number | null;
  rtt_ms: number | null;
  /** Peran sumber (B6): "active" | "preview". */
  role: "active" | "preview";
}

/** Satu antarmuka jaringan privat dari `/api/connections` (B9.1). */
export interface InterfaceRow {
  name: string;
  addrs: string[];
  kind: "lan" | "tether" | "hotspot";
  /** Label jenis jalur untuk UI (B9.1). */
  kind_label: string;
  /** Prioritas jalur (B9.1: kabel > Wi-Fi; kecil = tinggi). */
  priority: number;
  status: "active" | "needs_setup" | "unavailable";
  /** URL pairing jalur ini (B9.1: IP antarmuka jalur ini). */
  pair_url: string;
}

/** Status binary adb (B9.2); null bila tidak ada di PATH. */
export interface AdbStatus {
  ready: boolean;
  unauthorized: boolean;
  /** URL pairing mode ADB (B9.2: http://localhost:8080). */
  pair_url: string;
}

export interface ConnectionsState {
  interfaces: InterfaceRow[];
  adb: AdbStatus | null;
}
