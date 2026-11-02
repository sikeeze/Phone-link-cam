import { render } from "preact";
import { useEffect, useState, useCallback } from "preact/hooks";

import { DesktopState, SourceRow } from "./types";
import "./desktop.css";

type Screen = "home" | "cameras" | "studio" | "settings";

/**
 * UI aplikasi desktop (README C3). Disediakan oleh server HTTPS PhoneCam
 * di `/app`, baik di dalam webview Tauri maupun browser biasa.
 *
 * Ponytail (fase 1): hanya layar Beranda yang fungsional (QR, PIN, daftar HP,
 * banner vcam). Kamera/Studio/Pengaturan ada kerangkanya; sumber tunggal MVP
 * berarti Kamera dan Studio mengikuti data Beranda.
 */
export function DesktopApp() {
  const [screen, setScreen] = useState<Screen>("home");
  const [state, setState] = useState<DesktopState | null>(null);
  const [copied, setCopied] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const res = await fetch("/api/state", { headers: { Accept: "application/json" } });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      setState(await res.json());
    } catch (e) {
      console.error("gagal memuat state", e);
    }
  }, []);

  // B2: PIN kedaluwarsa 5 menit; muat ulang tiap 5 detik agar hitung mundur
  // akurat tanpa timer tambahan.
  useEffect(() => {
    void refresh();
    const id = setInterval(refresh, 5000);
    return () => clearInterval(id);
  }, [refresh]);

  const regenerate = async () => {
    const res = await fetch("/api/pin/regenerate", { method: "POST" });
    if (res.ok) void refresh();
  };

  const copyUrl = async () => {
    if (!state) return;
    try {
      await navigator.clipboard.writeText(state.pair_url);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // clipboard di webview lama bisa ditolak; biarkan pengguna menyalin manual
      console.warn("clipboard tidak tersedia");
    }
  };

  return (
    <div class="app-shell">
      <Sidebar screen={screen} onNavigate={setScreen} sources={state?.sources ?? []} />
      <main class="main">
        {screen === "home" && state && (
          <Home state={state} onRegenerate={regenerate} onCopyUrl={copyUrl} copied={copied} />
        )}
        {screen === "cameras" && <Cameras sources={state?.sources ?? []} />}
        {screen === "studio" && <Studio sources={state?.sources ?? []} />}
        {screen === "settings" && <Settings />}
        {!state && <p class="subtitle">Memuat…</p>}
      </main>
    </div>
  );
}

function Sidebar({
  screen,
  onNavigate,
  sources,
}: {
  screen: Screen;
  onNavigate: (s: Screen) => void;
  sources: SourceRow[];
}) {
  // ponytail: label teks ikon; Lucide diganti glyph sederhana (tanpa dependensi).
  const items: [Screen, string, string][] = [
    ["home", "Beranda", "⌂"],
    ["cameras", "Kamera", "▣"],
    ["studio", "Studio", "◎"],
    ["settings", "Pengaturan", "⚙"],
  ];
  const connected = sources.some((s) => s.state === "live");
  return (
    <nav class="sidebar" aria-label="Navigasi utama">
      <div class="logo" aria-hidden="true" />
      {items.map(([id, label, glyph]) => (
        <button
          key={id}
          class={`nav-btn${screen === id ? " active" : ""}`}
          title={label}
          aria-label={label}
          aria-current={screen === id ? "page" : undefined}
          onClick={() => onNavigate(id)}
        >
          <span aria-hidden="true">{glyph}</span>
        </button>
      ))}
      <div class="spacer" />
      <span class={`status-pill${connected ? "" : " disconnected"}`} title="Status kamera">
        <span class="dot" aria-hidden="true" />
        {connected ? "Terhubung" : "Menunggu"}
      </span>
    </nav>
  );
}

