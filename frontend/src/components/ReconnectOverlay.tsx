import { useState, useEffect } from "preact/hooks";

import type { ConnStatus } from "../signaling";

interface Props {
  status: ConnStatus | null;
  /** Coba lagi manual setelah anggaran reconnect habis (B3/D5). */
  onRetry: () => void;
}

/**
 * Overlay status koneksi (D5).
 *
 * Hanya menampilkan state dari Signaling (A3: UI tidak menyimpan state
 * sendiri). Menghitung mundur dari `retryInMs` setiap 250 ms. Saat anggaran
 * reconnect habis, tampilkan tombol Coba lagi.
 */
export function ReconnectOverlay({ status, onRetry }: Props) {
  const [remaining, setRemaining] = useState(0);

  useEffect(() => {
    if (!status || status.kind !== "reconnecting") {
      setRemaining(0);
      return;
    }
    setRemaining(Math.ceil(status.retryInMs / 1000));
    const start = Date.now();
    const id = window.setInterval(() => {
      const left = Math.ceil((status.retryInMs - (Date.now() - start)) / 1000);
      setRemaining(left > 0 ? left : 0);
    }, 250);
    return () => clearInterval(id);
  }, [status]);

  if (!status || status.kind === "connected") return null;

  const isReconnecting = status?.kind === "reconnecting";
  const isClosed = status?.kind === "closed";

  return (
    <div class="overlay reconnect-overlay" role="status" aria-live="polite">
      {isReconnecting && (
        <>
          <div class="spinner" aria-hidden="true" />
          <p>
            Koneksi terputus. Mencoba hubungkan ulang dalam {remaining} detik…
          </p>
          <p class="muted">Percobaan ke-{status.attempt}</p>
        </>
      )}
      {isClosed && (
        <>
          <p class="error-title">Tidak bisa terhubung kembali</p>
          <p class="muted">
            PC tidak merespons selama 60 detik. Periksa jaringan lalu coba lagi.
          </p>
          <button type="button" onClick={onRetry}>
            Coba lagi
          </button>
        </>
      )}
    </div>
  );
}
