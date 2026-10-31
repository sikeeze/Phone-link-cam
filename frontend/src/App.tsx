import { render } from "preact";
import { useState } from "preact/hooks";
import { Router, route } from "preact-router";

import { PairingScreen } from "./components/PairingScreen";
import { CameraScreen } from "./components/CameraScreen";
import { Signaling } from "./signaling";

/**
 * Aplikasi web HP (fase 0: kerangka).
 *
 * Router preact-router: `/` = layar PIN, `/camera` = layar siaran.
 * State signaling diangkat ke sini agar diteruskan ke CameraScreen.
 */
export function App() {
  const [signaling, setSignaling] = useState<Signaling | null>(null);

  const onPaired = (sig: Signaling) => {
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
