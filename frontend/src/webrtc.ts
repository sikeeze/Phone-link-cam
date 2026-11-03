import type { Signaling } from "./signaling";
import type { WireMessage, CameraFacing } from "./types";
import { MsgType } from "./types";

/**
 * Peer connection WebRTC di sisi HP (offerer) — README B6.
 *
 * Mengambil aliran kamera lewat getUserMedia, membuat offer, mengirimkannya
 * lewat signaling, lalu menerapkan answer + kandidat ICE dari desktop.
 * ponytail: tidak ada restart ICE / rollback offer — fase 1 single camera.
 */
export class CameraPeer {
  private pc: RTCPeerConnection | null = null;
  private stream: MediaStream | null = null;
  private readonly sig: Signaling;

  constructor(sig: Signaling) {
    this.sig = sig;
    sig.on(MsgType.ANSWER, (msg) => this.onAnswer(msg));
    sig.on(MsgType.ICE, (msg) => this.onRemoteIce(msg));
    sig.on(MsgType.CMD_ROLE, (msg) => this.onCmdRole(msg));
    sig.on(MsgType.CMD_CAMERA, (msg) => this.onCmdCamera(msg));
    sig.on(MsgType.CMD_KEYFRAME, (msg) => this.onCmdKeyframe(msg));
  }

  /**
   * Mulai kamera dan negosiasi. `facing` = "front" | "back" (B5).
   * Mengembalikan stream untuk elemen <video> lokal.
   */
  async start(facing: CameraFacing): Promise<MediaStream> {
    this.stream = await navigator.mediaDevices.getUserMedia({
      video: { facingMode: facing === "back" ? "environment" : "user" },
      audio: false, // ponytail: audio menyusul di fase 2 (Opus + vmic).
    });
    this.pc = new RTCPeerConnection({
      iceServers: [], // B1: hanya kandidat host, tanpa STUN/TURN.
    });
    for (const track of this.stream.getTracks()) {
      this.pc.addTrack(track, this.stream);
    }
    this.pc.onicecandidate = (ev) => {
      if (ev.candidate) {
        this.sig.send(MsgType.ICE, { candidate: ev.candidate.candidate });
      }
    };
    const offer = await this.pc.createOffer();
    await this.pc.setLocalDescription(offer);
    this.sig.send(MsgType.OFFER, { sdp: offer.sdp });
    return this.stream;
  }

  /** Ganti kamera depan/belakang tanpa renegosiasi (B5). */
  async flip(facing: CameraFacing): Promise<MediaStream> {
    this.stream?.getTracks().forEach((t) => t.stop());
    this.stream = await navigator.mediaDevices.getUserMedia({
      video: { facingMode: facing === "back" ? "environment" : "user" },
      audio: false,
    });
    if (this.pc) {
      const sender = this.pc.getSenders().find((s) => s.track?.kind === "video");
      const newTrack = this.stream.getVideoTracks()[0];
      if (sender && newTrack) {
        await sender.replaceTrack(newTrack);
      }
    }
    return this.stream;
  }

  /** Terapkan answer SDP dari desktop. */
  private async onAnswer(msg: WireMessage): Promise<void> {
    const sdp = (msg.payload as { sdp?: string } | null)?.sdp;
    if (!sdp || !this.pc) return;
    await this.pc.setRemoteDescription({ type: "answer", sdp });
  }

  /** Terapkan kandidat ICE dari desktop (trickle ICE, B4). */
  private async onRemoteIce(msg: WireMessage): Promise<void> {
    const candidate = (msg.payload as { candidate?: string } | null)?.candidate;
    if (!candidate || !this.pc) return;
    try {
      await this.pc.addIceCandidate({ candidate });
    } catch (e) {
      console.warn("kandidat ICE ditolak", e);
    }
  }

  /** `cmd_role` dari PC (B6): aktif = resolusi penuh, preview = rendah. */
  private onCmdRole(msg: WireMessage): void {
    const role = (msg.payload as { role?: "active" | "preview" } | null)?.role;
    if (role !== "active" && role !== "preview") return;
    const track = this.stream?.getVideoTracks()[0];
    if (!track) return;
    // B5/B6: preview memakai resolusi rendah (hemat baterai + bandwidth),
    // aktif kembali ke resolusi penuh. Ganti constraint tidak perlu
    // renegosiasi SDP (facingMode/width lewat applyConstraints).
    const width = role === "preview" ? 320 : 1280;
    const height = role === "preview" ? 180 : 720;
    track
      .applyConstraints({ width: { ideal: width }, height: { ideal: height } })
      .catch((e) => console.warn("gagal terapkan resolusi role", e));
  }

  /** `cmd_camera` dari PC (B6): kontrol jarak jauh facing/torch/zoom. */
  private onCmdCamera(msg: WireMessage): void {
    const p = msg.payload as {
      facing?: CameraFacing;
      torch?: boolean;
      zoom?: number;
    } | null;
    if (!p) return;
    const track = this.stream?.getVideoTracks()[0];
    if (p.facing) {
      void this.flip(p.facing).catch((e) => console.warn("gagal ganti kamera", e));
    }
    if (track && typeof p.torch === "boolean") {
      track
        .applyConstraints({ advanced: [{ torch: p.torch } as MediaTrackConstraintSet] })
        .catch((e) => console.warn("gagal terapkan torch", e));
    }
    if (track && typeof p.zoom === "number") {
      track
        .applyConstraints({ advanced: [{ zoom: p.zoom } as MediaTrackConstraintSet] })
        .catch((e) => console.warn("gagal terapkan zoom", e));
    }
  }

  /** `cmd_keyframe` dari PC (B6): persiapan penggantian sumber aktif.
   *
   * ponytail: browser tidak punya API untuk memaksa encoder mengirim frame
   * kunci sesuai permintaan (PLI/FIR); kita balas `keyframe_ready` sebagai
   * sinkronisasi switch — encoder WebRTC memancarkan IDR berkala alami,
   * jadi jeda singkat ini cukup untuk MVP. Ganti ke PLI eksplisit saat
   * backend desktop menulis RTCP langsung.
   */
  private onCmdKeyframe(msg: WireMessage): void {
    const sourceId = (msg.payload as { source_id?: string } | null)?.source_id;
    this.sig.trySend(MsgType.KEYFRAME_READY, sourceId ? { source_id: sourceId } : {});
  }

  /** Track video aktif (untuk torch/zoom B5). */
  activeTrack(): MediaStreamTrack | null {
    return this.stream?.getVideoTracks()[0] ?? null;
  }

  /** Tutup peer dan lepaskan kamera (B3: CLOSED). */
  close(): void {
    this.stream?.getTracks().forEach((t) => t.stop());
    this.pc?.close();
    this.pc = null;
    this.stream = null;
  }
}
