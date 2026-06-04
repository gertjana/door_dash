//! Energy / DSMR P1 source.
//!
//! Reads instantaneous values per phase (L1/L2/L3) for power produced,
//! power consumed, voltage and current; the cumulative tariff and gas
//! counters; and indoor temp/humidity. Also fetches a 24-hour history
//! strip for the values that get sparklines on the energy page (L1
//! power produced, L1 power consumed, L1 voltage, L1 current, indoor
//! temp, indoor humidity).
//!
//! Mirrors `addon/app/sources/energy.py`. Returns a synthesised
//! fallback when HA is unreachable so dev renders always have
//! plausible-looking data.
//!
//! Power values are in **kW** (DSMR/SlimmeLezer native unit), voltage
//! in V, current in A. Sparkline buckets preserve those units rather
//! than converting — the page module formats them at draw time.

use std::f64::consts::PI;

use chrono::{DateTime, Duration, Utc};
use serde_json::Value;
use tracing::{info, warn};

use crate::config::Settings;
use crate::ha_client::HAClient;

/// 24-hour window resampled onto a uniform grid. 96 buckets = 15 min
/// each, which gives ~3-4 px per bucket on a ~380 px wide sparkline
/// cell — coarse enough to be readable on a 1-bit panel without
/// losing the daily shape.
pub const HISTORY_BUCKETS: usize = 96;
pub const HISTORY_HOURS: i64 = 24;

const UNKNOWN_STATES: &[&str] = &["unknown", "unavailable", "none", ""];

/// Per-phase value triplet. Any field may be `None` if HA returns no
/// state for that phase (older meters often only report L1).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PhaseValues {
    pub l1: Option<f64>,
    pub l2: Option<f64>,
    pub l3: Option<f64>,
}

impl PhaseValues {
    pub fn values(&self) -> (Option<f64>, Option<f64>, Option<f64>) {
        (self.l1, self.l2, self.l3)
    }
}

/// Snapshot of P1 + indoor environment values for the energy page.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EnergyState {
    pub power_produced: PhaseValues,
    pub power_consumed: PhaseValues,
    pub voltage: PhaseValues,
    pub current: PhaseValues,

    /// kWh consumed (low tariff). Cumulative since meter install.
    pub energy_tariff1: Option<f64>,
    /// kWh consumed (high tariff). Cumulative since meter install.
    pub energy_tariff2: Option<f64>,
    /// m³ gas consumed. Cumulative since meter install.
    pub gas: Option<f64>,

    pub indoor_temp: Option<f64>,
    pub indoor_humidity: Option<f64>,

    /// 24h history grids (length `HISTORY_BUCKETS`, oldest → newest).
    /// Each entry is a value in the same unit as the live field, or
    /// `None` for buckets where no value was available yet.
    pub history_power_produced_l1: Vec<Option<f64>>,
    pub history_power_consumed_l1: Vec<Option<f64>>,
    pub history_voltage_l1: Vec<Option<f64>>,
    pub history_current_l1: Vec<Option<f64>>,
    pub history_indoor_temp: Vec<Option<f64>>,
    pub history_indoor_humidity: Vec<Option<f64>>,
}

