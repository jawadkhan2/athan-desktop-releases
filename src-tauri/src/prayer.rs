use crate::config::Location;
use chrono::{DateTime, Local, NaiveDate};
use salah::prelude::*;
use serde::Serialize;

pub fn method_from_key(key: &str) -> Method {
    match key {
        "muslim_world_league" => Method::MuslimWorldLeague,
        "egyptian" => Method::Egyptian,
        "karachi" => Method::Karachi,
        "umm_al_qura" => Method::UmmAlQura,
        "dubai" => Method::Dubai,
        "moonsighting" => Method::MoonsightingCommittee,
        "north_america" => Method::NorthAmerica,
        "kuwait" => Method::Kuwait,
        "qatar" => Method::Qatar,
        "singapore" => Method::Singapore,
        "tehran" => Method::Tehran,
        "turkey" => Method::Turkey,
        _ => Method::MuslimWorldLeague,
    }
}

pub fn madhab_from_str(s: &str) -> Madhab {
    match s {
        "hanafi" => Madhab::Hanafi,
        _ => Madhab::Shafi,
    }
}

/// One prayer row for display (times rendered in the machine's local timezone).
#[derive(Clone, Serialize)]
pub struct PrayerEntry {
    pub key: String,
    pub name: String,
    pub time: String, // "HH:MM"
    pub iso: String,  // RFC3339 local
    pub is_fardh: bool,
}

/// Compute prayer times. Returns `None` if the calculation is undefined for the
/// location/date — e.g. high-latitude summer nights where Fajr/Isha never reach
/// the twilight angle (salah panics internally there, so we catch it).
pub fn try_compute(
    loc: &Location,
    method_key: &str,
    madhab: &str,
    date: NaiveDate,
) -> Option<PrayerTimes> {
    let (lat, lon) = (loc.lat, loc.lon);
    let method = method_from_key(method_key);
    let madhab = madhab_from_str(madhab);
    std::panic::catch_unwind(move || {
        let coords = Coordinates::new(lat, lon);
        let params = Configuration::with(method, madhab);
        PrayerTimes::new(date, coords, params)
    })
    .ok()
}

const DISPLAY: [(Prayer, &str, bool); 6] = [
    (Prayer::Fajr, "fajr", true),
    (Prayer::Sunrise, "sunrise", false),
    (Prayer::Dhuhr, "dhuhr", true),
    (Prayer::Asr, "asr", true),
    (Prayer::Maghrib, "maghrib", true),
    (Prayer::Isha, "isha", true),
];

/// The 5 fardh prayers + sunrise, as local-time display rows.
pub fn entries(pt: &PrayerTimes) -> Vec<PrayerEntry> {
    DISPLAY
        .iter()
        .map(|(p, key, fardh)| {
            let t: DateTime<Local> = pt.time(*p).with_timezone(&Local);
            PrayerEntry {
                key: (*key).to_string(),
                name: p.name(),
                time: t.format("%H:%M").to_string(),
                iso: t.to_rfc3339(),
                is_fardh: *fardh,
            }
        })
        .collect()
}

/// The 5 fardh prayers with their local time, used by the scheduler for firing.
pub fn fardh_times(pt: &PrayerTimes) -> Vec<(String, DateTime<Local>)> {
    [
        (Prayer::Fajr, "fajr"),
        (Prayer::Dhuhr, "dhuhr"),
        (Prayer::Asr, "asr"),
        (Prayer::Maghrib, "maghrib"),
        (Prayer::Isha, "isha"),
    ]
    .iter()
    .map(|(p, k)| ((*k).to_string(), pt.time(*p).with_timezone(&Local)))
    .collect()
}

fn display_name(key: &str) -> String {
    match key {
        "fajr" => "Fajr".to_string(),
        "dhuhr" => "Dhuhr".to_string(),
        "asr" => "Asr".to_string(),
        "maghrib" => "Maghrib".to_string(),
        "isha" => "Isha".to_string(),
        _ => key.to_string(),
    }
}

/// The next prayer the app should announce or count down to: the 5 fardh
/// prayers only, never sunrise/qiyam from the salah crate's broader sequence.
pub fn next_fardh(pt: &PrayerTimes, now: DateTime<Local>) -> (String, String, DateTime<Local>) {
    if let Some((key, time)) = fardh_times(pt).into_iter().find(|(_, time)| *time > now) {
        return (key.clone(), display_name(&key), time);
    }

    (
        "fajr".to_string(),
        "Fajr".to_string(),
        pt.time(Prayer::FajrTomorrow).with_timezone(&Local),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Location;

    fn cairo() -> Location {
        Location { lat: 30.0444, lon: 31.2357, city: "Cairo".into(), country_code: "EG".into() }
    }

    #[test]
    fn times_are_ordered_through_the_day() {
        let date = NaiveDate::from_ymd_opt(2026, 6, 8).unwrap();
        let pt = try_compute(&cairo(), "egyptian", "shafi", date).expect("times should compute");

        let order = [
            Prayer::Fajr,
            Prayer::Sunrise,
            Prayer::Dhuhr,
            Prayer::Asr,
            Prayer::Maghrib,
            Prayer::Isha,
        ];
        for w in order.windows(2) {
            assert!(
                pt.time(w[0]) < pt.time(w[1]),
                "{:?} ({}) should be before {:?} ({})",
                w[0],
                pt.time(w[0]),
                w[1],
                pt.time(w[1]),
            );
        }
    }

    #[test]
    fn hanafi_asr_is_later_than_shafi() {
        let date = NaiveDate::from_ymd_opt(2026, 6, 8).unwrap();
        let shafi = try_compute(&cairo(), "egyptian", "shafi", date).unwrap();
        let hanafi = try_compute(&cairo(), "egyptian", "hanafi", date).unwrap();
        assert!(hanafi.time(Prayer::Asr) > shafi.time(Prayer::Asr));
    }

    #[test]
    fn high_latitude_summer_degrades_to_none_without_panicking() {
        // Tromsø, Norway in June: Fajr/Isha never reach the twilight angle.
        let tromso = Location {
            lat: 69.6492,
            lon: 18.9553,
            city: "Tromsø".into(),
            country_code: "NO".into(),
        };
        let date = NaiveDate::from_ymd_opt(2026, 6, 21).unwrap();
        assert!(try_compute(&tromso, "muslim_world_league", "shafi", date).is_none());
    }

    #[test]
    fn next_fardh_skips_sunrise() {
        let date = NaiveDate::from_ymd_opt(2026, 6, 8).unwrap();
        let pt = try_compute(&cairo(), "egyptian", "shafi", date).unwrap();
        let after_fajr = pt.time(Prayer::Fajr).with_timezone(&Local) + chrono::Duration::minutes(1);
        let (key, name, _) = next_fardh(&pt, after_fajr);
        assert_eq!(key, "dhuhr");
        assert_eq!(name, "Dhuhr");
    }

    #[test]
    fn next_fardh_after_isha_uses_tomorrows_fajr() {
        let date = NaiveDate::from_ymd_opt(2026, 6, 8).unwrap();
        let pt = try_compute(&cairo(), "egyptian", "shafi", date).unwrap();
        let after_isha = pt.time(Prayer::Isha).with_timezone(&Local) + chrono::Duration::minutes(1);
        let (key, name, time) = next_fardh(&pt, after_isha);
        assert_eq!(key, "fajr");
        assert_eq!(name, "Fajr");
        assert_eq!(time, pt.time(Prayer::FajrTomorrow).with_timezone(&Local));
    }
}
