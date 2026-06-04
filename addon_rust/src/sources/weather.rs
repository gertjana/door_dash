//! Weather source — current conditions + daily/hourly forecasts.
//!
//! Reads a Home Assistant `weather.*` entity for the live state and
//! calls `weather.get_forecasts` (HA 2024.x+) for the forecast lists.
//! Falls back to a synthesized fixture when HA is unreachable so the
//! dashboard always renders during local dev.
//!
//! Mirrors `addon/app/sources/weather.py`.

use chrono::{DateTime, Duration, NaiveDateTime, NaiveTime, Timelike, Utc};
use serde_json::{json, Value};
use tracing::{info, warn};

use crate::config::Settings;
use crate::ha_client::HAClient;

/// One forecast point — daily or hourly. For hourly entries HA only
/// populates `temp_high`; daily entries usually include both highs
/// and lows.
#[derive(Debug, Clone, PartialEq)]
pub struct ForecastEntry {
    pub when: DateTime<Utc>,
    pub condition: String,
    pub temp_high: Option<f64>,
    pub temp_low: Option<f64>,
    pub precipitation_probability: Option<u8>,
}

/// Current conditions + forecasts.
#[derive(Debug, Clone, PartialEq)]
pub struct Weather {
    pub condition: String,
    pub temperature: Option<f64>,
    pub temperature_unit: String,
    pub precipitation_probability: Option<u8>,
    pub wind_speed: Option<f64>,
    pub wind_unit: String,
    pub humidity: Option<f64>,
    pub pressure: Option<f64>,
    pub pressure_unit: String,
    pub wind_bearing: Option<f64>,
    pub last_updated: Option<DateTime<Utc>>,
    pub icon: Option<String>,
    pub forecast: Vec<ForecastEntry>,
    pub forecast_hourly: Vec<ForecastEntry>,
}

const COMPASS_16: [&str; 16] = [
    "N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE", "S", "SSW", "SW", "WSW", "W", "WNW", "NW",
    "NNW",
];

/// Convert a wind bearing in degrees (0 = N, clockwise) to a 16-point
/// compass label. Returns `None` for non-finite or missing bearings.
pub fn bearing_to_cardinal(bearing: Option<f64>) -> Option<&'static str> {
    let b = bearing?;
    if !b.is_finite() {
        return None;
    }
    let idx = ((b / 22.5).round() as i64).rem_euclid(16) as usize;
    Some(COMPASS_16[idx])
}

fn to_float(v: Option<&Value>) -> Option<f64> {
    let v = v?;
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn to_u8(v: Option<&Value>) -> Option<u8> {
    let f = to_float(v)?;
    if !(0.0..=255.0).contains(&f) {
        return None;
    }
    Some(f.round() as u8)
}

fn to_string(v: Option<&Value>) -> Option<String> {
    v?.as_str().map(str::to_owned)
}

/// Parse an HA datetime string. HA emits RFC 3339 with either `Z` or
/// an explicit offset; chrono accepts both via `parse_from_rfc3339`.
fn parse_dt(s: Option<&Value>) -> Option<DateTime<Utc>> {
    let s = s?.as_str()?;
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn parse_forecast_entries(raw: &Value) -> Vec<ForecastEntry> {
    let Some(arr) = raw.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|f| {
            let when = parse_dt(f.get("datetime"))?;
            Some(ForecastEntry {
                when,
                condition: f
                    .get("condition")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_owned(),
                temp_high: to_float(f.get("temperature")),
                temp_low: to_float(f.get("templow")),
                precipitation_probability: to_u8(f.get("precipitation_probability")),
            })
        })
        .collect()
}

async fn fetch_forecast(ha: &HAClient, entity_id: &str, kind: &str) -> Vec<ForecastEntry> {
    let resp = ha
        .call_service(
            "weather",
            "get_forecasts",
            Some(json!({ "entity_id": entity_id, "type": kind })),
            true,
        )
        .await;
    let Some(resp) = resp else {
        return Vec::new();
    };
    // Response shape: {"service_response": {"<entity_id>": {"forecast": [...]}}}
    // Some HA versions skip the outer wrapper.
    let service_resp = resp.get("service_response").unwrap_or(&resp);
    let Some(entity_resp) = service_resp.get(entity_id) else {
        return Vec::new();
    };
    let Some(forecast) = entity_resp.get("forecast") else {
        return Vec::new();
    };
    parse_forecast_entries(forecast)
}

fn build_fallback_forecast() -> Vec<ForecastEntry> {
    // Daily fallback anchored on today so dates stay current in dev.
    let today = Utc::now().date_naive();
    let samples: &[(&str, f64, f64, u8)] = &[
        ("sunny", 22.0, 12.0, 5),
        ("partlycloudy", 20.0, 11.0, 20),
        ("rainy", 17.0, 10.0, 70),
        ("cloudy", 19.0, 9.0, 30),
        ("partlycloudy", 21.0, 10.0, 10),
        ("sunny", 24.0, 13.0, 0),
        ("partlycloudy", 23.0, 13.0, 5),
    ];
    samples
        .iter()
        .enumerate()
        .map(|(i, &(cond, hi, lo, pp))| {
            let date = today + Duration::days(i as i64 + 1);
            let dt = NaiveDateTime::new(date, NaiveTime::from_hms_opt(0, 0, 0).unwrap());
            ForecastEntry {
                when: DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc),
                condition: cond.to_owned(),
                temp_high: Some(hi),
                temp_low: Some(lo),
                precipitation_probability: Some(pp),
            }
        })
        .collect()
}

