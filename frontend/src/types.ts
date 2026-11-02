/// <reference types="vite/client" />

// Tipe protokol signaling, cerminan phonecam-core::protocol (B4).
// Disinkronkan manual pada fase 1; lihat PROGRESS.md.

export const PROTOCOL_VERSION = 1;

/** Jenis pesan signaling (B4). */
export const MsgType = {
  HELLO: "hello",
  AUTH: "auth",
  AUTH_OK: "auth_ok",
  AUTH_FAIL: "auth_fail",
  OFFER: "offer",
  ANSWER: "answer",
  ICE: "ice",
  SOURCE_UPDATE: "source_update",
  CMD_QUALITY: "cmd_quality",
  CMD_KEYFRAME: "cmd_keyframe",
  CMD_CAMERA: "cmd_camera",
  CMD_ROLE: "cmd_role",
  PING: "ping",
  PONG: "pong",
  BYE: "bye",
} as const;

export type MsgTypeValue = (typeof MsgType)[keyof typeof MsgType];

/** Pesan signaling mentah `{ v, type, id?, ts?, payload? }` (B4). */
export interface WireMessage {
  v: number;
  type: MsgTypeValue;
  id?: string;
  ts?: number;
  payload?: unknown;
}

export interface Capabilities {
  zoom: boolean;
  torch: boolean;
  camera_switch: boolean;
  webcodecs: boolean;
}

export type Transport = "webrtc" | "ws-webcodecs";

/** Payload `hello` (HP > PC, B4). */
export interface HelloPayload {
  device_id: string;
  name: string;
  user_agent: string;
  capabilities: Capabilities;
  transport: Transport;
}

export type AuthCredential = { pin: string } | { token: string };

/** Payload `auth` (HP > PC, B4). */
export interface AuthPayload {
  credential: AuthCredential;
}

export type AuthFailCode =
  | "BAD_PIN"
  | "LOCKED"
  | "EXPIRED"
  | "SESSION_FULL"
  | "PERMISSION_DENIED";

/** Payload `auth_fail` (PC > HP, B4). */
export interface AuthFailPayload {
  code: AuthFailCode;
  retry_after?: number;
}

/** Preset kualitas (B5). Cerminan phonecam-core::protocol::QualityPreset. */
export type QualityPreset = "saving" | "balanced" | "high" | "smooth" | "preview";

/** Payload `auth_ok` (PC > HP, B4). Token untuk reconnect. */
export interface AuthOkPayload {
  source_id: string;
  token: string;
  quality: QualityPreset;
}

/** Payload `offer`/`answer` (B4). */
export interface SdpPayload {
  sdp: string;
  type: "offer" | "answer";
  source_id?: string;
}

/** Payload `ice` (B4). */
export interface IcePayload {
  candidate: string;
  sdp_mline_index: number;
  sdp_mid?: string;
  source_id?: string;
}

export type CameraFacing = "front" | "back";

/** Payload `source_update` (HP > PC, B4). */
export interface SourceUpdatePayload {
  source_id: string;
  torch_on?: boolean;
  zoom?: number;
  battery_level?: number;
  thermal?: "normal" | "fair" | "serious" | "critical";
}
