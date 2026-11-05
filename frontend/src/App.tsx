import { render } from "preact";
import { useState } from "preact/hooks";
import { Router, route } from "preact-router";

import { PairingScreen } from "./components/PairingScreen";
import { CameraScreen } from "./components/CameraScreen";
import { Signaling } from "./signaling";
import { MsgType, type Transport } from "./types";
import { webcodecsSupported } from "./webcodecs";

/** Payload `auth_ok` (B2): token reconnect 128-bit per sesi desktop. */
interface AuthOkPayload {
  source_id: string;
  token: string;
}

/**
 * Aplikasi web HP.
 *
 * Router preact-router: `/` = layar PIN, `/camera` = layar siaran.
 * State signaling diangkat ke sini agar diteruskan ke CameraScreen.
 * B2/B3: token dari `auth_ok` disimpan di Signaling supaya bisa dipakai
 * ulang saat koneksi putus (slot sumber tetap, tidak menumpuk).
 */
export function App() {
  const [signaling, setSignaling] = useState<Signaling | null>(null);
  const [transport, setTransport] = useState<Transport>("webrtc");

  const onPaired = (sig: Signaling) => {
    // B2: simpan token reconnect saat auth berhasil.
    sig.on(MsgType.AUTH_OK, (msg) => {
      const payload = msg.payload as AuthOkPayload | null;
      const token = payload?.token;
      // B9.2.4: catat transport yang diterima PC (hello echo).
      const t = (msg.payload as { transport?: Transport } | null)?.transport;
      if (t) setTransport(t);
      if (token) {
        sig.setReauth(token, () => ({
          device_id: "phone-web",
          name: "HP Web",
          user_agent: navigator.userAgent,
          capabilities: { zoom: false, torch: false, camera_switch: false, webcodecs: webcodecsSupported() },
          transport: t === "ws-webcodecs" ? "ws-webcodecs" : "webrtc",
        }));
      }
    });
    setSignaling(sig);
    route("/camera", true);
  };


  return (
    <Router>
      <PairingScreen path="/" onPaired={onPaired} default />
      <CameraScreen path="/camera" signaling={signaling} transport={transport} />
    </Router>
  );
}

export function mount(root: HTMLElement): void {
  render(<App />, root);
}