fn build_fallback_hourly() -> Vec<ForecastEntry> {
    // Smoothly varying temps so dev renders look believable rather
    // than randomly noisy. Starts on the next hour boundary.
    let now = Utc::now();
    let base = now.date_naive().and_hms_opt(now.hour(), 0, 0).unwrap() + Duration::hours(1);
    let conds = [
        "partlycloudy",
        "partlycloudy",
        "cloudy",
        "cloudy",
        "rainy",
        "rainy",
        "cloudy",
        "partlycloudy",
        "clear-night",
        "clear-night",
        "clear-night",
        "clear-night",
    ];
    (0..12)
        .map(|i| {
            let when =
                DateTime::<Utc>::from_naive_utc_and_offset(base + Duration::hours(i as i64), Utc);
            ForecastEntry {
                when,
                condition: conds[i % conds.len()].to_owned(),
                temp_high: Some((18.0 - (i as f64) * 0.35 * 10.0).round() / 10.0),
                temp_low: None,
                precipitation_probability: Some(if (4..=5).contains(&i) { 40 } else { 10 }),
            }
        })
        .collect()
}

fn fallback() -> Weather {
    Weather {
        condition: "partlycloudy".to_owned(),
        temperature: Some(18.5),
        temperature_unit: "°C".to_owned(),
        precipitation_probability: Some(20),
        wind_speed: Some(13.0),
        wind_unit: "km/h".to_owned(),
        humidity: Some(66.0),
        pressure: Some(1014.2),
        pressure_unit: "hPa".to_owned(),
        wind_bearing: Some(292.5), // WNW
        last_updated: Some(Utc::now() - Duration::hours(7)),
        icon: None,
        forecast: build_fallback_forecast(),
        forecast_hourly: build_fallback_hourly(),
    }
}

