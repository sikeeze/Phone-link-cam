import { render } from "preact";
import { useState } from "preact/hooks";
import { Router, route } from "preact-router";

import { PairingScreen } from "./components/PairingScreen";
import { CameraScreen } from "./components/CameraScreen";
import { Signaling } from "./signaling";
import { MsgType } from "./types";

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

  const onPaired = (sig: Signaling) => {
    // B2: simpan token reconnect saat auth berhasil.
    sig.on(MsgType.AUTH_OK, (msg) => {
      const token = (msg.payload as AuthOkPayload | null)?.token;
      if (token) {
        sig.setReauth(token, () => ({
          device_id: "phone-web",
          name: "HP Web",
          user_agent: navigator.userAgent,
          capabilities: { zoom: false, torch: false, camera_switch: false, webcodecs: false },
          transport: "webrtc",
        }));
      }
    });
    setSignaling(sig);
    route("/camera", true);
  };

  return (
    <Router>
      <PairingScreen path="/" onPaired={onPaired} default />
      <CameraScreen path="/camera" signaling={signaling} />
    </Router>
  );
}

export function mount(root: HTMLElement): void {
  render(<App />, root);
}
