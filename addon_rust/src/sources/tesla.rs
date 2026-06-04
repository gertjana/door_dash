//! Tesla source — battery %, range, cabin temp + climate state.
//!
//! Each entity is independently optional; whichever is configured
//! gets read. When HA is unreachable, or no battery entity is
//! configured, returns demo fallback data so the widget always
//! renders during development / pre-setup.
//!
//! `climate_on` is `Some(true)` only when the climate entity reports
//! a non-`off` mode. While the car is asleep, the climate entity
//! goes `unknown`/`unavailable`; in that case `climate_on` is `None`
//! (we can't tell) and the widget should hide the indicator rather
//! than imply it's off.
//!
//! Mirrors `addon/app/sources/tesla.py`.

use serde_json::Value;
use tracing::debug;

use crate::config::Settings;
use crate::ha_client::HAClient;

/// Snapshot of the configured Tesla entities. Every field is
/// independently optional.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TeslaState {
    /// Battery state of charge, 0-100.
    pub battery_pct: Option<f64>,
    /// Estimated remaining range, kilometres.
    pub range_km: Option<f64>,
    /// Cabin / interior temperature in °C.
    pub inside_temp_c: Option<f64>,
    /// `Some(true)` if HVAC is running, `Some(false)` if explicitly
    /// off, `None` if the climate entity is `unknown`/`unavailable`
    /// (typical when the car is asleep).
    pub climate_on: Option<bool>,
}

const UNKNOWN_STATES: &[&str] = &["unknown", "unavailable", "none", ""];

impl TeslaState {
    /// Demo data used when HA is unreachable or no battery entity is
    /// configured. Matches the Python addon's fallback values so the
    /// dashboard renders identically across both implementations
    /// during development.
    pub fn fallback() -> Self {
        Self {
            battery_pct: Some(73.0),
            range_km: Some(309.0),
            inside_temp_c: None,
            climate_on: None,
        }
    }
}

/// Coerce a JSON state value (number or string) to `f64`. Returns
/// `None` for non-numeric strings, missing values, or sentinels like
/// `"unknown"`.
fn to_float(value: Option<&Value>) -> Option<f64> {
    let v = value?;
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => {
            let trimmed = s.trim();
            if UNKNOWN_STATES.contains(&trimmed.to_ascii_lowercase().as_str()) {
                return None;
            }
            trimmed.parse().ok()
        }
        _ => None,
    }
}

async fn read_float(ha: &HAClient, entity_id: &str) -> Option<f64> {
    if entity_id.is_empty() {
        return None;
    }
    let state = ha.get_state(entity_id).await?;
    to_float(state.get("state"))
}

async fn read_climate_on(ha: &HAClient, entity_id: &str) -> Option<bool> {
    if entity_id.is_empty() {
        return None;
    }
    let state = ha.get_state(entity_id).await?;
    let raw = state
        .get("state")
        .and_then(Value::as_str)?
        .trim()
        .to_ascii_lowercase();
    if UNKNOWN_STATES.contains(&raw.as_str()) {
        return None;
    }
    Some(raw != "off")
}

/// Fetch the current Tesla snapshot from HA, falling back to demo
/// data when the integration isn't available.
pub async fn fetch(settings: &Settings) -> TeslaState {
    let ha = HAClient::new(settings);
    if !ha.available() {
        debug!("tesla: HA unavailable, using fallback");
        return TeslaState::fallback();
    }
    if settings.tesla_battery_entity.is_empty() {
        debug!("tesla: no battery entity configured, using fallback");
        return TeslaState::fallback();
    }
    TeslaState {
        battery_pct: read_float(&ha, &settings.tesla_battery_entity).await,
        range_km: read_float(&ha, &settings.tesla_range_entity).await,
        inside_temp_c: read_float(&ha, &settings.tesla_inside_temp_entity).await,
        climate_on: read_climate_on(&ha, &settings.tesla_climate_entity).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn to_float_handles_number_value() {
        let v = json!({"state": 42.5});
        assert_eq!(to_float(v.get("state")), Some(42.5));
    }

    #[test]
    fn to_float_handles_numeric_string() {
        let v = json!({"state": "2.71"});
        assert_eq!(to_float(v.get("state")), Some(2.71));
    }

    #[test]
    fn to_float_returns_none_for_unknown_sentinels() {
        for s in ["unknown", "unavailable", "none", ""] {
            let v = json!({"state": s});
            assert!(to_float(v.get("state")).is_none(), "{s} should be None");
        }
    }

    #[test]
    fn to_float_returns_none_for_garbage_strings() {
        let v = json!({"state": "hello"});
        assert!(to_float(v.get("state")).is_none());
    }

    #[test]
    fn to_float_returns_none_for_missing_value() {
        let v = json!({});
        assert!(to_float(v.get("state")).is_none());
    }

    #[test]
    fn fallback_has_demo_values() {
        let f = TeslaState::fallback();
        assert_eq!(f.battery_pct, Some(73.0));
        assert_eq!(f.range_km, Some(309.0));
        assert!(f.inside_temp_c.is_none());
        assert!(f.climate_on.is_none());
    }

    #[tokio::test]
    async fn fetch_falls_back_when_no_token() {
        let settings = Settings {
            supervisor_token: None,
            tesla_battery_entity: "sensor.tesla_battery".to_owned(),
            ..Settings::default()
        };
        assert_eq!(fetch(&settings).await, TeslaState::fallback());
    }

    #[tokio::test]
    async fn fetch_falls_back_when_battery_entity_empty() {
        let settings = Settings {
            supervisor_token: Some("dev-token".to_owned()),
            tesla_battery_entity: String::new(),
            ..Settings::default()
        };
        assert_eq!(fetch(&settings).await, TeslaState::fallback());
    }
}
