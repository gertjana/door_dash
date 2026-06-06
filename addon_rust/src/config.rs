//! Add-on configuration.
//!
//! Mirrors the Python `app.config.Settings` class. Three layers, applied
//! lowest priority first:
//!
//! 1. Hard-coded defaults from `Settings::default()`.
//! 2. The HA add-on's `/data/options.json` (Supervisor writes this from
//!    the schema in `config.yaml`). Missing in local dev — that's fine.
//! 3. A small set of env-var overrides for the values that actually
//!    differ between dev and prod (HA base URL, Supervisor token).
//!
//! We deliberately *don't* mirror pydantic's "every field overridable
//! via env var" behaviour — in production HA writes options.json so the
//! env-var path is dev-only, and supporting 30 separate env vars adds
//! a lot of boilerplate for negligible benefit.

use std::env;
use std::path::Path;

use serde::Deserialize;
use tracing::{debug, warn};

const OPTIONS_PATH: &str = "/data/options.json";

/// Default panel width for the reTerminal E1001 (7.5" monochrome ePaper).
const DEFAULT_WIDTH: u32 = 800;
const DEFAULT_HEIGHT: u32 = 480;

/// Live add-on configuration.
///
/// Field order + naming matches `addon/app/config.py` so the two
/// implementations are trivially diffable.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Settings {
    // Display (reTerminal E1001)
    pub width: u32,
    pub height: u32,

    // Wi-Fi QR
    pub wifi_ssid: String,
    pub wifi_password: String,
    /// `WPA` / `WEP` / `nopass`. Empty/`None` is normalised to `nopass`.
    pub wifi_security: String,
    pub wifi_hidden: bool,

    // Weather
    pub weather_entity: String,

    // Calendar
    pub calendar_entities: Vec<String>,
    pub max_events: usize,

    // Tesla — each entity is independently optional.
    pub show_tesla: bool,
    pub tesla_battery_entity: String,
    pub tesla_range_entity: String,
    pub tesla_inside_temp_entity: String,
    pub tesla_climate_entity: String,

    // Local sensors (pushed by device via query params)
    pub show_local_sensors: bool,

    // Energy / DSMR P1 reader.
    pub energy_power_produced_l1_entity: String,
    pub energy_power_produced_l2_entity: String,
    pub energy_power_produced_l3_entity: String,
    pub energy_power_consumed_l1_entity: String,
    pub energy_power_consumed_l2_entity: String,
    pub energy_power_consumed_l3_entity: String,
    pub energy_voltage_l1_entity: String,
    pub energy_voltage_l2_entity: String,
    pub energy_voltage_l3_entity: String,
    pub energy_current_l1_entity: String,
    pub energy_current_l2_entity: String,
    pub energy_current_l3_entity: String,
    pub energy_tariff1_entity: String,
    pub energy_tariff2_entity: String,
    pub energy_gas_entity: String,
    pub indoor_temp_entity: String,
    pub indoor_humidity_entity: String,

    // Timezone / locale
    pub timezone: String,

    // Cache (seconds)
    pub refresh_cache_seconds: u64,

    // Home Assistant access. Supervisor injects `SUPERVISOR_TOKEN` —
    // we read it directly in `Settings::load()` rather than expecting
    // it to come through options.json.
    #[serde(default)]
    pub supervisor_token: Option<String>,
    pub ha_base_url: String,
}

impl Default for Settings {
    /// Production defaults. Match Python `Settings` field defaults so
    /// behaviour is identical when neither options.json nor env vars
    /// override anything.
    fn default() -> Self {
        Self {
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,

            wifi_ssid: "YourWiFi".to_owned(),
            wifi_password: String::new(),
            wifi_security: "nopass".to_owned(),
            wifi_hidden: false,

            weather_entity: "weather.home".to_owned(),

            calendar_entities: vec!["calendar.personal".to_owned()],
            max_events: 8,

            show_tesla: true,
            tesla_battery_entity: String::new(),
            tesla_range_entity: String::new(),
            tesla_inside_temp_entity: String::new(),
            tesla_climate_entity: String::new(),

            show_local_sensors: true,

            energy_power_produced_l1_entity: "sensor.power_produced_phase_1".to_owned(),
            energy_power_produced_l2_entity: "sensor.power_produced_phase_2".to_owned(),
            energy_power_produced_l3_entity: "sensor.power_produced_phase_3".to_owned(),
            energy_power_consumed_l1_entity: "sensor.power_consumed_phase_1".to_owned(),
            energy_power_consumed_l2_entity: "sensor.power_consumed_phase_2".to_owned(),
            energy_power_consumed_l3_entity: "sensor.power_consumed_phase_3".to_owned(),
            energy_voltage_l1_entity: "sensor.voltage_phase_1".to_owned(),
            energy_voltage_l2_entity: "sensor.voltage_phase_2".to_owned(),
            energy_voltage_l3_entity: "sensor.voltage_phase_3".to_owned(),
            energy_current_l1_entity: "sensor.current_phase_1".to_owned(),
            energy_current_l2_entity: "sensor.current_phase_2".to_owned(),
            energy_current_l3_entity: "sensor.current_phase_3".to_owned(),
            energy_tariff1_entity: "sensor.energy_consumed_tariff_1".to_owned(),
            energy_tariff2_entity: "sensor.energy_consumed_tariff_2".to_owned(),
            energy_gas_entity: "sensor.gas_consumed".to_owned(),
            indoor_temp_entity: "sensor.door_dashboard_indoor_temperature".to_owned(),
            indoor_humidity_entity: "sensor.door_dashboard_indoor_humidity".to_owned(),

            timezone: "Europe/Amsterdam".to_owned(),

            refresh_cache_seconds: 60,

            supervisor_token: None,
            ha_base_url: "http://supervisor/core".to_owned(),
        }
    }
}

