import { render } from "preact";
import { useEffect, useState, useCallback, useRef } from "preact/hooks";

import { DesktopState, SourceRow, ConnectionsState } from "./types";
import { WizardState, ANDROID_STEPS, IPHONE_STEPS } from "./wizard";
import "./desktop.css";

type Screen = "home" | "cameras" | "studio" | "connections" | "settings";

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
        {screen === "connections" && <Connections />}
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
    ["connections", "Koneksi", "⇄"],
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
                  {s.rtt_ms != null && (
                    <span class={`tag ${qualityBadge(s.rtt_ms) === "Baik" ? "qual-ok" : qualityBadge(s.rtt_ms) === "Buruk" ? "qual-bad" : ""}`}>
                      {s.rtt_ms} ms · {qualityBadge(s.rtt_ms)}
                    </span>
                  )}
                </div>
              ))
            )}
          </div>
          {!state.vcam_installed && (
            <div class="banner">
              <span aria-hidden="true">⚠</span>
              <span>Virtual camera belum terpasang.</span>
              <button
                class="btn"
                onClick={async () => {
                  // Phase 4: aksi jelas saat driver hilang (VCAM_NOT_INSTALLED).
                  // Backend OSS Virtual Camera membalas 409 + langkah pemasangan.
                  try {
                    const res = await fetch("/api/devices/install?kind=vcam", {
                      method: "POST",
                    });
                    if (res.status === 409) {
                      const body = await res.json();
                      window.alert(
                        `${body.title ?? "Gagal memasang"}\n\n${body.explain ?? ""}\n\n${body.action ?? ""}`,
                      );
                    } else if (!res.ok) {
                      window.alert("Gagal memasang virtual camera (cek log aplikasi).");
                    }
                  } catch {
                    window.alert("Permintaan instalasi gagal (server tidak merespons).");
                  }
                }}
              >
                Pasang
              </button>
            </div>
          )}
        </div>
      </div>
    </>
  );
}

function Cameras({ sources }: { sources: SourceRow[] }) {
  const [renaming, setRenaming] = useState<string | null>(null);
  return (
    <>
      <h1>Kamera</h1>
      <p class="subtitle">Daftar HP yang sedang terhubung (B6: maks 6).</p>
      {sources.length === 0 ? (
        <div class="empty-state">
          <span>Belum ada kamera terhubung. Hubungkan dari layar Beranda.</span>
        </div>
      ) : (
        <div class="camera-grid">
          {sources.map((s, i) => (
            <div key={s.id} class="card camera-card">
              <div class="thumb" aria-label="Pratinjau sumber">
                <span class="thumb-num">{i + 1}</span>
                {s.role === "active" && <span class="live-label">LIVE</span>}
              </div>
              <div class="camera-meta">
                {renaming === s.id ? (
                  <input
                    class="rename-input"
                    value={s.name}
                    autoFocus
                    onBlur={(e) => {
                      setRenaming(null);
                      void fetch(`/api/source/${s.id}/rename`, {
                        method: "POST",
                        headers: { "Content-Type": "application/json" },
                        body: JSON.stringify({ name: (e.target as HTMLInputElement).value }),
                      });
                    }}
                  />
                ) : (
                  <button
                    class="name-btn"
                    title="Ganti nama perangkat"
                    onClick={() => setRenaming(s.id)}
                  >
                    {s.name} ✎
                  </button>
                )}
                <span class="tag">{s.connection_kind}</span>
                <span class="status-pill">
                  <span class="dot" aria-hidden="true" />
                  {s.state}
                </span>
                {s.battery != null && <span class="tag">{s.battery}%</span>}
                {s.rtt_ms != null && (
                  <span class={`tag ${qualityBadge(s.rtt_ms) === "Baik" ? "qual-ok" : qualityBadge(s.rtt_ms) === "Buruk" ? "qual-bad" : ""}`}>
                    {s.rtt_ms} ms · {qualityBadge(s.rtt_ms)}
                  </span>
                )}
              </div>
              <button
                class="btn"
                disabled={s.role === "active"}
                onClick={() => void fetch(`/api/source/${s.id}/active`, { method: "POST" })}
              >
                {s.role === "active" ? "Sedang aktif" : "Jadikan aktif"}
              </button>
            </div>
          ))}
        </div>
      )}
    </>
  );
}

