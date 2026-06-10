use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
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

pub fn load(app: &AppHandle) -> Settings {
    match fs::read_to_string(config_path(app)) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => Settings::default(),
    }
}

pub fn save(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(config_path(app), json).map_err(|e| e.to_string())
}