/// Coerce HA state strings to f64, returning None for unknown /
/// unavailable / non-numeric values.
fn safe_float(value: Option<&Value>) -> Option<f64> {
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

async fn read_state(ha: &HAClient, entity_id: &str) -> Option<f64> {
    if entity_id.is_empty() {
        return None;
    }
    let state = ha.get_state(entity_id).await?;
    safe_float(state.get("state"))
}

/// Resample HA history (state-change list) onto an N-point uniform
/// time grid.
///
/// HA's history endpoint returns events when the state *changed*,
/// not on a uniform schedule. To plot a sparkline we need N
/// evenly-spaced samples, so for each grid timestamp we take the
/// most recent state at-or-before that moment ("step" interpolation,
/// the standard for state recordings).
///
/// Buckets before the first recorded state-change are returned as
/// `None` so the sparkline shows a gap rather than a flat-line lie.
pub fn bucket_history(
    raw: &[Value],
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    n: usize,
) -> Vec<Option<f64>> {
    if raw.is_empty() {
        return vec![None; n];
    }
    // Parse + sort points by timestamp. Skip events with no usable
    // timestamp (HA always provides one but be defensive).
    let mut parsed: Vec<(DateTime<Utc>, Option<f64>)> = raw
        .iter()
        .filter_map(|item| {
            let ts = item
                .get("last_changed")
                .or_else(|| item.get("last_updated"))?
                .as_str()?;
            let t = DateTime::parse_from_rfc3339(ts).ok()?.with_timezone(&Utc);
            let v = safe_float(item.get("state"));
            Some((t, v))
        })
        .collect();
    if parsed.is_empty() {
        return vec![None; n];
    }
    parsed.sort_by_key(|p| p.0);

    let span = (end - start).num_seconds();
    if span <= 0 {
        return vec![None; n];
    }

    let mut out = Vec::with_capacity(n);
    let mut j: usize = 0;
    let mut last_v: Option<f64> = None;
    let denom = (n as i64 - 1).max(1);
    for i in 0..n {
        let t_grid = start + Duration::seconds(span * i as i64 / denom);
        while j < parsed.len() && parsed[j].0 <= t_grid {
            last_v = parsed[j].1;
            j += 1;
        }
        out.push(last_v);
    }
    out
}

/// Synthesise believable demo data for offline dev rendering.
///
/// Builds a sunny-midday scenario: ~2.5 kW solar production peaking
/// on L2, modest household draw spread across phases, voltage
/// hovering around 230 V, indoor at a comfortable 21 °C / 48% rh.
fn fallback() -> EnergyState {
    let n = HISTORY_BUCKETS;

    // Solar curve: 0 outside roughly 06:00-18:00 (buckets 24-72 of 96),
    // half-sine peak ~2.5 kW around noon (bucket 48).
    let history_power_produced_l1: Vec<Option<f64>> = (0..n)
        .map(|i| {
            if (24..=72).contains(&i) {
                let t = (i - 24) as f64 / 48.0;
                Some(round3(2.5 * (PI * t).sin()))
            } else {
                Some(0.0)
            }
        })
        .collect();

    // Consumption: morning + evening peaks, baseload 0.2 kW.
    let history_power_consumed_l1: Vec<Option<f64>> = (0..n)
        .map(|i| {
            let i_f = i as f64;
            let morning = 0.4 * f64::exp(-((i_f - 30.0).powi(2)) / 50.0);
            let evening = 0.6 * f64::exp(-((i_f - 78.0).powi(2)) / 60.0);
            Some(round3(0.2 + morning + evening))
        })
        .collect();

    let history_voltage_l1: Vec<Option<f64>> = (0..n)
        .map(|i| Some(round2(230.0 + 0.5 * (i as f64 / 7.0).sin())))
        .collect();
    let history_current_l1: Vec<Option<f64>> = (0..n)
        .map(|i| Some(round2(0.7 + 0.6 * (i as f64 / 9.0).cos())))
        .collect();
    let history_indoor_temp: Vec<Option<f64>> = (0..n)
        .map(|i| Some(round2(20.5 + 1.2 * ((i as f64 - 36.0) / 18.0).sin())))
        .collect();
    let history_indoor_humidity: Vec<Option<f64>> = (0..n)
        .map(|i| Some(round1(50.0 - 5.0 * ((i as f64 - 36.0) / 18.0).sin())))
        .collect();

    EnergyState {
        power_produced: PhaseValues {
            l1: Some(0.0),
            l2: Some(2.5),
            l3: Some(0.0),
        },
        power_consumed: PhaseValues {
            l1: Some(0.180),
            l2: Some(0.090),
            l3: Some(0.130),
        },
        voltage: PhaseValues {
            l1: Some(230.1),
            l2: Some(231.5),
            l3: Some(229.4),
        },
        current: PhaseValues {
            l1: Some(0.78),
            l2: Some(11.2),
            l3: Some(0.57),
        },
        energy_tariff1: Some(12345.6),
        energy_tariff2: Some(7890.1),
        gas: Some(2345.678),
        indoor_temp: Some(21.3),
        indoor_humidity: Some(48.0),
        history_power_produced_l1,
        history_power_consumed_l1,
        history_voltage_l1,
        history_current_l1,
        history_indoor_temp,
        history_indoor_humidity,
    }
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}
fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}
fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