/// Fetch the current weather + forecasts for the configured HA
/// `weather.*` entity. Falls back to a synthesised fixture when HA
/// is unreachable or returns no state.
pub async fn fetch(settings: &Settings) -> Weather {
    let ha = HAClient::new(settings);
    let Some(state) = ha.get_state(&settings.weather_entity).await else {
        warn!(
            entity = %settings.weather_entity,
            available = ha.available(),
            base = %settings.ha_base_url,
            "weather: no state for entity, using fallback"
        );
        return fallback();
    };
    let attrs = state
        .get("attributes")
        .cloned()
        .unwrap_or_else(|| Value::Object(Default::default()));

    let forecast = fetch_forecast(&ha, &settings.weather_entity, "daily").await;
    let forecast_hourly = fetch_forecast(&ha, &settings.weather_entity, "hourly").await;

    let last_updated =
        parse_dt(state.get("last_updated")).or_else(|| parse_dt(state.get("last_changed")));

    let condition = state
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_owned();

    info!(
        entity = %settings.weather_entity,
        state = %condition,
        daily = forecast.len(),
        hourly = forecast_hourly.len(),
        "weather: fetched"
    );

    Weather {
        condition,
        temperature: to_float(attrs.get("temperature")),
        temperature_unit: to_string(attrs.get("temperature_unit"))
            .unwrap_or_else(|| "°C".to_owned()),
        precipitation_probability: to_u8(attrs.get("precipitation_probability")),
        wind_speed: to_float(attrs.get("wind_speed")),
        wind_unit: to_string(attrs.get("wind_speed_unit")).unwrap_or_else(|| "km/h".to_owned()),
        humidity: to_float(attrs.get("humidity")),
        pressure: to_float(attrs.get("pressure")),
        pressure_unit: to_string(attrs.get("pressure_unit")).unwrap_or_else(|| "hPa".to_owned()),
        wind_bearing: to_float(attrs.get("wind_bearing")),
        last_updated,
        icon: to_string(attrs.get("icon")),
        forecast,
        forecast_hourly,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearing_north() {
        assert_eq!(bearing_to_cardinal(Some(0.0)), Some("N"));
        assert_eq!(bearing_to_cardinal(Some(360.0)), Some("N"));
    }

    #[test]
    fn bearing_compass_octants() {
        // Each cardinal/intercardinal sits at a multiple of 22.5°.
        assert_eq!(bearing_to_cardinal(Some(90.0)), Some("E"));
        assert_eq!(bearing_to_cardinal(Some(180.0)), Some("S"));
        assert_eq!(bearing_to_cardinal(Some(270.0)), Some("W"));
        assert_eq!(bearing_to_cardinal(Some(292.5)), Some("WNW"));
    }

    #[test]
    fn bearing_negative_handles_modulo() {
        assert_eq!(bearing_to_cardinal(Some(-90.0)), Some("W"));
    }

    #[test]
    fn bearing_none_for_nan_or_missing() {
        assert!(bearing_to_cardinal(None).is_none());
        assert!(bearing_to_cardinal(Some(f64::NAN)).is_none());
        assert!(bearing_to_cardinal(Some(f64::INFINITY)).is_none());
    }

    #[test]
    fn parse_forecast_entries_drops_invalid_timestamps() {
        let raw = json!([
            {"datetime": "2026-05-26T00:00:00+00:00", "condition": "sunny", "temperature": 22.0},
            {"datetime": "not-a-date", "condition": "rainy"},
            {"condition": "cloudy"},
        ]);
        let parsed = parse_forecast_entries(&raw);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].condition, "sunny");
        assert_eq!(parsed[0].temp_high, Some(22.0));
    }

    #[test]
    fn parse_forecast_entries_handles_z_suffix() {
        let raw = json!([{"datetime": "2026-05-26T12:34:56Z", "condition": "sunny"}]);
        let parsed = parse_forecast_entries(&raw);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].when.to_rfc3339(), "2026-05-26T12:34:56+00:00");
    }

    #[test]
    fn parse_forecast_entries_uses_unknown_for_missing_condition() {
        let raw = json!([{"datetime": "2026-05-26T00:00:00Z"}]);
        let parsed = parse_forecast_entries(&raw);
        assert_eq!(parsed[0].condition, "unknown");
    }

    #[test]
    fn fallback_returns_populated_weather() {
        let w = fallback();
        assert_eq!(w.condition, "partlycloudy");
        assert!(w.temperature.is_some());
        assert!(
            !w.forecast.is_empty(),
            "fallback should include daily forecast"
        );
        assert!(
            !w.forecast_hourly.is_empty(),
            "fallback should include hourly forecast"
        );
    }

    #[tokio::test]
    async fn fetch_returns_fallback_when_ha_unavailable() {
        let settings = Settings {
            supervisor_token: None,
            ..Settings::default()
        };
        let w = fetch(&settings).await;
        assert_eq!(w.condition, "partlycloudy"); // fallback default
    }
}
