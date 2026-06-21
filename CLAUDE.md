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
| `audio.rs` | Dedicated audio thread (rodio); `AudioCmd` channel for Play / Stop / SetVolume. Opens a fresh `OutputStream` per play and auto-recovers from mid-athan device loss (resumes from the elapsed offset); keeps system + monitor awake during playback (`SetThreadExecutionState` + F15 wake nudge) so monitor-attached speakers can't be silenced by display sleep |
| `scheduler.rs` | 20 s tick loop: fires the athan at prayer time, keeps tray tooltip/menu current |
| `tray.rs` | Tray icon, context menu, tooltip |
| `updater.rs` | OTA auto-update thread: checks GitHub Releases 30 s after launch, then every 6 h; silent download + install with notification. Waits (polls `AppState.athan_playing`) until any athan finishes before installing, so a passive install never cuts off playback |

### Frontend (`src/`)

Single `main.ts` file (vanilla TS): prayer list, animated sky (day/night with bezier-arc sun/moon), settings UI. Communicates with Rust exclusively via `invoke` and `listen` from `@tauri-apps/api`.

### Key invariants

- `AppState` holds `Mutex<Settings>` + `Mutex<Sender<AudioCmd>>` managed via Tauri state.
- All audio goes through `send_audio(app, AudioCmd::…)` in `lib.rs`; audio lives on a non-Send dedicated thread.
- `AppState.athan_playing` (`Arc<AtomicBool>`) is set in `fire_prayer` and cleared by the audio thread's `on_ended` callback; the updater blocks installs while it is true.
- Scheduler caches `PrayerTimes` keyed by `date|lat|lon|method|madhab` — recomputes only on change.
- Prayer fire window: 0–20 s after prayer time, deduped per day by a `HashSet<String>`.
- Fajr always uses `athan_fajr.mp3`; other prayers use the user-selected style file.
- Window close is intercepted (`CloseRequested`) → `window.hide()`. Quit uses `w.destroy()` then `app.exit(0)` (avoids WebView2 warnings on Windows).
- Frontend navigation between main/settings is driven by a `"navigate"` Tauri event (payload `"main"` or `"settings"`).
- The Rust `Settings` struct in `config.rs` and the TypeScript `Settings` interface in `src/main.ts` must stay in sync.

## Releases & OTA updates

- Distribution: GitHub Releases on the **public** `jawadkhan2/athan-desktop-releases` repo (the code repo is private, so releases live separately). Users install once via the NSIS installer (`Athan_X.Y.Z_x64-setup.exe`, per-user, no UAC); after that the app self-updates via `tauri-plugin-updater` against that repo's `releases/latest/download/latest.json`.
- To release: bump the version in **all three** of `src-tauri/tauri.conf.json` (authoritative), `src-tauri/Cargo.toml`, `package.json`; run `cargo check` to refresh `Cargo.lock`; commit; `git tag vX.Y.Z`; push the tag. `.github/workflows/release.yml` builds, signs, and publishes the release (including `latest.json`) automatically.
- Updater artifacts are signed with the minisign key at `~\.tauri\athan.key` (no password), uploaded as the `TAURI_SIGNING_PRIVATE_KEY` repo secret. The matching pubkey is baked into `tauri.conf.json` — losing the private key strands all installed clients.
- CI publishes cross-repo using the `RELEASES_TOKEN` secret (fine-grained PAT with Contents read/write on `athan-desktop-releases`).
- Windows bundles NSIS only (no MSI): the updater needs passive install mode, and per-user install avoids UAC.
