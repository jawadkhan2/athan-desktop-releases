use crate::config::Location;
use serde::Deserialize;
use std::time::Duration;

#[derive(Deserialize)]
struct IpWhoIs {
    success: bool,
    latitude: Option<f64>,
    longitude: Option<f64>,
    city: Option<String>,
    country_code: Option<String>,
    message: Option<String>,
}

/// Detect approximate location from the public IP over HTTPS (no API key).
pub fn detect() -> Result<Location, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client
        .get("https://ipwho.is/?fields=success,latitude,longitude,city,country_code,message")
        .send()
        .map_err(|e| e.to_string())?
        .json::<IpWhoIs>()
        .map_err(|e| e.to_string())?;

    if !resp.success {
        return Err(resp.message.unwrap_or_else(|| "IP geolocation lookup failed".into()));
    }

    Ok(Location {
        lat: resp.latitude.ok_or("missing latitude")?,
        lon: resp.longitude.ok_or("missing longitude")?,
        city: resp.city.unwrap_or_default(),
        country_code: resp.country_code.unwrap_or_default(),
    })
}

/// Pick a sensible calculation-method key for a country (ISO 3166-1 alpha-2).
pub fn method_for_country(cc: &str) -> &'static str {
    match cc {
        "SA" => "umm_al_qura",
        "EG" | "SY" | "IQ" | "JO" | "LB" | "SD" | "LY" | "DZ" | "TN" | "MA" => "egyptian",
        "PK" | "IN" | "BD" | "AF" => "karachi",
        "US" | "CA" => "north_america",
        "AE" => "dubai",
        "KW" => "kuwait",
        "QA" => "qatar",
        "SG" | "MY" | "ID" => "singapore",
        "TR" => "turkey",
        "IR" => "tehran",
        _ => "muslim_world_league",
    }
}