impl Settings {
    /// Load + layer settings from defaults, options.json, and env vars.
    ///
    /// Never panics: a malformed `options.json` is logged and ignored
    /// rather than killing the add-on at boot. A missing options file
    /// is normal in local dev.
    pub fn load() -> Self {
        let mut s = Self::default();

        if Path::new(OPTIONS_PATH).exists() {
            match std::fs::read_to_string(OPTIONS_PATH) {
                Ok(raw) => match serde_json::from_str::<Settings>(&raw) {
                    Ok(loaded) => {
                        s = loaded;
                        debug!(path = OPTIONS_PATH, "loaded options.json");
                    }
                    Err(e) => {
                        warn!(error = %e, path = OPTIONS_PATH, "failed to parse options.json; using defaults");
                    }
                },
                Err(e) => {
                    warn!(error = %e, path = OPTIONS_PATH, "failed to read options.json; using defaults");
                }
            }
        } else {
            debug!(path = OPTIONS_PATH, "options.json not present (dev mode)");
        }

        // Env-var overrides — only the dev-relevant ones. Production
        // doesn't need them; HA writes options.json. Supervisor token
        // comes from the unprefixed `SUPERVISOR_TOKEN` env var (HA
        // injects it that way, not as `EPDASH_SUPERVISOR_TOKEN`).
        if let Ok(token) = env::var("SUPERVISOR_TOKEN") {
            if !token.is_empty() {
                s.supervisor_token = Some(token);
            }
        }
        if let Ok(token) = env::var("EPDASH_SUPERVISOR_TOKEN") {
            if !token.is_empty() {
                s.supervisor_token = Some(token);
            }
        }
        if let Ok(url) = env::var("EPDASH_HA_BASE_URL") {
            if !url.is_empty() {
                s.ha_base_url = url;
            }
        }

        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_python_reference() {
        // Spot-check a representative cross-section of the schema rather
        // than every field — `Default` impl is the canonical reference,
        // checking each field would just be a tautology. Pick the ones
        // that are genuinely user-facing or have unit suffixes.
        let s = Settings::default();
        assert_eq!(s.width, 800);
        assert_eq!(s.height, 480);
        assert_eq!(s.weather_entity, "weather.home");
        assert_eq!(s.calendar_entities, vec!["calendar.personal".to_owned()]);
        assert_eq!(s.max_events, 8);
        assert_eq!(s.timezone, "Europe/Amsterdam");
        assert_eq!(s.refresh_cache_seconds, 60);
        assert_eq!(s.ha_base_url, "http://supervisor/core");
        assert!(s.show_local_sensors);
        assert!(s.show_tesla);
        assert_eq!(s.supervisor_token, None);
    }

    #[test]
    fn deserialize_options_json_partial_overrides_only_named_fields() {
        // Verifies `#[serde(default)]` at struct level keeps unspecified
        // fields at their defaults — critical because production
        // options.json frequently omits optional keys (e.g. tesla_*).
        let raw = r#"{
            "weather_entity": "weather.kitchen",
            "max_events": 5,
            "calendar_entities": ["calendar.work"]
        }"#;
        let s: Settings = serde_json::from_str(raw).expect("parse");
        assert_eq!(s.weather_entity, "weather.kitchen");
        assert_eq!(s.max_events, 5);
        assert_eq!(s.calendar_entities, vec!["calendar.work".to_owned()]);
        // Untouched defaults preserved.
        assert_eq!(s.timezone, "Europe/Amsterdam");
        assert_eq!(s.width, 800);
        assert_eq!(
            s.energy_power_consumed_l1_entity,
            "sensor.power_consumed_phase_1"
        );
    }
}
