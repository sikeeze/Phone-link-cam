/**
 * Encoder WebCodecs H.264 di sisi HP (B9.2.4: transport ws-webcodecs).
 *
 * Memakai getUserMedia + VideoEncoder langsung (tanpa RTCPeerConnection):
 * frame di-encode H.264 lalu dikirim sebagai pesan biner WebSocket ke PC,
 * yang men-decode-nya langsung. Dipakai pada mode ADB di mana adb reverse
 * hanya menjangkau TCP (WebRTC butuh UDP).
 *
 * Bila browser tidak mendukung WebCodecs, `supported()` mengembalikan false
 * dan UI menampilkan TRANSPORT_UNSUPPORTED (B9.4) lalu menyarankan
 * USB tethering atau Wi-Fi.
 *
 * ponytail: audio lewat transport ini menyusul (Opus lewat WS biner); saat
 * ini hanya video.
 */
import type { Signaling } from "./signaling";

// B9.2.4: MediaStreamTrackProcessor belum di lib.dom.d.ts TS ini;
// deklarasi minimal sesuai spesifikasi W3C (Chrome sudah mendukung).
declare global {
  interface MediaStreamTrackProcessor<T = VideoFrame> {
    readable: ReadableStream<T>;
  }
  interface MediaStreamTrackProcessorInit {
    track: MediaStreamTrack;
  }
  // eslint-disable-next-line no-var
  var MediaStreamTrackProcessor: {
    prototype: MediaStreamTrackProcessor;
    new (init: MediaStreamTrackProcessorInit): MediaStreamTrackProcessor;
  };
}

/** True bila browser mendukung WebCodecs (B9.2.4). */
export function webcodecsSupported(): boolean {
  return typeof VideoEncoder !== "undefined" && typeof VideoFrame !== "undefined";
}

/**
 * Encoder kamera HP: capture frame lewat MediaStreamTrackProcessor,
 * encode H.264, kirim chunk biner lewat signaling socket.
 */
export class WebCodecsCamera {
  private encoder: VideoEncoder | null = null;
  private stream: MediaStream | null = null;
  private readonly sig: Signaling;
  private running = false;

  constructor(sig: Signaling) {
    this.sig = sig;
  }

  /** Mulai kamera + encoder. Mengembalikan stream untuk <video> lokal. */
  async start(): Promise<MediaStream> {
    this.stream = await navigator.mediaDevices.getUserMedia({
      video: { facingMode: "environment" },
      audio: false, // ponytail: audio menyusul (Opus via WS biner).
    });
    const track = this.stream.getVideoTracks()[0];
    if (!track) throw new Error("tidak ada track video");

    // B9.2.4: H.264, tanpa B-frame, keyframe tiap 1-2 detik (30000/1001 fps
    // basis ~30 fps → keyframe setiap ~60 frame ≈ 2 d).
    this.encoder = new VideoEncoder({
      output: (chunk) => this.onChunk(chunk),
      error: (e) => console.warn("encoder error", e),
    });
    this.encoder.configure({
      codec: "avc1.42E01F", // H.264 Baseline 3.1 (kompabilitas luas).
      width: 1280,
      height: 720,
      bitrate: 4_000_000,
      framerate: 30,
      avc: { format: "avc" },
    });

    // ponytail: MediaStreamTrackProcessor baru di Chrome; frame loop
    // memakai requestVideoFrame sebagai cadangan.
    const processor = new MediaStreamTrackProcessor({ track });
    const reader = processor.readable.getReader();
    this.running = true;
    this.pump(reader);

    return this.stream;
  }

  /** Loop baca frame dari track processor → encode. */
  private async pump(reader: ReadableStreamDefaultReader<VideoFrame>): Promise<void> {
    while (this.running) {
      let frame: VideoFrame;
      try {
        const res = await reader.read();
        if (res.done) break;
        frame = res.value;
      } catch {
        break; // track berakhir / dibaca ulang.
      }
      if (this.encoder && this.encoder.state === "configured") {
        // B9.2.4: tanpa B-frame → encodeNow tidak ada opsi; codec avc
        // baseline sudah tanpa B-frame.
        this.encoder.encode(frame, { keyFrame: false });
      }
      frame.close();
    }
  }

  /** Kirim chunk terencode sebagai pesan biner WS (B9.2.4). */
  private onChunk(chunk: EncodedVideoChunk): void {
    // B4: pesan biner = bytes mentah chunk; ukuran dibatasi server (16 KiB).
    const buf = new Uint8Array(chunk.byteLength);
    chunk.copyTo(buf);
    this.sig.sendBinary(buf.buffer);
  }

  /** Hentikan encoder dan kamera. */
  close(): void {
    this.running = false;
    try {
      this.encoder?.flush().catch(() => {});
      this.encoder?.close();
    } catch {
      // encoder sudah tertutup — aman.
    }
    this.stream?.getTracks().forEach((t) => t.stop());
    this.encoder = null;
    this.stream = null;
  }
}
