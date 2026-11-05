import { useState } from "preact/hooks";

import { Signaling } from "../signaling";
import { MsgType } from "../types";
import { webcodecsSupported } from "../webcodecs";
import type { AuthFailCode } from "../types";

interface Props {
  path?: string;
  default?: boolean;
  onPaired: (sig: Signaling) => void;
}

const FAIL_MESSAGES: Record<AuthFailCode, { title: string; hint: string }> = {
  BAD_PIN: {
    title: "PIN salah",
    hint: "PIN yang kamu masukkan tidak cocok dengan yang ditampilkan di PC.",
  },
  LOCKED: {
    title: "Terlalu banyak percobaan",
    hint: "Kamu sudah salah memasukkan PIN 5 kali. Tunggu 60 detik lalu coba lagi.",
  },
  EXPIRED: {
    title: "PIN kedaluwarsa",
    hint: "PIN sudah berlaku lebih dari 5 menit. Minta PIN baru dari PC.",
  },
  SESSION_FULL: {
    title: "Sesi penuh",
    hint: "Sudah ada 6 HP terhubung ke PC ini. Putuskan salah satu HP di PC lalu coba lagi.",
  },
  PERMISSION_DENIED: {
    title: "Izin ditolak",
    hint: "Browser tidak diizinkan mengakses kamera atau mic.",
  },
};

/**
 * Layar pemasangan PIN (B2, C1).
 *
 * Alur: pengguna memasukkan PIN 6 digit -> kirim `auth` -> tunggu
 * `auth_ok` (token reconnect) atau `auth_fail` (kode + saran).
 */
export function PairingScreen({ onPaired }: Props) {
  const [pin, setPin] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [connecting, setConnecting] = useState(false);

  const submit = async () => {
    setError(null);
    setConnecting(true);
    try {
      // ponytail: fase 1 — URL like `wss://192.168.1.24:8443/ws` dari URL saat ini.
      const wsUrl = `${location.protocol === "https:" ? "wss" : "ws"}://${location.host}/ws`;
      const sig = new Signaling(wsUrl);
      await sig.connect();
      sig.on(MsgType.AUTH_OK, () => {
        setConnecting(false);
        onPaired(sig);
      });
      sig.on(MsgType.AUTH_FAIL, (msg) => {
        setConnecting(false);
        const payload = msg.payload as { code: AuthFailCode; retry_after?: number } | undefined;
        const code = payload?.code ?? "BAD_PIN";
        const m = FAIL_MESSAGES[code] ?? FAIL_MESSAGES.BAD_PIN;
        setError(`${m.title}. ${m.hint}`);
      });
      // B4: hello dulu, baru auth. Credential = externally-tagged enum:
      // {"pin": "..."} atau {"token": "..."}.
      sig.send(MsgType.HELLO, {
        device_id: "phone-web",
        name: "HP Web",
        user_agent: navigator.userAgent,
        capabilities: {
          zoom: false,
          torch: false,
          camera_switch: false,
          webcodecs: webcodecsSupported(),
        },
        // B9.2.4: minta ws-webcodecs bila didukung (mode ADB tanpa UDP);
        // PC boleh menolak dengan TRANSPORT_UNSUPPORTED (B9.4).
        transport: webcodecsSupported() ? "ws-webcodecs" : "webrtc",
      });
      sig.send(MsgType.AUTH, { credential: { pin } });
    } catch (e) {
      setConnecting(false);
      setError(`Tidak bisa terhubung ke PC: ${String(e)}`);
    }
  };

  return (
    <main class="screen pairing-screen">
      <h1>Masukkan PIN</h1>
      <p class="subtitle">PIN ditampilkan di layar PC kamu.</p>
      <input
        type="text"
        inputmode="numeric"
        autocomplete="off"
        maxlength={6}
        placeholder="6 digit"
        value={pin}
        onInput={(e) => setPin((e.target as HTMLInputElement).value)}
      />
      <button type="button" onClick={submit} disabled={pin.length !== 6 || connecting}>
        {connecting ? "Menghubungkan…" : "Sambungkan"}
      </button>
      {error && <p class="error">{error}</p>}
    </main>
  );
}
