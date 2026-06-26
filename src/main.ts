import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { LogicalSize } from "@tauri-apps/api/dpi";
import { listen } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";

// ---------------- types (mirror the Rust commands) ----------------
interface Location {
  lat: number;
  lon: number;
  city: string;
  country_code: string;
}
interface Settings {
  selected_style: string;
  play_dua_after: boolean;
  madhab: string;
  method: string | null;
  volume: number;
  location: Location | null;
  autostart: boolean;
}
interface PrayerEntry {
  key: string;
  name: string;
  time: string; // "HH:MM"
  iso: string; // RFC3339 local
  is_fardh: boolean;
}
interface TimesResponse {
  location: Location | null;
  method: string | null;
  entries: PrayerEntry[];
  next_key: string | null;
  next_name: string | null;
  next_time: string | null;
  next_iso: string | null;
}
interface StyleOption {
  key: string;
  label: string;
}
/** What the backend is currently playing (`playback-started` payload). */
interface PlaybackStarted {
  kind: "athan" | "preview-style" | "preview-dua";
  prayer: string | null;
}

const ICONS: Record<string, string> = {
  fajr: '<svg width="26" height="26" viewBox="0 0 24 24" fill="none" stroke="#8aa6ff" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M17 18a5 5 0 0 0-10 0"/><path d="M12 2v7M4.2 10.2l1.4 1.4M1 18h2M21 18h2M18.4 11.6l1.4-1.4M23 22H1"/><path d="m8 6 4-4 4 4" stroke="#6f8dff"/></svg>',
  dhuhr: '<svg width="26" height="26" viewBox="0 0 24 24" fill="none" stroke="#f5c542" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/></svg>',
  asr: '<svg width="26" height="26" viewBox="0 0 24 24" fill="none" stroke="#f5c542" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/></svg>',
  maghrib: '<svg width="26" height="26" viewBox="0 0 24 24" fill="none" stroke="#ff8a73" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M17 18a5 5 0 0 0-10 0"/><path d="M12 9V2M4.2 10.2l1.4 1.4M1 18h2M21 18h2M18.4 11.6l1.4-1.4M23 22H1"/><path d="m16 5-4 4-4-4" stroke="#ff6f57"/></svg>',
  isha: '<svg width="26" height="26" viewBox="0 0 24 24" fill="none" stroke="#b69cff" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z"/></svg>',
};

const $ = <T extends HTMLElement>(sel: string) => document.querySelector(sel) as T;

let settings: Settings;
let lastTimes: TimesResponse | null = null;
let playing: PlaybackStarted | null = null;
let styleOptions: StyleOption[] = [];

// ---------------- helpers ----------------
function to12h(d: Date): string {
  let h = d.getHours();
  const m = d.getMinutes();
  const ampm = h >= 12 ? "PM" : "AM";
  h = h % 12 || 12;
  return `${h}:${String(m).padStart(2, "0")} ${ampm}`;
}

