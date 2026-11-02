import type { WireMessage, MsgTypeValue } from "./types";
import { PROTOCOL_VERSION } from "./types";

/** Wadah slot untuk pesan masuk (B4). */
export type MessageHandler = (msg: WireMessage) => void;

/**
 * Koneksi WebSocket signaling ke PC (B1, B2).
 *
 * Fase 0: kerangka. Hanya handshake URL + retry dasar. Penanganan pesan
 * lengkap (hello, offer, candidate, auth) diisi saat fase 1.
 */
export class Signaling {
  private ws: WebSocket | null = null;
  private url: string;
  private handlers: Map<MsgTypeValue, MessageHandler[]> = new Map();
  private closed = false;

  constructor(url: string) {
    this.url = url;
  }

  /** Daftarkan penangan untuk satu jenis pesan (B4). */
  on(type: MsgTypeValue, handler: MessageHandler): void {
    const list = this.handlers.get(type) ?? [];
    list.push(handler);
    this.handlers.set(type, list);
  }

  /** Buka koneksi. Mengembalikan promise yang selesai saat ws terbuka. */
  connect(): Promise<void> {
    return new Promise((resolve, reject) => {
      this.closed = false;
      const ws = new WebSocket(this.url);
      this.ws = ws;
      let settled = false;
      ws.onopen = () => {
        settled = true;
        resolve();
      };
      ws.onerror = () => {
        if (!settled) {
          settled = true;
          reject(new Error(`gagal membuka WebSocket ke ${this.url}`));
        }
      };
      ws.onmessage = (ev) => this.dispatch(ev.data);
      ws.onclose = () => {
        if (!this.closed) {
          // ponytail: fase 1 —— reconnect dengan backoff B3 lewat core.
          this.ws = null;
        }
      };
    });
  }

  /** Kirim pesan sesuai format wire (B4). */
  send(type: MsgTypeValue, payload?: unknown): void {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
      throw new Error("WebSocket belum terbuka");
    }
    const msg: WireMessage = {
      v: PROTOCOL_VERSION,
      type,
      ts: Date.now(),
    };
    if (payload !== undefined) {
      msg.payload = payload;
    }
    this.ws.send(JSON.stringify(msg));
  }

  /** Tutup koneksi. */
  close(): void {
    this.closed = true;
    this.ws?.close();
    this.ws = null;
  }

  private dispatch(raw: unknown): void {
    let msg: WireMessage;
    try {
      msg = JSON.parse(String(raw)) as WireMessage;
    } catch {
      console.error("pesan signaling bukan JSON valid");
      return;
    }
    if (msg.v !== PROTOCOL_VERSION) {
      console.error(`versi protokol tidak didukung: ${msg.v}`);
      return;
    }
    const list = this.handlers.get(msg.type) ?? [];
    for (const h of list) h(msg);
  }
}
