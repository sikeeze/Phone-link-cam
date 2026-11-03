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