type Transition = "cut" | "crossfade" | "slide";

/** Badge kualitas jalur dari RTT (B9.1: Baik < 30 ms, Cukup 30-80, Buruk > 80). */
function qualityBadge(rttMs: number | null): string | null {
  if (rttMs == null) return null;
  if (rttMs < 30) return "Baik";
  if (rttMs <= 80) return "Cukup";
  return "Buruk";
}

function Studio({ sources }: { sources: SourceRow[] }) {
  const active = sources.find((s) => s.role === "active") ?? sources[0];
  const [transition, setTransition] = useState<Transition>("cut");
  const [durationMs, setDurationMs] = useState(200);
  /**
   * B6: transisi crossfade/slide nyata. Saat sumber aktif berganti, nama
   * sumber lama tetap tampil di layer bawah sementara nama baru animasi
   * masuk sesuai transisi + durasi yang dipilih. Cut = tanpa animasi.
   * ponytail: pratinjau video masih placeholder (fase 1); saat vcam
   * menayangkan frame, ganti `label` ini dengan elemen video.
   */
  const [prevName, setPrevName] = useState<string | null>(null);
  const [entering, setEntering] = useState(false);
  const activeIdRef = useRef<string | null>(active?.id ?? null);
  const activeNameRef = useRef<string | null>(active?.name ?? null);
  if (active && active.id !== activeIdRef.current) {
    activeIdRef.current = active.id;
    // B6: simpan nama sumber lama untuk layer keluar transisi.
    setPrevName(activeNameRef.current);
    activeNameRef.current = active.name;
    setEntering(true);
  }
  // Animasi selesai setelah durasi transisi; layer keluar dibersihkan.
  useEffect(() => {
    if (!entering) return;
    const id = window.setTimeout(() => {
      setEntering(false);
      setPrevName(null);
    }, durationMs);
    return () => window.clearTimeout(id);
  }, [entering, durationMs]);

  // B7: hotkey 1-6 memilih sumber saat aplikasi fokus.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement) return;
      if (e.key >= "1" && e.key <= "6") {
        const s = sources[Number(e.key) - 1];
        if (s && s.role !== "active") {
          void fetch(`/api/source/${s.id}/active`, { method: "POST" });
        }
      } else if (e.key === "ArrowRight" || e.key === "ArrowLeft") {
        const idx = sources.findIndex((s) => s.role === "active");
        const next = sources[(idx + (e.key === "ArrowRight" ? 1 : sources.length - 1)) % sources.length];
        if (next && next.role !== "active") {
          void fetch(`/api/source/${next.id}/active`, { method: "POST" });
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [sources]);

  const switchTo = (s: SourceRow) => {
    if (s.role === "active") return;
    // B6: keyframe-before-switch — PC minta keyframe HP sebelum ganti;
    // logika tunggu 500 ms ada di backend (cmd_keyframe).
    void fetch(`/api/source/${s.id}/active`, { method: "POST" });
  };

  return (
    <>
      <h1>Studio</h1>
      <p class="subtitle">Preview output dan kontrol kamera aktif (B6/B7).</p>
      <div class="studio-grid">
        <div class="preview" data-transition={transition}>
          <span class="live-label">LIVE</span>
          {/* B6: layer keluar (sumber lama) — fade/slide out saat diganti. */}
          {prevName !== null && (
            <span
              class={`preview-name exiting t-${transition}`}
              style={{ ["--dur" as string]: `${durationMs}ms` }}
            >
              {prevName}
            </span>
          )}
          {/* B6: layer masuk (sumber aktif). cut = langsung, crossfade/slide = animasi. */}
          {active && (
            <span
              class={`preview-name${entering && transition !== "cut" ? ` entering t-${transition}` : ""}`}
              style={{ ["--dur" as string]: `${durationMs}ms` }}
            >
              {active.name}
            </span>
          )}
        </div>
        <div class="card control-panel">
          <fieldset>
            <legend>Transisi (B6)</legend>
            {(["cut", "crossfade", "slide"] as const).map((t) => (
              <label key={t} class="radio-row">
                <input
                  type="radio"
                  name="transition"
                  checked={transition === t}
                  onChange={() => setTransition(t)}
                />
                {t === "cut" ? "Cut" : t === "crossfade" ? "Crossfade" : "Slide"}
              </label>
            ))}
          </fieldset>
          <label>
            Durasi transisi: {durationMs} ms
            <input
              type="range"
              min="0"
              max="500"
              step="50"
              value={durationMs}
              onInput={(e) => setDurationMs(Number((e.target as HTMLInputElement).value))}
            />
          </label>
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
      <div class="switcher" role="tablist" aria-label="Switcher sumber 1-6 (B7)">
        {sources.map((s, i) => (
          <button
            key={s.id}
            role="tab"
            aria-selected={s.role === "active"}
            class={`switcher-thumb ${s.role === "active" ? "active" : ""}`}
            title={`${s.name} (hotkey ${i + 1})`}
            onClick={() => switchTo(s)}
          >
            {/* B6: thumbnail statis base64 JPEG dari HP (snapshot 2 detik). */}
            {s.thumb ? (
              <img
                class="thumb-img"
                src={`data:image/jpeg;base64,${s.thumb}`}
                alt=""
                aria-hidden="true"
              />
            ) : (
              <span class="thumb-num">{i + 1}</span>
            )}
            <span class="switcher-name">{s.name}</span>
            {s.role === "active" && <span class="live-dot" aria-label="Aktif" />}
            {/* B7: kontrol jarak jauh cmd_camera — ganti kamera depan/belakang. */}
            <span
              class="thumb-flip"
              role="button"
              tabindex={0}
              aria-label={`Ganti kamera ${s.name}`}
              onClick={(e) => {
                e.stopPropagation();
                void fetch(`/api/source/${s.id}/camera`, {
                  method: "POST",
                  headers: { "Content-Type": "application/json" },
                  body: JSON.stringify({ facing: "back" }),
                });
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.stopPropagation();
                  e.preventDefault();
                  void fetch(`/api/source/${s.id}/camera`, {
                    method: "POST",
                    headers: { "Content-Type": "application/json" },
                    body: JSON.stringify({ facing: "back" }),
                  });
                }
              }}
            >
              ⇄
            </span>
          </button>
        ))}
      </div>
      <p class="subtitle" style="margin: 0">
        Hotkey: 1-6 pilih sumber, ←/→ pindah (B7). Transisi aktif: {transition} {durationMs} ms.
      </p>
    </>
  );
}

