/**
 * Hook PWA HP: Wake Lock, baterai, dan indikator sinyal (D4/D5, fase 2).
 *
 * - Wake Lock: Screen Wake Lock API; request ulang saat halaman kembali
 *   visible (API melepaskan lock saat tab di-background).
 * - Baterai: Battery Status API; dilaporkan lewat source_update (B2/D3).
 * - Sinyal: kekuatan koneksi dari RTT heartbeat/pongg (B5) + navigator.connection
 *   bila tersedia.
 * ponytail: tiga hook kecil tanpa library — stdlib browser saja (A4).
 */
import { useEffect, useState } from "preact/hooks";

/** Nilai baterai null bila API tidak tersedia (mis. iOS). */
export function useBattery() {
  const [level, setLevel] = useState<number | null>(null);
  const [charging, setCharging] = useState(false);

  useEffect(() => {
    const nav = navigator as Navigator & {
      getBattery?: () => Promise<{ level: number; charging: boolean;
        addEventListener: (t: string, cb: () => void) => void;
        removeEventListener: (t: string, cb: () => void) => void; }>;
    };
    if (!nav.getBattery) return;
    let battery: { level: number; charging: boolean;
      addEventListener: (t: string, cb: () => void) => void;
      removeEventListener: (t: string, cb: () => void) => void; } | null = null;
    const update = () => {
      if (!battery) return;
      setLevel(battery.level);
      setCharging(battery.charging);
    };
    nav.getBattery().then((b) => {
      battery = b;
      update();
      b.addEventListener("levelchange", update);
      b.addEventListener("chargingchange", update);
    }).catch(() => { /* beberapa browser menolak tanpa https */ });
    return () => {
      if (battery) {
        battery.removeEventListener("levelchange", update);
        battery.removeEventListener("chargingchange", update);
      }
    };
  }, []);

  return { level, charging };
}

/** Request Screen Wake Lock (D4); request ulang saat tab kembali visible. */
export function useWakeLock(enabled: boolean) {
  const [active, setActive] = useState(false);

  useEffect(() => {
    if (!enabled) return;
    let lock: { released: boolean; release: () => Promise<void> } | null = null;
    const nav = navigator as Navigator & { wakeLock?: {
      request: (t: "screen") => Promise<{ released: boolean; release: () => Promise<void> }>; }; };

    const request = () => {
      if (!nav.wakeLock) return;
      nav.wakeLock.request("screen").then((w) => {
        lock = w;
        setActive(true);
      }).catch(() => {
        // Biasanya karena tab belum visible; dicoba lagi saat visible.
        setActive(false);
      });
    };

    const onVisibility = () => {
      if (document.visibilityState === "visible") request();
      else setActive(false);
    };

    request();
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      document.removeEventListener("visibilitychange", onVisibility);
      if (lock) void lock.release();
      setActive(false);
    };
  }, [enabled]);

  return active;
}

/** Indikator sinyal 0-4 bar dari kekuatan koneksi (D4). */
export function useSignalBar() {
  const [bars, setBars] = useState(4);

  useEffect(() => {
    const conn = (navigator as Navigator & { connection?: {
      effectiveType?: string; addEventListener?: (t: string, cb: () => void) => void;
      removeEventListener?: (t: string, cb: () => void) => void; } }).connection;
    if (!conn) return;
    const update = () => {
      // effectiveType: slow-2g/2g/3g/4g -> 1/2/3/4 bar.
      const map: Record<string, number> = { "slow-2g": 1, "2g": 2, "3g": 3, "4g": 4 };
      setBars(map[conn.effectiveType ?? "4g"] ?? 4);
    };
    update();
    conn.addEventListener?.("change", update);
    return () => conn.removeEventListener?.("change", update);
  }, []);

  return bars;
}