function fmtDuration(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m ${sec}s`;
  return `${sec}s`;
}

const addDays = (d: Date, n: number) => new Date(d.getTime() + n * 86400000);

async function save() {
  await invoke("save_settings", { settings });
}

// ---------------- main card rendering ----------------
function fardhEntries(): (PrayerEntry & { t: Date })[] {
  if (!lastTimes) return [];
  return lastTimes.entries
    .filter((e) => e.is_fardh)
    .map((e) => ({ ...e, t: new Date(e.iso) }));
}

function backendNextFardh(): { name: string; key: string; t: Date } | null {
  if (!lastTimes?.next_key || !lastTimes.next_name || !lastTimes.next_iso) return null;
  return {
    name: lastTimes.next_name,
    key: lastTimes.next_key,
    t: new Date(lastTimes.next_iso),
  };
}

/** Determine the current (active) prayer and the next one, with the next time. */
function currentAndNext() {
  const items = fardhEntries();
  if (items.length === 0) return null;
  const now = new Date();

  const nextIdx = items.findIndex((i) => i.t > now);
  if (nextIdx === -1) {
    // After Isha -> next is tomorrow's real Fajr from the backend.
    const fajr = items[0];
    return {
      current: items[items.length - 1],
      next: backendNextFardh() ?? { name: fajr.name, key: fajr.key, t: addDays(fajr.t, 1) },
    };
  }
  if (nextIdx === 0) {
    // Before today's Fajr -> still in last night's Isha window.
    const isha = items[items.length - 1];
    return {
      current: { ...isha, t: addDays(isha.t, -1) },
      next: { name: items[0].name, key: items[0].key, t: items[0].t },
    };
  }
  return {
    current: items[nextIdx - 1],
    next: { name: items[nextIdx].name, key: items[nextIdx].key, t: items[nextIdx].t },
  };
}

function renderList() {
  const items = fardhEntries();
  const cn = currentAndNext();
  const activeKey = cn?.current.key;
  const list = $("#plist");
  list.innerHTML = "";
  for (const e of items) {
    const row = document.createElement("div");
    row.className = "prow" + (e.key === activeKey ? " active" : "");
    row.dataset.key = e.key;
    const t = new Date(e.iso);
    row.innerHTML = `<span class="picon">${ICONS[e.key] ?? ""}</span>
      <span class="pname">${e.name}</span>
      <span class="eq"><i></i><i></i><i></i></span>
      <span class="ptime">${to12h(t)}</span>
      <button class="row-stop" title="Stop athan">
        <svg width="13" height="13" viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="6" width="12" height="12" rx="2"/></svg>
      </button>`;
    row.querySelector(".row-stop")!.addEventListener("click", () => invoke("stop_audio"));
    list.appendChild(row);
  }
  applyPlaybackUI();
}

/** Reflect the backend playback state: morph preview buttons and light up the ringing row. */
function applyPlaybackUI() {
  $("#preview-style").classList.toggle("is-playing", playing?.kind === "preview-style");
  $("#preview-dua").classList.toggle("is-playing", playing?.kind === "preview-dua");
  const ringingKey = playing?.kind === "athan" ? playing.prayer : null;
  document.querySelectorAll<HTMLElement>(".prow").forEach((row) => {
    row.classList.toggle("ringing", row.dataset.key === ringingKey);
  });
}

function renderHeader() {
  const loc = lastTimes?.location;
  $("#city").textContent = loc ? loc.city || "Unknown" : "Detecting…";
}

/** Quadratic bézier matching the dashed arc: P0(-10,200) P1(195,-30) P2(400,200). */
function arcPoint(t: number): { left: number; top: number } {
  const mt = 1 - t;
  const x = mt * mt * -10 + 2 * mt * t * 195 + t * t * 400;
  const y = mt * mt * 200 + 2 * mt * t * -30 + t * t * 200;
  return { left: (x / 390) * 100, top: (y / 210) * 100 };
}

function entryDate(key: string): Date | null {
  const e = lastTimes?.entries.find((x) => x.key === key);
  return e ? new Date(e.iso) : null;
}

/** Position sun/moon and switch the sky between day and night by the real time. */
function updateSky() {
  const sky = $("#sky");
  const sun = $("#sun");
  const moon = $("#moon");
  const sunrise = entryDate("sunrise");
  const maghrib = entryDate("maghrib");
  const fajr = entryDate("fajr");
  if (!sunrise || !maghrib || !fajr) return;

  const now = new Date();
  const isDay = now >= sunrise && now < maghrib;

  if (isDay) {
    sky.classList.remove("night");
    sun.classList.remove("hide");
    moon.classList.add("hide");
    const t = (now.getTime() - sunrise.getTime()) / (maghrib.getTime() - sunrise.getTime());
    const p = arcPoint(Math.min(1, Math.max(0, t)));
    sun.style.left = `${p.left}%`;
    sun.style.top = `${p.top}%`;
  } else {
    sky.classList.add("night");
    sun.classList.add("hide");
    moon.classList.remove("hide");
    // Night spans maghrib → next fajr (handle both before-sunrise and after-maghrib).
    const start = now < sunrise ? addDays(maghrib, -1) : maghrib;
    const end = now < sunrise ? fajr : addDays(fajr, 1);
    const t = (now.getTime() - start.getTime()) / (end.getTime() - start.getTime());
    const p = arcPoint(Math.min(1, Math.max(0, t)));
    moon.style.left = `${p.left}%`;
    moon.style.top = `${p.top}%`;
  }
}

function buildStars() {
  const stars = $("#stars");
  if (stars.childElementCount) return;
  for (let i = 0; i < 28; i++) {
    const s = document.createElement("div");
    s.className = "star";
    const size = 1 + Math.random() * 2;
    s.style.width = s.style.height = `${size}px`;
    s.style.left = `${Math.random() * 100}%`;
    s.style.top = `${Math.random() * 65}%`;
    s.style.animationDelay = `${Math.random() * 3}s`;
    stars.appendChild(s);
  }
}

/** Rising particles drifting up the sky, behind the sun (variant C). */
function buildParticles() {
  const sky = $("#sky");
  const sun = $("#sun");
  for (let i = 0; i < 14; i++) {
    const d = document.createElement("div");
    d.className = "p";
    const size = 2 + Math.random() * 3;
    d.style.width = d.style.height = `${size}px`;
    d.style.left = `${Math.random() * 100}%`;
    d.style.animationDuration = `${6 + Math.random() * 7}s`;
    d.style.animationDelay = `${Math.random() * 7}s`;
    sky.insertBefore(d, sun);
  }
}

/** Runs every second: updates hero + countdown banner. */
function tick() {
  const cn = currentAndNext();
  if (!cn) {
    $("#curName").textContent = lastTimes?.location ? "—" : "Locating…";
    $("#curTime").textContent = "";
    $("#countbar").textContent = lastTimes?.location ? "No times available" : "Detecting location…";
    return;
  }
  $("#curName").textContent = cn.current.name;
  $("#curTime").textContent = to12h(cn.current.t);

  const remaining = cn.next.t.getTime() - Date.now();
  $("#countbar").innerHTML = `next · ${cn.next.name} in <b>${fmtDuration(remaining)}</b>`;

  // Refresh from backend when the countdown elapses (prayer just passed).
  if (remaining <= 0) refreshTimes();
}

async function refreshTimes() {
  lastTimes = await invoke<TimesResponse>("get_times");
  renderHeader();
  renderList();
  updateSky();
  tick();
  fitWindow(); // list rows can appear/disappear (e.g. after location detection)
}

// ---------------- settings rendering ----------------
function renderMadhab() {
  document.querySelectorAll<HTMLButtonElement>(".seg button[data-madhab]").forEach((b) => {
    b.classList.toggle("on", b.dataset.madhab === settings.madhab);
  });
}

function renderVolume() {
  const el = $<HTMLInputElement>("#volume");
  el.value = String(settings.volume);
  el.style.setProperty("--v", `${Math.round(settings.volume * 100)}%`);
}

/** Rebuild the custom style dropdown: button label + menu items. */
function renderStyleDropdown() {
  const menu = $("#style-dd-menu");
  menu.innerHTML = "";
  for (const s of styleOptions) {
    const it = document.createElement("div");
    it.className = "dd-item" + (s.key === settings.selected_style ? " sel" : "");
    it.textContent = s.label;
    it.addEventListener("click", () => {
      settings.selected_style = s.key;
      renderStyleDropdown();
      $("#style-dd").classList.remove("open");
      save();
    });
    menu.appendChild(it);
  }
  const current = styleOptions.find((s) => s.key === settings.selected_style);
  $("#style-dd-val").textContent = current?.label ?? settings.selected_style;
}

async function populate() {
  settings = await invoke<Settings>("get_settings");

  styleOptions = await invoke<StyleOption[]>("list_styles");
  renderStyleDropdown();

  $<HTMLInputElement>("#dua").checked = settings.play_dua_after;
  $<HTMLInputElement>("#autostart").checked = settings.autostart;
  renderVolume();
  renderMadhab();

  await refreshTimes();
  updateLocHint();

  getVersion().then((v) => {
    appVersion = v;
    setUpdateHint();
  });
}

// Current app version, captured once at startup.
let appVersion = "";

// Render the update hint: always shows the version, with an optional status
// suffix (e.g. "Checking…", "Up to date") so the version never disappears.
function setUpdateHint(status?: string) {
  const base = appVersion ? `Version ${appVersion}` : "";
  $("#update-hint").textContent = status
    ? base
      ? `${base} — ${status}`
      : status
    : base;
}

function updateLocHint() {
  const loc = lastTimes?.location ?? settings.location;
  $("#loc-hint").textContent = loc ? `${loc.city || "Unknown"}, ${loc.country_code}` : "Not set";
}

// ---------------- navigation ----------------
const WIN_W = 390;
let winH = 0;

/** Resize the window to hug the active card so there's no dead space around it.
 *  Growing happens before the slide-in; shrinking waits for the slide-out. */
function fitWindow() {
  const active = document.body.classList.contains("show-settings")
    ? $("#view-settings")
    : $("#view-main");
  const h = active.offsetHeight;
  window.scrollTo(0, 0); // focus on off-screen controls can scroll the viewport; keep the card pinned
  if (!h || h === winH) return;
  const grow = h > winH || winH === 0;
  winH = h;
  const apply = () => {
    if (winH === h) getCurrentWindow().setSize(new LogicalSize(WIN_W, h));
  };
  if (grow) apply();
  else setTimeout(apply, 420); // let the 0.4s view transition finish first
}

function showSettings(on: boolean) {
  document.body.classList.toggle("show-settings", on);
  fitWindow();
}

// ---------------- wiring ----------------
function wire() {
  // navigation (in-window)
  $("#gear").addEventListener("click", () => showSettings(true));
  $("#back").addEventListener("click", () => showSettings(false));
  $("#close").addEventListener("click", () => getCurrentWindow().hide());

  // navigation driven from the tray (left-click → card, "Open Settings" → settings)
  listen<string>("navigate", (e) => showSettings(e.payload === "settings"));

  // background location detect finished → pull fresh times right away
  listen("location-updated", () => {
    refreshTimes();
    updateLocHint();
  });

  // playback state from the backend (scheduled athans + previews)
  listen<PlaybackStarted>("playback-started", (e) => {
    playing = e.payload;
    applyPlaybackUI();
  });
  listen("playback-ended", () => {
    playing = null;
    applyPlaybackUI();
  });

  // style dropdown (custom; items are wired in renderStyleDropdown)
  const dd = $("#style-dd");
  $("#style-dd-btn").addEventListener("click", (e) => {
    e.stopPropagation();
    dd.classList.toggle("open");
  });
  document.addEventListener("click", () => dd.classList.remove("open"));

  // dua
  $("#dua").addEventListener("change", (e) => {
    settings.play_dua_after = (e.target as HTMLInputElement).checked;
    save();
  });

  // autostart
  $("#autostart").addEventListener("change", (e) => {
    settings.autostart = (e.target as HTMLInputElement).checked;
    save();
  });

  // volume — live fill while dragging, save on release
  const vol = $<HTMLInputElement>("#volume");
  vol.addEventListener("input", (e) => {
    const v = parseFloat((e.target as HTMLInputElement).value);
    settings.volume = v;
    vol.style.setProperty("--v", `${Math.round(v * 100)}%`);
    invoke("set_volume", { volume: v }); // live-adjust anything currently playing
  });
  vol.addEventListener("change", save);

  // madhab segmented control
  document.querySelectorAll<HTMLButtonElement>(".seg button[data-madhab]").forEach((b) =>
    b.addEventListener("click", () => {
      settings.madhab = b.dataset.madhab!;
      renderMadhab();
      save().then(refreshTimes);
    })
  );

  // previews — the pressed button morphs into a stop button while its audio plays
  $("#preview-style").addEventListener("click", () => {
    if (playing?.kind === "preview-style") invoke("stop_audio");
    else invoke("test_play", { style: settings.selected_style });
  });
  $("#preview-dua").addEventListener("click", () => {
    if (playing?.kind === "preview-dua") invoke("stop_audio");
    else invoke("test_dua");
  });

  // re-detect location (saves silently; failures show in the hint line)
  $("#redetect").addEventListener("click", async () => {
    $("#loc-hint").textContent = "Detecting…";
    $("#city").textContent = "Detecting…";
    try {
      await invoke("redetect_location");
    } catch {
      $("#loc-hint").textContent = "Detection failed";
      await refreshTimes();
      return;
    }
    await refreshTimes();
    updateLocHint();
  });

  // check for updates (installs + relaunches if one is found)
  const checkBtn = $<HTMLButtonElement>("#check-update");
  checkBtn.addEventListener("click", async () => {
    checkBtn.disabled = true;
    setUpdateHint("Checking…");
    try {
      const version = await invoke<string | null>("check_for_updates");
      setUpdateHint(version ? `updating to v${version}…` : "up to date");
    } catch (e) {
      setUpdateHint("check failed");
      console.error(e);
    } finally {
      checkBtn.disabled = false;
    }
  });
}

// Disable the native right-click context menu across the whole app.
window.addEventListener("contextmenu", (e) => e.preventDefault());

window.addEventListener("DOMContentLoaded", async () => {
  buildStars();
  buildParticles();
  wire();
  await populate();
  fitWindow();
  setInterval(tick, 1000); // live countdown + hero
  setInterval(updateSky, 60_000); // reposition sun/moon
  setInterval(refreshTimes, 60_000); // keep times fresh (e.g. after midnight)
});