function Home({
  state,
  onRegenerate,
  onCopyUrl,
  copied,
}: {
  state: DesktopState;
  onRegenerate: () => void;
  onCopyUrl: () => void;
  copied: boolean;
}) {
  const mins = Math.floor(state.pin_remaining_secs / 60);
  const secs = state.pin_remaining_secs % 60;
  return (
    <>
      <h1>Beranda</h1>
      <p class="subtitle">Pindai QR dengan HP untuk mulai menghubungkan kamera.</p>
      <div class="home-grid">
        <div class="card qr-card">
          <img src={state.qr_url} alt="QR kode pairing PhoneCam" />
          <div class="pin-display">
            <span class="pin">{state.pin}</span>
            <span class="status-pill">
              {mins}:{String(secs).padStart(2, "0")}
            </span>
          </div>
          <button class="btn" onClick={onRegenerate}>
            Buat PIN baru
          </button>
        </div>
        <div>
          <div class="pair-url">
            <code>{state.pair_url}</code>
            <button class="btn" onClick={onCopyUrl}>
              {copied ? "Disalin" : "Salin"}
            </button>
          </div>
          <div class="source-list">
            {state.sources.length === 0 ? (
              <div class="empty-state">
                <span aria-hidden="true" style="font-size: 32px">
                  ▒
                </span>
                <span>Pindai QR dengan HP kamu</span>
              </div>
            ) : (
              state.sources.map((s) => (
                <div key={s.id} class="source-row">
                  <span class="name">{s.name}</span>
                  <span class="tag">{s.connection_kind}</span>
                  <span class="status-pill">
                    <span class="dot" aria-hidden="true" />
                    {s.state}
                  </span>
                  {s.battery != null && <span class="tag">{s.battery}%</span>}
                  {s.rtt_ms != null && <span class="tag">{s.rtt_ms} ms</span>}
                </div>
              ))
            )}
          </div>
          {!state.vcam_installed && (
            <div class="banner">
              <span aria-hidden="true">⚠</span>
              <span>Virtual camera belum terpasang.</span>
              <button class="btn">Pasang</button>
            </div>
          )}
        </div>
      </div>
    </>
  );
}

function Cameras({ sources }: { sources: SourceRow[] }) {
  return (
    <>
      <h1>Kamera</h1>
      <p class="subtitle">Daftar HP yang sedang terhubung.</p>
      {sources.length === 0 ? (
        <div class="empty-state">
          <span>Belum ada kamera terhubung. Hubungkan dari layar Beranda.</span>
        </div>
      ) : (
        <div class="source-list">
          {sources.map((s) => (
            <div key={s.id} class="card">
              <div class="source-row" style="border: none; padding: 0">
                <span class="name">{s.name}</span>
                <span class="tag">{s.connection_kind}</span>
                <span class="status-pill">
                  <span class="dot" aria-hidden="true" />
                  {s.state}
                </span>
              </div>
            </div>
          ))}
        </div>
      )}
    </>
  );
}

function Studio({ sources }: { sources: SourceRow[] }) {
  const active = sources.find((s) => s.state === "live");
  return (
    <>
      <h1>Studio</h1>
      <p class="subtitle">Preview output dan kontrol kamera aktif.</p>
      <div class="studio-grid">
        <div class="preview">
          <span class="live-label">LIVE</span>
        </div>
        <div class="card control-panel">
          <label>
            Zoom
            <input type="range" min="1" max="2" step="0.1" defaultValue="1" />
          </label>
          <label>
            Rotasi
            <input type="range" min="0" max="270" step="90" defaultValue="0" />
          </label>
          <div class="setting-row">
            <span>Mirror</span>
            <button class="toggle on" aria-label="Mirror" />
          </div>
          <div class="setting-row">
            <span>Fit/Fill</span>
            <button class="toggle" aria-label="Fit atau fill" />
          </div>
          <p class="subtitle" style="margin: 0">
            {active ? `Aktif: ${active.name}` : "Belum ada sumber aktif."}
          </p>
        </div>
      </div>
    </>
  );
}

function Settings() {
  return (
    <>
      <h1>Pengaturan</h1>
      <p class="subtitle">Perubahan diterapkan langsung (C3.4).</p>
      <div class="card settings-section">
        <h2>Umum</h2>
        <div class="setting-row">
          <span>Tema</span>
          <span class="tag">Sistem</span>
        </div>
        <div class="setting-row">
          <span>Mulai bersama sistem</span>
          <button class="toggle" aria-label="Mulai bersama sistem" />
        </div>
      </div>
      <div class="card settings-section">
        <h2>Video</h2>
        <div class="setting-row">
          <span>Resolusi output</span>
          <span class="tag">1280×720</span>
        </div>
        <div class="setting-row">
          <span>Frame rate</span>
          <span class="tag">30 fps</span>
        </div>
      </div>
      <div class="card settings-section">
        <h2>Jaringan</h2>
        <div class="setting-row">
          <span>Port HTTPS</span>
          <span class="tag">8443</span>
        </div>
      </div>
    </>
  );
}

export function mount(root: HTMLElement): void {
  render(<DesktopApp />, root);
}