/// Pull current values + 24h history for the energy page.
///
/// Strategy:
///
/// 1. One HA state call per configured entity for the live numbers.
/// 2. A single `/api/history/period` call for all six sparkline
///    entities at once (HA accepts a comma-separated list).
///
/// Falls back to synthetic data if HA is unreachable or no live
/// values came through (typical "entity IDs are wrong" symptom —
/// better to show *something* than a page full of em-dashes).
pub async fn fetch(settings: &Settings) -> EnergyState {
    let ha = HAClient::new(settings);
    if !ha.available() {
        info!("energy: HA unavailable, using fallback");
        return fallback();
    }

    let mut out = EnergyState {
        power_produced: PhaseValues {
            l1: read_state(&ha, &settings.energy_power_produced_l1_entity).await,
            l2: read_state(&ha, &settings.energy_power_produced_l2_entity).await,
            l3: read_state(&ha, &settings.energy_power_produced_l3_entity).await,
        },
        power_consumed: PhaseValues {
            l1: read_state(&ha, &settings.energy_power_consumed_l1_entity).await,
            l2: read_state(&ha, &settings.energy_power_consumed_l2_entity).await,
            l3: read_state(&ha, &settings.energy_power_consumed_l3_entity).await,
        },
        voltage: PhaseValues {
            l1: read_state(&ha, &settings.energy_voltage_l1_entity).await,
            l2: read_state(&ha, &settings.energy_voltage_l2_entity).await,
            l3: read_state(&ha, &settings.energy_voltage_l3_entity).await,
        },
        current: PhaseValues {
            l1: read_state(&ha, &settings.energy_current_l1_entity).await,
            l2: read_state(&ha, &settings.energy_current_l2_entity).await,
            l3: read_state(&ha, &settings.energy_current_l3_entity).await,
        },
        energy_tariff1: read_state(&ha, &settings.energy_tariff1_entity).await,
        energy_tariff2: read_state(&ha, &settings.energy_tariff2_entity).await,
        gas: read_state(&ha, &settings.energy_gas_entity).await,
        indoor_temp: read_state(&ha, &settings.indoor_temp_entity).await,
        indoor_humidity: read_state(&ha, &settings.indoor_humidity_entity).await,
        ..Default::default()
    };

    // Fetch history for the six sparkline traces in a single HTTP
    // call. We tolerate any of these being unconfigured (empty
    // entity ID) by excluding them and leaving the corresponding
    // history Vec empty — the sparkline drawer treats that as
    // "no data" and renders a stub.
    let end = Utc::now();
    let start = end - Duration::hours(HISTORY_HOURS);
    let history_targets: [(&str, &str); 6] = [
        (
            "history_power_produced_l1",
            &settings.energy_power_produced_l1_entity,
        ),
        (
            "history_power_consumed_l1",
            &settings.energy_power_consumed_l1_entity,
        ),
        ("history_voltage_l1", &settings.energy_voltage_l1_entity),
        ("history_current_l1", &settings.energy_current_l1_entity),
        ("history_indoor_temp", &settings.indoor_temp_entity),
        ("history_indoor_humidity", &settings.indoor_humidity_entity),
    ];
    let configured: Vec<(&str, &str)> = history_targets
        .iter()
        .copied()
        .filter(|(_, ent)| !ent.is_empty())
        .collect();

    if !configured.is_empty() {
        let ent_ids: Vec<&str> = configured.iter().map(|(_, ent)| *ent).collect();
        let start_iso = start.to_rfc3339();
        let end_iso = end.to_rfc3339();
        let history_resp = ha.get_history(&ent_ids, &start_iso, &end_iso).await;
        for (i, (attr, _ent)) in configured.iter().enumerate() {
            let raw = history_resp.get(i).cloned().unwrap_or_default();
            let buckets = bucket_history(&raw, start, end, HISTORY_BUCKETS);
            assign_history(&mut out, attr, buckets);
        }
    }

    // If literally nothing came through on the live values, the
    // entity IDs are probably wrong — render fallback so the page
    // still demos cleanly rather than displaying a sea of em-dashes.
    let has_any_live = out.power_produced.l1.is_some()
        || out.power_consumed.l1.is_some()
        || out.voltage.l1.is_some()
        || out.current.l1.is_some()
        || out.energy_tariff1.is_some()
        || out.indoor_temp.is_some();
    if !has_any_live {
        warn!(
            "energy: no live values returned for any configured entity — \
             using fallback. Check entity IDs in addon options."
        );
        return fallback();
    }

    info!(
        p_prod_l1 = ?out.power_produced.l1,
        p_cons_l1 = ?out.power_consumed.l1,
        v_l1 = ?out.voltage.l1,
        i_l1 = ?out.current.l1,
        t1 = ?out.energy_tariff1,
        t2 = ?out.energy_tariff2,
        gas = ?out.gas,
        indoor_t = ?out.indoor_temp,
        indoor_h = ?out.indoor_humidity,
        "energy: fetched"
    );
    out
}

