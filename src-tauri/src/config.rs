use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Location {
    pub lat: f64,
    pub lon: f64,
    pub city: String,
    pub country_code: String,
}

/// User-facing settings, persisted as JSON in the app config dir.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Athan style key for the 4 daytime prayers: "makkah" | "madina" | "egypt" | "alaqsa".
    pub selected_style: String,
    /// Play the dua clip right after the athan finishes.
    pub play_dua_after: bool,
    /// Asr juristic method: "shafi" | "hanafi".
    pub madhab: String,
    /// Calculation method key, auto-derived from the detected country and cached.
    pub method: Option<String>,
    /// Playback volume 0.0..=1.0.
    pub volume: f32,
    /// Auto-detected location (cached).
    pub location: Option<Location>,
    /// Launch on login.
    pub autostart: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            selected_style: "makkah".into(),
            play_dua_after: true,
            madhab: "shafi".into(),
            method: None,
            volume: 1.0,
            location: None,
            autostart: false,
        }
    }
}

fn config_path(app: &AppHandle) -> PathBuf {
    let dir = app
        .path()
        .app_config_dir()
        .expect("could not resolve app config dir");
    let _ = fs::create_dir_all(&dir);
    dir.join("settings.json")
}

fn backup_path(path: &Path) -> PathBuf {
    path.with_extension("json.bak")
}

fn temp_path(path: &Path) -> PathBuf {
    path.with_extension("json.tmp")
}

fn read_settings(path: &Path) -> Result<Settings, String> {
    let raw = fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}

pub fn load(app: &AppHandle) -> Settings {
    let path = config_path(app);
    if !path.exists() {
        return Settings::default();
    }

    match read_settings(&path) {
        Ok(settings) => settings,
        Err(e) => {
            eprintln!("settings: could not load {path:?}: {e}");
            let backup = backup_path(&path);
            read_settings(&backup).unwrap_or_default()
        }
    }
}

pub fn save(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let path = config_path(app);
    let tmp = temp_path(&path);
    let backup = backup_path(&path);
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(&tmp, json).map_err(|e| e.to_string())?;

    if path.exists() {
        let _ = fs::copy(&path, &backup);
    }

    #[cfg(windows)]
    if path.exists() {
        fs::remove_file(&path).map_err(|e| e.to_string())?;
    }

    match fs::rename(&tmp, &path) {
        Ok(()) => Ok(()),
        Err(e) => {
            #[cfg(windows)]
            if backup.exists() && !path.exists() {
                let _ = fs::copy(&backup, &path);
            }
            Err(e.to_string())
        }
    }
}
