# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```powershell
npm install                   # install JS deps
npm run tauri dev             # run with hot-reload (Rust + TS together)
npm run tauri build           # produce a release installer
cd src-tauri; cargo test      # prayer-calc + audio-decode unit tests
cd src-tauri; cargo check     # fast type-check
cd src-tauri; cargo clippy    # lint
npm run build                 # tsc + vite build (catches TS errors without launching the app)
```

## Architecture

Tauri v2 desktop app: Rust backend + vanilla TypeScript frontend (no framework). The app lives in the system tray; the webview window (390 px wide) is a prayer-card / settings panel that hides rather than closes.

### Rust modules (`src-tauri/src/`)

| File | Responsibility |
|------|---------------|
| `lib.rs` | `AppState`, all Tauri commands, `fire_prayer`, `run()` entry point |
| `config.rs` | `Settings` struct, `load`/`save` to `%APPDATA%\com.jawad.athandesktop\settings.json` |
| `location.rs` | IP geolocation (`ip-api.com`), country → calculation-method mapping |
| `prayer.rs` | Wraps the `salah` crate: `try_compute`, `entries`, `fardh_times` |
| `audio.rs` | Dedicated audio thread (rodio); `AudioCmd` channel for Play / Stop / SetVolume |
| `scheduler.rs` | 20 s tick loop: fires the athan at prayer time, keeps tray tooltip/menu current |
| `tray.rs` | Tray icon, context menu, tooltip |

### Frontend (`src/`)

Single `main.ts` file (vanilla TS): prayer list, animated sky (day/night with bezier-arc sun/moon), settings UI. Communicates with Rust exclusively via `invoke` and `listen` from `@tauri-apps/api`.

### Key invariants

- `AppState` holds `Mutex<Settings>` + `Mutex<Sender<AudioCmd>>` managed via Tauri state.
- All audio goes through `send_audio(app, AudioCmd::…)` in `lib.rs`; audio lives on a non-Send dedicated thread.
- Scheduler caches `PrayerTimes` keyed by `date|lat|lon|method|madhab` — recomputes only on change.
- Prayer fire window: 0–20 s after prayer time, deduped per day by a `HashSet<String>`.
- Fajr always uses `athan_fajr.mp3`; other prayers use the user-selected style file.
- Window close is intercepted (`CloseRequested`) → `window.hide()`. Quit uses `w.destroy()` then `app.exit(0)` (avoids WebView2 warnings on Windows).
- Frontend navigation between main/settings is driven by a `"navigate"` Tauri event (payload `"main"` or `"settings"`).
- The Rust `Settings` struct in `config.rs` and the TypeScript `Settings` interface in `src/main.ts` must stay in sync.