/// Route a parsed history series onto the right `EnergyState` field.
/// Keeping this as a small dispatch helper means the field assignment
/// is one place rather than six near-duplicates inline.
fn assign_history(state: &mut EnergyState, attr: &str, buckets: Vec<Option<f64>>) {
    match attr {
        "history_power_produced_l1" => state.history_power_produced_l1 = buckets,
        "history_power_consumed_l1" => state.history_power_consumed_l1 = buckets,
        "history_voltage_l1" => state.history_voltage_l1 = buckets,
        "history_current_l1" => state.history_current_l1 = buckets,
        "history_indoor_temp" => state.history_indoor_temp = buckets,
        "history_indoor_humidity" => state.history_indoor_humidity = buckets,
        _ => {} // unreachable for known callers; ignore silently
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn safe_float_handles_number_and_numeric_string() {
        let v = json!({"state": 12.5});
        assert_eq!(safe_float(v.get("state")), Some(12.5));
        let v2 = json!({"state": "0.345"});
        assert_eq!(safe_float(v2.get("state")), Some(0.345));
    }

    #[test]
    fn safe_float_rejects_unknown_sentinels() {
        for s in [
            "unknown",
            "unavailable",
            "none",
            "",
            "Unknown",
            "  unavailable  ",
        ] {
            let v = json!({"state": s});
            assert!(safe_float(v.get("state")).is_none(), "should reject: {s}");
        }
    }

    #[test]
    fn bucket_history_empty_input_yields_all_none() {
        let start = Utc::now();
        let end = start + Duration::hours(1);
        let buckets = bucket_history(&[], start, end, 4);
        assert_eq!(buckets, vec![None, None, None, None]);
    }

    #[test]
    fn bucket_history_step_interpolates_state_changes() {
        // Three state changes within a 1h window: 0.1 at start, 0.5
        // mid-way, 0.9 near end. Bucketed into 5 points (every 15 min).
        let start = DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let end = start + Duration::hours(1);
        let raw = vec![
            json!({"last_changed": "2026-01-01T00:00:00Z", "state": 0.1}),
            json!({"last_changed": "2026-01-01T00:30:00Z", "state": 0.5}),
            json!({"last_changed": "2026-01-01T00:55:00Z", "state": 0.9}),
        ];
        let buckets = bucket_history(&raw, start, end, 5);
        // Grid points: t=0, 15, 30, 45, 60 min.
        assert_eq!(buckets[0], Some(0.1));
        assert_eq!(buckets[1], Some(0.1));
        assert_eq!(buckets[2], Some(0.5));
        assert_eq!(buckets[3], Some(0.5));
        assert_eq!(buckets[4], Some(0.9));
    }

    #[test]
    fn bucket_history_pre_data_buckets_are_none() {
        // First state change at minute 30; buckets at minutes 0/15
        // should be None (no data yet), not 0.
        let start = DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let end = start + Duration::hours(1);
        let raw = vec![json!({"last_changed": "2026-01-01T00:30:00Z", "state": 1.0})];
        let buckets = bucket_history(&raw, start, end, 5);
        assert_eq!(buckets[0], None);
        assert_eq!(buckets[1], None);
        assert_eq!(buckets[2], Some(1.0));
        assert_eq!(buckets[3], Some(1.0));
        assert_eq!(buckets[4], Some(1.0));
    }

    #[test]
    fn bucket_history_handles_unsorted_input() {
        let start = DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let end = start + Duration::hours(1);
        let raw = vec![
            json!({"last_changed": "2026-01-01T00:55:00Z", "state": 0.9}),
            json!({"last_changed": "2026-01-01T00:00:00Z", "state": 0.1}),
            json!({"last_changed": "2026-01-01T00:30:00Z", "state": 0.5}),
        ];
        let buckets = bucket_history(&raw, start, end, 5);
        assert_eq!(buckets[0], Some(0.1));
        assert_eq!(buckets[4], Some(0.9));
    }

    #[test]
    fn bucket_history_skips_garbage_timestamps() {
        let start = DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let end = start + Duration::hours(1);
        let raw = vec![
            json!({"last_changed": "garbage", "state": 0.1}),
            json!({"last_changed": "2026-01-01T00:30:00Z", "state": 0.5}),
        ];
        let buckets = bucket_history(&raw, start, end, 3);
        // Bucket 0 (00:00) has no data; bucket 1 (00:30) sees the 0.5
        // change; bucket 2 (01:00) carries it forward.
        assert_eq!(buckets[0], None);
        assert_eq!(buckets[1], Some(0.5));
        assert_eq!(buckets[2], Some(0.5));
    }

    #[test]
    fn bucket_history_zero_or_negative_span_yields_all_none() {
        let t = Utc::now();
        let buckets = bucket_history(
            &[json!({"last_changed": "2026-01-01T00:00:00Z", "state": 1.0})],
            t,
            t,
            4,
        );
        assert_eq!(buckets, vec![None, None, None, None]);
    }

    #[test]
    fn fallback_yields_full_length_history_grids() {
        let s = fallback();
        assert_eq!(s.history_power_produced_l1.len(), HISTORY_BUCKETS);
        assert_eq!(s.history_power_consumed_l1.len(), HISTORY_BUCKETS);
        assert_eq!(s.history_voltage_l1.len(), HISTORY_BUCKETS);
        assert_eq!(s.history_current_l1.len(), HISTORY_BUCKETS);
        assert_eq!(s.history_indoor_temp.len(), HISTORY_BUCKETS);
        assert_eq!(s.history_indoor_humidity.len(), HISTORY_BUCKETS);
    }

    #[test]
    fn fallback_solar_peaks_around_noon() {
        let s = fallback();
        // Bucket 48 is the half-sine peak; should be very close to 2.5.
        let peak = s.history_power_produced_l1[48].unwrap();
        assert!(
            (peak - 2.5).abs() < 0.01,
            "noon peak {peak} should be ~2.5 kW"
        );
        // Buckets at 00:00 and 23:45 should be 0 (no sun).
        assert_eq!(s.history_power_produced_l1[0], Some(0.0));
        assert_eq!(s.history_power_produced_l1[95], Some(0.0));
    }

    #[tokio::test]
    async fn fetch_returns_fallback_when_ha_unavailable() {
        let settings = Settings {
            supervisor_token: None,
            ..Settings::default()
        };
        let s = fetch(&settings).await;
        // Fallback has known live values.
        assert_eq!(s.power_produced.l2, Some(2.5));
        assert_eq!(s.energy_tariff1, Some(12345.6));
    }
}
