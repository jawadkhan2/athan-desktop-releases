mod audio;
mod config;
mod location;
mod prayer;
mod scheduler;
mod tray;
#[cfg(desktop)]
mod updater;

use audio::AudioCmd;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use tauri::path::BaseDirectory;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_notification::NotificationExt;

pub struct AppState {
    pub settings: Mutex<config::Settings>,
    pub audio_tx: Mutex<Sender<AudioCmd>>,
    /// True while a scheduled athan (and its optional dua) is playing. The
    /// updater checks this so a passive install never stops the app mid-athan.
    pub athan_playing: Arc<AtomicBool>,
    /// Single-flight guard for background location detection.
    pub location_detecting: AtomicBool,
}

/// Resolve the bundled audio directory (falls back to the source tree in dev).
fn audio_dir(app: &AppHandle) -> PathBuf {
    if let Ok(p) = app.path().resolve("resources/audio", BaseDirectory::Resource) {
        if p.exists() {
            return p;
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/audio")
}

pub fn send_audio(app: &AppHandle, cmd: AudioCmd) {
    let state = app.state::<AppState>();
    let tx = state.audio_tx.lock().unwrap().clone();
    let _ = tx.send(cmd);
}

/// What is currently playing, broadcast to the UI as the `playback-started` event.
/// `kind` is "athan" (scheduled), "preview-style" or "preview-dua";
/// `prayer` is the prayer key for scheduled athans.
#[derive(Serialize, Clone)]
struct PlaybackStarted {
    kind: &'static str,
    prayer: Option<String>,
}

fn emit_playback_started(app: &AppHandle, kind: &'static str, prayer: Option<String>) {
    let _ = app.emit("playback-started", PlaybackStarted { kind, prayer });
}

/// Play the athan (Fajr uses its own clip) + optional dua, and show a notification.
pub fn fire_prayer(app: &AppHandle, key: &str) {
    let settings = {
        let state = app.state::<AppState>();
        let s = state.settings.lock().unwrap();
        s.clone()
    };
    let dir = audio_dir(app);

    let athan = if key == "fajr" {
        audio::FAJR_FILE.to_string()
    } else {
        audio::style_file(&settings.selected_style).to_string()
    };
    let mut paths = vec![dir.join(athan)];
    if settings.play_dua_after {
        paths.push(dir.join(audio::DUA_FILE));
    }
    app.state::<AppState>()
        .athan_playing
        .store(true, Ordering::SeqCst);
    send_audio(app, AudioCmd::Play {
        paths,
        volume: settings.volume,
        wake_display: true,
    });
    emit_playback_started(app, "athan", Some(key.to_string()));

    let name = match key {
        "fajr" => "Fajr",
        "dhuhr" => "Dhuhr",
        "asr" => "Asr",
        "maghrib" => "Maghrib",
        "isha" => "Isha",
        other => other,
    };
    let _ = app
        .notification()
        .builder()
        .title("Athan")
        .body(format!("It's time for {name} prayer"))
        .show();
}

/// Detect location in the background and persist it.
///
/// Retries with backoff: the network stack (or DNS) is often not ready in the
/// first seconds after launch — especially right after install or a reboot —
/// and a single failed lookup must not leave the app stuck on "Detecting…".
pub fn trigger_redetect(app: &AppHandle) {
    let state = app.state::<AppState>();
    if state.location_detecting.swap(true, Ordering::SeqCst) {
        return;
    }

    let handle = app.clone();
    std::thread::spawn(move || {
        // ~0, 3, 6, 12, 24, then 60 s steady: recover fast, then poll forever.
        let backoff = [3u64, 6, 12, 24];
        let mut attempt = 0usize;
        loop {
            match location::detect() {
                Ok(loc) => {
                    let state = handle.state::<AppState>();
                    let mut s = state.settings.lock().unwrap();
                    s.method =
                        Some(location::method_for_country(&loc.country_code).to_string());
                    s.location = Some(loc);
                    let _ = config::save(&handle, &s);
                    drop(s);
                    // Nudge the frontend to refresh immediately, not on its 60 s tick.
                    let _ = handle.emit("location-updated", ());
                    handle
                        .state::<AppState>()
                        .location_detecting
                        .store(false, Ordering::SeqCst);
                    return;
                }
                Err(_) => {
                    let delay = backoff.get(attempt).copied().unwrap_or(60);
                    attempt += 1;
                    std::thread::sleep(std::time::Duration::from_secs(delay));
                }
            }
        }
    });
}

fn apply_autostart(_app: &AppHandle, _enable: bool) {
    #[cfg(desktop)]
    {
        use tauri_plugin_autostart::ManagerExt;
        let mgr = _app.autolaunch();
        let res = if _enable { mgr.enable() } else { mgr.disable() };
        if let Err(e) = res {
            eprintln!("autostart: {} failed: {e}", if _enable { "enable" } else { "disable" });
        }
    }
}

// ---------------- Tauri commands ----------------

#[tauri::command]
fn get_settings(state: State<AppState>) -> config::Settings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
fn save_settings(
    app: AppHandle,
    state: State<AppState>,
    settings: config::Settings,
) -> Result<(), String> {
    apply_autostart(&app, settings.autostart);
    config::save(&app, &settings)?;
    *state.settings.lock().unwrap() = settings;
    Ok(())
}

#[derive(Serialize)]
struct TimesResponse {
    location: Option<config::Location>,
    method: Option<String>,
    entries: Vec<prayer::PrayerEntry>,
    next_key: Option<String>,
    next_name: Option<String>,
    next_time: Option<String>,
    next_iso: Option<String>,
}

#[tauri::command]
fn get_times(state: State<AppState>) -> TimesResponse {
    let s = state.settings.lock().unwrap().clone();
    let Some(loc) = s.location else {
        return TimesResponse {
            location: None,
            method: None,
            entries: vec![],
            next_key: None,
            next_name: None,
            next_time: None,
            next_iso: None,
        };
    };
    let method = s
        .method
        .clone()
        .unwrap_or_else(|| location::method_for_country(&loc.country_code).to_string());
    let today = chrono::Local::now().date_naive();
    let Some(pt) = prayer::try_compute(&loc, &method, &s.madhab, today) else {
        return TimesResponse {
            location: Some(loc),
            method: Some(method),
            entries: vec![],
            next_key: None,
            next_name: None,
            next_time: None,
            next_iso: None,
        };
    };
    let entries = prayer::entries(&pt);
    let (next_key, next_name, next_time) = prayer::next_fardh(&pt, chrono::Local::now());
    TimesResponse {
        location: Some(loc),
        method: Some(method),
        entries,
        next_key: Some(next_key),
        next_name: Some(next_name),
        next_time: Some(next_time.format("%H:%M").to_string()),
        next_iso: Some(next_time.to_rfc3339()),
    }
}

#[tauri::command]
fn redetect_location(
    app: AppHandle,
    state: State<AppState>,
) -> Result<config::Location, String> {
    let loc = location::detect()?;
    let mut s = state.settings.lock().unwrap();
    s.method = Some(location::method_for_country(&loc.country_code).to_string());
    s.location = Some(loc.clone());
    config::save(&app, &s)?;
    Ok(loc)
}

#[derive(Serialize)]
struct StyleOption {
    key: String,
    label: String,
}

#[tauri::command]
fn list_styles() -> Vec<StyleOption> {
    [("makkah", "Makkah"), ("madina", "Madina"), ("egypt", "Egypt"), ("alaqsa", "Al-Aqsa")]
        .iter()
        .map(|(k, l)| StyleOption { key: (*k).into(), label: (*l).into() })
        .collect()
}

#[tauri::command]
fn test_play(app: AppHandle, state: State<AppState>, style: String) {
    let volume = state.settings.lock().unwrap().volume;
    let dir = audio_dir(&app);
    let file = if style == "fajr" {
        audio::FAJR_FILE.to_string()
    } else {
        audio::style_file(&style).to_string()
    };
    send_audio(&app, AudioCmd::Play {
        paths: vec![dir.join(file)],
        volume,
        wake_display: false,
    });
    emit_playback_started(&app, "preview-style", None);
}

#[tauri::command]
fn test_dua(app: AppHandle, state: State<AppState>) {
    let volume = state.settings.lock().unwrap().volume;
    let dir = audio_dir(&app);
    send_audio(&app, AudioCmd::Play {
        paths: vec![dir.join(audio::DUA_FILE)],
        volume,
        wake_display: false,
    });
    emit_playback_started(&app, "preview-dua", None);
}

#[tauri::command]
fn stop_audio(app: AppHandle) {
    send_audio(&app, AudioCmd::Stop);
}

#[tauri::command]
fn set_volume(app: AppHandle, volume: f32) {
    send_audio(&app, AudioCmd::SetVolume(volume));
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();

    #[cfg(desktop)]
    {
        builder = builder
            .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }))
            .plugin(tauri_plugin_autostart::init(
                tauri_plugin_autostart::MacosLauncher::LaunchAgent,
                Some(vec!["--minimized"]),
            ))
            .plugin(tauri_plugin_updater::Builder::new().build());
    }

    builder
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            get_times,
            redetect_location,
            list_styles,
            test_play,
            test_dua,
            stop_audio,
            set_volume,
            updater::check_for_updates,
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Keep running in the tray instead of exiting.
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(|app| {
            let handle = app.handle().clone();
            let settings = config::load(&handle);

            let athan_playing = Arc::new(AtomicBool::new(false));
            let ended_handle = handle.clone();
            let ended_flag = athan_playing.clone();
            let audio_tx = audio::spawn(move || {
                // Playback drained or was stopped: clear the athan guard.
                ended_flag.store(false, Ordering::SeqCst);
                let _ = ended_handle.emit("playback-ended", ());
            });
            app.manage(AppState {
                settings: Mutex::new(settings.clone()),
                audio_tx: Mutex::new(audio_tx),
                athan_playing,
                location_detecting: AtomicBool::new(false),
            });

            apply_autostart(&handle, settings.autostart);
            tray::build(&handle)?;

            if settings.location.is_none() {
                trigger_redetect(&handle);
            }

            scheduler::spawn(handle.clone());

            #[cfg(desktop)]
            updater::spawn(handle.clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
