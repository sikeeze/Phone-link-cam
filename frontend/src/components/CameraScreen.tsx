import { useState, useEffect, useRef } from "preact/hooks";

import type { Signaling } from "../signaling";
import { MsgType, type CameraFacing } from "../types";
import { CameraPeer } from "../webrtc";

interface Props {
  path?: string;
  default?: boolean;
  signaling: Signaling | null;
}

/**
 * Layar siaran kamera (B4/B5/B6).
 *
 * Mulai getUserMedia + nego WebRTC saat masuk layar; tombol Balikkan kamera
 * ganti facing tanpa renegosiasi penuh (B5). Status koneksi memakai
 * iceconnectionstate karena heartbeat dipantau di signaling.
 * ponytail: torch/zoom preset memakai track capabilities — native toggle
 * tanpa UI preset B5 lanjutan (fase 3).
 */
export function CameraScreen({ signaling }: Props) {
  const [torchOn, setTorchOn] = useState(false);
  const [status, setStatus] = useState("Menghubungkan…");
  const [facing, setFacing] = useState<CameraFacing>("back");
  const videoRef = useRef<HTMLVideoElement | null>(null);
  const peerRef = useRef<CameraPeer | null>(null);

  useEffect(() => {
    if (!signaling) {
      setStatus("Tidak terhubung");
      return;
    }
    setStatus("Memulai kamera…");
    const peer = new CameraPeer(signaling);
    peerRef.current = peer;
    peer
      .start("back")
      .then((stream) => {
        if (videoRef.current) {
          videoRef.current.srcObject = stream;
        }
        setStatus("Siaran aktif");
      })
      .catch((e: unknown) => {
        const msg = e instanceof Error ? e.message : String(e);
        setStatus(`Kamera gagal: ${msg}`);
      });
    signaling.on(MsgType.BYE, () => setStatus("PC memutuskan koneksi"));

    return () => {
      peer.close();
      peerRef.current = null;
    };
  }, [signaling]);

  /** Beralih kamera depan/belakang (B5: facingMode). */
  const flipCamera = () => {
    const next: CameraFacing = facing === "back" ? "front" : "back";
    const peer = peerRef.current;
    if (!peer) return;
    peer
      .flip(next)
      .then((stream) => {
        if (videoRef.current) videoRef.current.srcObject = stream;
        setFacing(next);
      })
      .catch((e: unknown) => {
        const msg = e instanceof Error ? e.message : String(e);
        setStatus(`Gagal ganti kamera: ${msg}`);
      });
  };

  /** Nyalakan/matikan senter bila track mendukung (B5). */
  const toggleTorch = () => {
    const track = peerRef.current?.activeTrack();
    if (!track) return;
    const cap = track.getCapabilities?.() as MediaTrackCapabilities & {
      torch?: boolean;
    };
    if (!cap?.torch) return;
    const next = !torchOn;
    track
      .applyConstraints({ advanced: [{ torch: next } as MediaTrackConstraintSet] })
      .then(() => setTorchOn(next))
      .catch(() => setStatus("Senter tidak didukung"));
  };

  return (
    <main class="screen camera-screen">
      <video autoplay playsinline muted ref={videoRef} />
      <div class="status-bar">
        <span class="status">{status}</span>
      </div>
      <div class="controls">
        <button
          type="button"
          class={torchOn ? "active" : ""}
          onClick={toggleTorch}
        >
          {torchOn ? "Senter menyala" : "Senter"}
        </button>
        <button type="button" onClick={flipCamera}>
          {facing === "back" ? "Balikkan" : "Depan"}
        </button>
        <button type="button" onClick={() => signaling?.send(MsgType.BYE)}>
          Putuskan
        </button>
      </div>
    </main>
  );
}
