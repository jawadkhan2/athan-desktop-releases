# Athan Desktop

A lightweight system-tray app that computes Islamic prayer times **offline** and plays an
athan mp3 at each prayer. Built with Tauri v2 + Rust.

## Features

- **Offline prayer times** via the [`salah`](https://github.com/insha/salah) crate (high-precision
  astronomical calculation). High-latitude summer nights where Fajr/Isha are undefined degrade
  gracefully instead of crashing.
- **Auto-located** on first run via IP geolocation; the calculation method is auto-selected from the
  detected country (e.g. ISNA in North America, Umm al-Qura in Saudi Arabia, Egyptian in Egypt…).
- **Audio**: 4 selectable athan styles (Makkah, Madina, Egypt, Al-Aqsa), a dedicated **Fajr** athan,
  and an optional **dua** that plays right after the athan.
- **System tray**: next prayer + countdown in the tooltip, today's times in the menu, plus
  Stop / Settings / Re-detect / Quit. Closing the settings window keeps the app running in the tray.
- **Settings window** (webview): style, dua toggle, madhab (Shafi/Hanafi), per-prayer toggles,
  volume, launch-on-login, and a live view of today's times.

## Project layout

```
src/                       # settings-window frontend (vanilla TS)
src-tauri/
  resources/audio/*.mp3     # bundled athan + dua clips
  src/
    config.rs               # settings load/save (JSON in the app config dir)
    location.rs             # IP geolocation + country -> method mapping
    prayer.rs               # salah calculation (try_compute), display/scheduler helpers
    audio.rs                # rodio playback on a dedicated audio thread
    scheduler.rs            # 20s tick loop: fires the athan + updates the tray
    tray.rs                 # tray icon, menu, tooltip
    lib.rs                  # wiring, managed state, Tauri commands
```

Settings are stored at `%APPDATA%\com.jawad.athandesktop\settings.json` on Windows.

## Development

```bash
npm install
npm run tauri dev      # run with hot-reload
npm run tauri build    # produce a release installer
cd src-tauri && cargo test   # prayer-calc + audio-decode tests
```

## Replacing the audio

Drop your own mp3s into `src-tauri/resources/audio/` using these names:
`athan_makkah.mp3`, `athan_madina.mp3`, `athan_egypt.mp3`, `athan_alaqsa.mp3`,
`athan_fajr.mp3`, `dua_after_athan.mp3`. (The original `.wma` sources live in `sound/`.)
