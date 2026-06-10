use crate::{config, location, prayer, tray, AppState};
use chrono::Local;
use salah::prelude::PrayerTimes;
use std::collections::HashSet;
use std::time::Duration;
use tauri::{AppHandle, Manager};

const TICK: Duration = Duration::from_secs(20);
/// How long after a prayer time we still consider it "just fired" (guards against
/// missed ticks, e.g. after the laptop wakes from sleep).
const FIRE_WINDOW_SECS: i64 = 120;

/// Background loop: computes today's times, fires the athan at each prayer, and
/// keeps the tray tooltip/menu up to date. Tick-based so it survives sleep/clock jumps.
pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        let mut fired: HashSet<String> = HashSet::new();
        let mut last_sig = String::new();
        // Cache the computed times, keyed by (date, location, method, madhab), so
        // salah only runs when the inputs actually change.
        let mut cache_key = String::new();
        let mut cached: Option<PrayerTimes> = None;

        loop {
            let settings: config::Settings = {
                let state = app.state::<AppState>();
                let s = state.settings.lock().unwrap();
                s.clone()
            };

            let Some(loc) = settings.location.clone() else {
                tray::set_tooltip(&app, "Athan — detecting location…");
                std::thread::sleep(TICK);
                continue;
            };

            let method = settings
                .method
                .clone()
                .unwrap_or_else(|| location::method_for_country(&loc.country_code).to_string());
            let today = Local::now().date_naive();

            let key = format!(
                "{today}|{:.4}|{:.4}|{method}|{}",
                loc.lat, loc.lon, settings.madhab
            );
            if key != cache_key {
                cache_key = key;
                cached = prayer::try_compute(&loc, &method, &settings.madhab, today);
                fired.clear();
                last_sig.clear();
            }

            let Some(pt) = cached else {
                tray::set_tooltip(&app, "Athan — prayer times unavailable for this location/date");
                std::thread::sleep(TICK);
                continue;
            };

            let now = Local::now();
            for (k, time) in prayer::fardh_times(&pt) {
                if fired.contains(&k) {
                    continue;
                }
                let delta = (now - time).num_seconds();
                if (0..FIRE_WINDOW_SECS).contains(&delta) {
                    fired.insert(k.clone());
                    crate::fire_prayer(&app, &k);
                }
            }

            tray::update(&app, &pt, &mut last_sig);
            std::thread::sleep(TICK);
        }
    });
}
