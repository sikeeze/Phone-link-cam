import type { WireMessage, MsgTypeValue } from "./types";
import { PROTOCOL_VERSION, MsgType } from "./types";

/** Wadah slot untuk pesan masuk (B4). */
export type MessageHandler = (msg: WireMessage) => void;

/** Status koneksi signaling (B3, cerminan SessionState sisi PC). */
export type ConnStatus =
  | { kind: "connecting" }
  | { kind: "connected" }
  | { kind: "reconnecting"; attempt: number; retryInMs: number }
  | { kind: "closed" };

/** Callback perubahan status (B3: UI hanya menampilkan, tidak menyimpan state). */
export type StatusHandler = (s: ConnStatus) => void;

/** Backoff retry eksponensial B3 (cerminan core::session::backoff_delay). */
const BACKOFF_STEPS_MS = [500, 1000, 2000, 4000, 8000];
/** Anggaran total reconnect B3: 60 detik (core RECONNECT_BUDGET). */
const RECONNECT_BUDGET_MS = 60_000;

/**
 * Koneksi WebSocket signaling ke PC (B1, B2, B3).
 *
 * Reconnect otomatis dengan backoff eksponensial B3 bila WS putus tanpa
 * `bye` eksplisit. Token dari `auth_ok` dipakai kembali agar sumber tetap
 * (slot tidak bertambah). Setelah anggaran 60 detik habis, status menjadi
 * `closed` dan UI menampilkan tombol Coba lagi (D5).
 */
export class Signaling {
  private ws: WebSocket | null = null;
  private url: string;
  private handlers: Map<MsgTypeValue, MessageHandler[]> = new Map();
  private statusHandlers: StatusHandler[] = [];
  private closed = false;
  /** Token reconnect dari auth_ok (B2). */
  private token: string | null = null;
  /** Cara membangun payload hello untuk re-auth (B3). */
  private helloFactory: (() => Record<string, unknown>) | null = null;
  private reconnectAttempt = 0;
  private reconnectStartedAt = 0;
  private reconnectTimer: number | null = null;

  constructor(url: string) {
    this.url = url;
    this.on(MsgType.PING, (msg) => this.onPing(msg));
  }


  /** Daftarkan penangan untuk satu jenis pesan (B4). */
  on(type: MsgTypeValue, handler: MessageHandler): void {
    const list = this.handlers.get(type) ?? [];
    list.push(handler);
    this.handlers.set(type, list);
  }

  /** Daftarkan penangan perubahan status koneksi (B3). */
  onStatus(handler: StatusHandler): void {
    this.statusHandlers.push(handler);
  }

  /** Simpan token reconnect + cara membangun payload hello (B2/B3). */
  setReauth(token: string, helloFactory: () => Record<string, unknown>): void {
    this.token = token;
    this.helloFactory = helloFactory;
  }

  private emit(s: ConnStatus): void {
    for (const h of this.statusHandlers) h(s);
  }

  /** Balas `pong` untuk `ping` dari PC (B3: heartbeat 2 detik).
   *
   * `ts` ping di-echo apa adanya sehingga PC bisa mengukur RTT dengan
   * jamnya sendiri (B5) tanpa bergantung pada sinkronisasi jam HP.
   */
  private onPing(msg: WireMessage): void {
    this.sendAt(MsgType.PONG, msg.ts ?? 0);
  }

  /** Kirim pesan dengan `ts` eksplisit (B4: diisi pengirim). */
  private sendAt(type: MsgTypeValue, ts: number): void {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) return;
    const m: WireMessage = { v: PROTOCOL_VERSION, type, ts };
    this.ws.send(JSON.stringify(m));
  }

  /** Buka koneksi. Mengembalikan promise yang selesai saat ws terbuka. */
  connect(): Promise<void> {
    return new Promise((resolve, reject) => {
      this.closed = false;
      this.emit({ kind: "connecting" });
      const ws = new WebSocket(this.url);
      this.ws = ws;
      let settled = false;
      ws.onopen = () => {
        settled = true;
        this.emit({ kind: "connected" });
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
        this.ws = null;
        // B3: hanya reconnect bila belum ditutup pengguna dan sudah pernah
        // auth (token ada). Pairing yang gagal sebelum auth tidak retry.
        if (!this.closed && this.token) {
          this.scheduleReconnect();
        }
      };
    });
  }

  /** Jadwalkan percobaan reconnect berikutnya (B3, cerminan core::session). */
  private scheduleReconnect(): void {
    if (this.reconnectStartedAt === 0) {
      this.reconnectStartedAt = Date.now();
      this.reconnectAttempt = 0;
    }
    const elapsed = Date.now() - this.reconnectStartedAt;
    if (elapsed >= RECONNECT_BUDGET_MS) {
      // B3: anggaran habis -> berhenti; UI menampilkan Coba lagi (D5).
      this.emit({ kind: "closed" });
      return;
    }
    const idx = Math.min(this.reconnectAttempt, BACKOFF_STEPS_MS.length - 1);
    const wait = Math.min(BACKOFF_STEPS_MS[idx], 8000);
    this.emit({ kind: "reconnecting", attempt: this.reconnectAttempt + 1, retryInMs: wait });
    this.reconnectAttempt += 1;
    this.reconnectTimer = window.setTimeout(() => {
      this.reconnectTimer = null;
      void this.reconnect();
    }, wait);
  }

  /** Buka ulang WS dan auth ulang dengan token (B2/B3). */
  private async reconnect(): Promise<void> {
    try {
      await this.connect();
      // B2/B3: hello + auth token; PC mengembalikan source_id yang sama.
      this.send(MsgType.HELLO, this.helloFactory ? this.helloFactory() : {});
      this.send(MsgType.AUTH, { credential: { token: this.token } });
    } catch {
      // Gagal membuka: jadwalkan lagi sampai anggaran habis.
      this.scheduleReconnect();
    }
  }

  /** Coba lagi manual setelah anggaran habis (B3: tombol Coba lagi). */
  retryManually(): void {
    this.reconnectStartedAt = 0;
    this.reconnectAttempt = 0;
    void this.reconnect();
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

  /** Tutup koneksi (tidak menyalahkan reconnect). */
  close(): void {
    this.closed = true;
    if (this.reconnectTimer !== null) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    this.ws?.close();
    this.ws = null;
    this.emit({ kind: "closed" });
  }

  /** Kirim pesan bila koneksi terbuka; abaikan bila tidak (B3 reconnect). */
  trySend(type: MsgTypeValue, payload?: unknown): void {
    try {
      this.send(type, payload);
    } catch {
      // B3: pesan diblokir selama reconnect; battery update berikutnya
      // akan terkirim setelah kembali terhubung.
    }
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
