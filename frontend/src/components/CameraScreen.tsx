import { useState, useEffect, useRef } from "preact/hooks";

import type { Signaling, ConnStatus } from "../signaling";
import { MsgType, type CameraFacing, type Transport } from "../types";
import { CameraPeer } from "../webrtc";
import { WebCodecsCamera } from "../webcodecs";
import { useBattery, useWakeLock, useSignalBar } from "../hooks";
import { ReconnectOverlay } from "./ReconnectOverlay";

interface Props {
  path?: string;
  default?: boolean;
  signaling: Signaling | null;
  /** Transport media yang dipakai sesi ini (B9.2.4). */
  transport: Transport;
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
export function CameraScreen({ signaling, transport }: Props) {
  const [torchOn, setTorchOn] = useState(false);
  const [status, setStatus] = useState("Menghubungkan…");
  const [facing, setFacing] = useState<CameraFacing>("back");
  const [connStatus, setConnStatus] = useState<ConnStatus | null>(null);
  const [overlayVisible, setOverlayVisible] = useState(true);
  const videoRef = useRef<HTMLVideoElement | null>(null);
  const peerRef = useRef<CameraPeer | null>(null);
  const camRef = useRef<WebCodecsCamera | null>(null);
  const hideTimer = useRef<number | null>(null);

  // D4: indikator baterai + sinyal; Wake Lock aktif selama siaran.
  const { level: batteryLevel, charging } = useBattery();
  const signalBars = useSignalBar();
  const wakeLockOn = useWakeLock(connStatus?.kind === "connected");
  // D4: tampilkan petunjuk bila halaman masuk background (wake lock lepas).
  const [backgrounded, setBackgrounded] = useState(false);
  useEffect(() => {
    const onVis = () => setBackgrounded(document.visibilityState !== "visible");
    document.addEventListener("visibilitychange", onVis);
    return () => document.removeEventListener("visibilitychange", onVis);
  }, []);

  useEffect(() => {
    if (!signaling) {
      setStatus("Tidak terhubung");
      return;
    }
    setStatus("Memulai kamera…");
    if (transport === "ws-webcodecs") {
      // B9.2.4: encode H.264 WebCodecs + kirim biner lewat WS signaling.
      const cam = new WebCodecsCamera(signaling);
      camRef.current = cam;
      cam
        .start()
        .then((stream) => {
          if (videoRef.current) videoRef.current.srcObject = stream;
          setStatus("Siaran aktif");
        })
        .catch((e: unknown) => {
          const msg = e instanceof Error ? e.message : String(e);
          setStatus(`Kamera gagal: ${msg}`);
        });
      signaling.on(MsgType.BYE, () => setStatus("PC memutuskan koneksi"));
      signaling.onStatus((cs) => {
        setConnStatus(cs);
        if (cs.kind === "connected") setStatus("Siaran aktif");
        else if (cs.kind === "reconnecting") setStatus("Menghubungkan ulang…");
        else if (cs.kind === "closed") setStatus("Koneksi terputus");
      });
      return () => {
        cam.close();
        camRef.current = null;
      };
    }
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

    // B3: status koneksi dari signaling -> overlay reconnect (D5).
    signaling.onStatus((cs) => {
      setConnStatus(cs);
      if (cs.kind === "connected") {
        setStatus("Siaran aktif");
      } else if (cs.kind === "reconnecting") {
        setStatus("Menghubungkan ulang…");
      } else if (cs.kind === "closed") {
        setStatus("Koneksi terputus");
      }
    });

    return () => {
      peer.close();
      peerRef.current = null;
    };
  }, [signaling]);

  // D4: kirim level baterai ke PC lewat source_update (B2/D3).
  useEffect(() => {
    if (batteryLevel === null || !signaling) return;
    signaling.trySend(MsgType.SOURCE_UPDATE, { battery_level: batteryLevel });
  }, [batteryLevel, signaling]);

  // D4: overlay auto-hide 3 detik, muncul saat disentuh.
  const showOverlay = () => {
    setOverlayVisible(true);
    if (hideTimer.current !== null) clearTimeout(hideTimer.current);
    hideTimer.current = window.setTimeout(() => setOverlayVisible(false), 3000);
  };

  useEffect(() => {
    showOverlay();
    return () => {
      if (hideTimer.current !== null) clearTimeout(hideTimer.current);
    };
  }, []);

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
    <main
      class="screen camera-screen"
      onTouchStart={showOverlay}
      onClick={showOverlay}
    >
      <video autoplay playsinline muted ref={videoRef} />
      <div class="status-bar">
        <span class="status">{status}</span>
        <span class="indicators" aria-hidden="false">
          {batteryLevel !== null && (
            <span class="battery" title={charging ? "Mengisi daya" : "Baterai"}>
              {charging ? "âš¡" : ""}
              {Math.round(batteryLevel * 100)}%
            </span>
          )}
          <span class="signal" title={`Sinyal ${signalBars}/4`}>
            {"▂".repeat(signalBars)}
            <span class="signal-off">{"▂".repeat(4 - signalBars)}</span>
          </span>
        </span>
      </div>
      <ReconnectOverlay
        status={connStatus}
        onRetry={() => signaling?.retryManually()}
      />
      {backgrounded && (
        <div class="overlay wake-hint" role="status">
          <p>Tetap buka halaman ini agar siaran tidak terhenti.</p>
          <p class="muted">Wake Lock {wakeLockOn ? "aktif" : "lepas"}.</p>
        </div>
      )}
      <div class="controls" hidden={!overlayVisible}>
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
