import { useState, useEffect } from "preact/hooks";

import type { Signaling } from "../signaling";
import { MsgType } from "../types";

interface Props {
  path?: string;
  default?: boolean;
  signaling: Signaling | null;
}

/**
 * Layar siaran kamera (B4, fase 3): pratinjau lokal + indikator status.
 *
 * Fase 0: kerangka. WebRTC send/recv pipeline diisi saat fase 2/3.
 */
export function CameraScreen({ signaling }: Props) {
  const [torchOn, setTorchOn] = useState(false);
  const [status, setStatus] = useState("Menghubungkan…");

  useEffect(() => {
    if (!signaling) {
      setStatus("Tidak terhubung");
      return;
    }
    setStatus("Terhubung");
    signaling.on(MsgType.BYE, () => setStatus("PC memutuskan koneksi"));
  }, [signaling]);

  return (
    <main class="screen camera-screen">
      <video autoplay playsinline muted />
      <div class="status-bar">
        <span class="status">{status}</span>
      </div>
      <div class="controls">
        <button
          type="button"
          class={torchOn ? "active" : ""}
          onClick={() => setTorchOn((v) => !v)}
        >
          {torchOn ? "Senter menyala" : "Senter"}
        </button>
        <button type="button" onClick={() => signaling?.send(MsgType.BYE)}>
          Putuskan
        </button>
      </div>
    </main>
  );
}