/**
 * Layar Jalur koneksi (README C4). Grid 2x2 kartu jalur: Wi-Fi, Kabel USB
 * Android (mode ADB), Kabel USB iPhone (tethering/hotspot), Hotspot.
 *
 * Status kartu diambil dari `/api/connections` (B9.1): antarmuka privat
 * diklasifikasikan per subnet, status adb dari PATH. Mode ADB memakai
 * endpoint POST /api/adb/reverse (B9.2). Tiap kartu menampilkan URL
 * pairing jalurnya sendiri (B9.1).
 *
 * ponytail: deteksi iPhone/Hotspot memakai subnet antarmuka (172.20.10.x);
 * Wi-Fi Direct HP ke PC butuh mDNS — fase berikutnya (ADR-002).
 */
function Connections() {
  const [conns, setConns] = useState<ConnectionsState | null>(null);
  const [autoMove, setAutoMove] = useState(false);
  /** B9.1: kirim toggle pindah otomatis ke server (default mati). */
  const toggleAutoMove = async () => {
    const next = !autoMove;
    setAutoMove(next);
    const res = await fetch("/api/failover", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ on: next }),
    });
    if (!res.ok) setAutoMove(false);
  };
  const [busy, setBusy] = useState(false);
  const [wizard, setWizard] = useState<WizardState | null>(null);

  // B9.1: polling tiap 3 detik (constants::timing::INTERFACE_POLL_SECS).
  const refresh = useCallback(async () => {
    try {
      const res = await fetch("/api/connections");
      if (res.ok) setConns(await res.json());
    } catch (e) {
      console.error("gagal memuat koneksi", e);
    }
  }, []);

  useEffect(() => {
    void refresh();
    const id = setInterval(refresh, 3000);
    return () => clearInterval(id);
  }, [refresh]);

  const tether = conns?.interfaces.find((i) => i.kind === "tether");
  const hotspot = conns?.interfaces.find((i) => i.kind === "hotspot");
  const adb = conns?.adb;
  const lan = conns?.interfaces.find((i) => i.kind === "lan");

  const useAdb = async () => {
    setBusy(true);
    try {
      const res = await fetch("/api/adb/reverse", { method: "POST" });
      if (res.status === 404) {
        // ADR-002 D1: konfirmasi eksplisit sebelum unduh platform-tools.
        alert(
          "adb tidak ditemukan.\n\nUnduh platform-tools dari Google? (~15 MB, " +
            "disimpan di data aplikasi — jalankan lagi nanti tanpa unduh ulang).",
        );
      } else if (!res.ok) {
        const body = await res.json().catch(() => ({}));
        alert(`Gagal memulai mode ADB: ${body.error ?? res.status}`);
      }
    } finally {
      setBusy(false);
    }
  };

  const card = (
    title: string,
    status: string,
    desc: string,
    target: string,
    actionLabel: string,
    onAction: () => void,
    selected: boolean,
    pairUrl?: string,
  ) => (
    <div class={`card conn-card${selected ? " selected" : ""}`}>
      <h2>{title}</h2>
      <span class={`badge ${status === "Aktif" ? "ok" : status === "Perlu setup" ? "warn" : "muted"}`}>
        {status}
      </span>
      <p class="subtitle" style="margin: 0">
        {desc}
      </p>
      <span class="tag">Target: {target}</span>
      {pairUrl && (
        <span class="tag mono" title={`URL pairing jalur ini: ${pairUrl}`}>
          {pairUrl}
        </span>
      )}
      <button class="btn" onClick={onAction} disabled={busy}>
        {actionLabel}
      </button>
    </div>
  );

  return (
    <>
      <h1>Koneksi</h1>
      <p class="subtitle">Pilih jalur koneksi ke HP (B9). Wi-Fi tetap jadi bawaan.</p>
      <div class="conn-grid">
        {card(
          "Wi-Fi",
          lan ? "Aktif" : "Tidak tersedia",
          "Pindai QR atau buka URL pairing di browser HP.",
          "20-60 ms",
          "Gunakan",
          () => setAutoMove(false),
          !tether && !hotspot && !adb?.ready,
          lan?.pair_url,
        )}
        {card(
          "Kabel USB Android",
          adb ? (adb.ready ? "Aktif" : adb.unauthorized ? "Perlu setup" : "Terdeteksi") : "Tidak tersedia",
          adb?.unauthorized
            ? 'Setujui dialog "Izinkan debugging USB" di HP.'
            : `Mode ADB: HP membuka ${adb?.pair_url ?? "http://localhost:8080"} lewat adb reverse.`,
          "40-100 ms",
          "Siapkan",
          useAdb,
          adb?.ready ?? false,
          adb?.pair_url,
        )}
        {card(
          "Kabel USB iPhone",
          hotspot ? "Aktif" : "Tidak tersedia",
          "Aktifkan Personal Hotspot di iPhone, sambungkan kabel, ketuk Percayai.",
          "40-100 ms",
          "Panduan",
          () => setWizard({ platform: "iphone", step: 0 }),
          hotspot != null,
          hotspot?.pair_url,
        )}
        {card(
          "Hotspot",
          tether ? "Aktif" : "Tidak tersedia",
          "Android: aktifkan USB tethering; PC mendapat antarmuka baru.",
          "40-100 ms",
          "Panduan",
          () => setWizard({ platform: "android", step: 0 }),
          tether != null,
          tether?.pair_url,
        )}
      </div>
      {conns && conns.interfaces.length > 0 && (
        <div class="card settings-section">
          <h2>Urutan prioritas jalur (B9.1)</h2>
          <ol class="priority-list">
            {conns.interfaces.map((i) => (
              <li key={i.name}>
                <span class="name">{i.kind_label}</span>
                <span class="tag">{i.name}</span>
                <span class={`badge ${i.status === "active" ? "ok" : "muted"}`}>
                  {i.status === "active" ? "Aktif" : i.status === "needs_setup" ? "Perlu setup" : "Tidak tersedia"}
                </span>
              </li>
            ))}
          </ol>
          <p class="subtitle" style="margin: 0">
            Prioritas otomatis: kabel, lalu Wi-Fi, lalu hotspot. Badge kualitas
            (Baik/Cukup/Buruk dari RTT) muncul setelah HP terhubung.
          </p>
        </div>
      )}
      {wizard && (
        <CableWizard
          wizard={wizard}
            adbReady={conns?.adb?.ready ?? false}
            adbUrl={conns?.adb?.pair_url ?? "http://localhost:8080"}
            busy={busy}
          onClose={() => setWizard(null)}
          onNext={() => {
            const steps = wizard.platform === "android" ? ANDROID_STEPS : IPHONE_STEPS;
            if (wizard.step >= steps.length - 1) {
              setWizard(null);
            } else {
              setWizard({ ...wizard, step: wizard.step + 1 });
            }
          }}
        />
      )}
      <div class="card settings-section">
        <div class="setting-row">
          <span>Pindah otomatis bila koneksi putus</span>
          <button
            class={`toggle${autoMove ? " on" : ""}`}
            aria-label="Pindah otomatis"
            onClick={() => void toggleAutoMove()}
          />
        </div>
        <p class="subtitle" style="margin: 0">
          B9.1: saat koneksi putus, HP mencoba jalur berikutnya dengan token
          sama (urutan kabel, Wi-Fi, hotspot). Default mati.
        </p>
      </div>
    </>
  );
}

/**
 * Wizard kabel (README C4): stepper 3 langkah per jenis HP.
 * ponytail: deteksi otomatis hanya untuk langkah terakhir Android (status
 * adb.ready dari /api/connections); langkah lain manual karena tidak ada
 * API web yang bisa membaca setelan HP.
 */
function CableWizard({
  wizard,
  adbReady,
  adbUrl,
  busy,
  onClose,
  onNext,
}: {
  wizard: WizardState;
  adbReady: boolean;
  /** B9.2: URL listener ADB aktual (fallback port bila 8080 sibuk). */
  adbUrl: string;
  busy: boolean;
  onClose: () => void;
  onNext: () => void;
}) {
  const steps = wizard.platform === "android" ? ANDROID_STEPS : IPHONE_STEPS;
  const isLast = wizard.step >= steps.length - 1;
  // Langkah terakhir Android terpenuhi otomatis saat adb mendeteksi perangkat.
  const detected = wizard.platform === "android" && isLast && adbReady;
  return (
    <div class="wizard-overlay" role="dialog" aria-label="Panduan kabel USB">
      <div class="card wizard-card">
        <h2>{wizard.platform === "android" ? "Kabel USB Android" : "Kabel USB iPhone"}</h2>
        <div class="wizard-dots" aria-hidden="true">
          {steps.map((_, i) => (
            <span key={i} class={`wizard-dot${i <= wizard.step ? " done" : ""}`} />
          ))}
        </div>
        <p class="wizard-step">
          <strong>
            Langkah {wizard.step + 1}/{steps.length}.
          </strong>{" "}
          {steps[wizard.step]}
        </p>
        {detected && (
          <p class="subtitle" style="margin: 0">
            ✓ Perangkat terdeteksi lewat kabel.
          </p>
        )}
        {wizard.platform === "android" && isLast && (
          <p class="subtitle" style="margin: 0">
            Buka <code>{adbUrl}</code> di browser HP, lalu masukkan PIN seperti
            biasa (B9.2).
          </p>
        )}
        <div class="wizard-actions">
          <button class="btn" onClick={onClose}>
            Batal
          </button>
          <button class="btn primary" onClick={onNext} disabled={busy}>
            {isLast ? "Selesai" : "Lanjut"}
          </button>
        </div>
      </div>
    </div>
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
