//! Local sensors — values *pushed* into the renderer by the device on
//! each wake (as query parameters), rather than fetched from HA.
//!
//! This module just normalises and clamps the incoming values; there
//! is no async I/O. Mirrors `addon/app/sources/local_sensors.py`.

#[derive(Debug, Clone, Default, PartialEq)]
pub struct LocalSensors {
    /// Indoor temperature in °C.
    pub indoor_temp: Option<f64>,
    /// Indoor humidity in %, clamped to 0–100.
    pub indoor_hum: Option<f64>,
    /// Battery percentage in %, clamped to 0–100.
    pub battery_pct: Option<f64>,
}

/// Drop NaN/Inf so downstream code can rely on `Option<f64>` meaning
/// "either a real number or no reading".
fn sanitize(value: Option<f64>) -> Option<f64> {
    value.filter(|v| v.is_finite())
}

impl LocalSensors {
    /// Build a `LocalSensors` from raw query-string values. Hum and
    /// battery are clamped to `0..=100`; temperature is left as-is
    /// because indoor temps can plausibly span a wide range.
    pub fn from_query(
        indoor_temp: Option<f64>,
        indoor_hum: Option<f64>,
        battery_pct: Option<f64>,
    ) -> Self {
        Self {
            indoor_temp: sanitize(indoor_temp),
            indoor_hum: sanitize(indoor_hum).map(|v| v.clamp(0.0, 100.0)),
            battery_pct: sanitize(battery_pct).map(|v| v.clamp(0.0, 100.0)),
        }
    }

    /// Quantise values to whole units for cache lookup.
    ///
    /// Sub-degree temperature drift wouldn't change the rendered output
    /// meaningfully, so rounding here cuts cache misses on ePaper wakes
    /// where the device reports e.g. 21.4 °C / 21.6 °C alternately.
    pub fn cache_key(&self) -> (Option<i64>, Option<i64>, Option<i64>) {
        (
            self.indoor_temp.map(|v| v.round() as i64),
            self.indoor_hum.map(|v| v.round() as i64),
            self.battery_pct.map(|v| v.round() as i64),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_query_keeps_valid_values() {
        let s = LocalSensors::from_query(Some(21.4), Some(48.0), Some(73.0));
        assert_eq!(s.indoor_temp, Some(21.4));
        assert_eq!(s.indoor_hum, Some(48.0));
        assert_eq!(s.battery_pct, Some(73.0));
    }

    #[test]
    fn from_query_clamps_out_of_range_humidity_and_battery() {
        let s = LocalSensors::from_query(Some(21.0), Some(150.0), Some(-5.0));
        assert_eq!(s.indoor_hum, Some(100.0));
        assert_eq!(s.battery_pct, Some(0.0));
    }

    #[test]
    fn from_query_does_not_clamp_temperature() {
        // Cold room or test rig — temps outside 0-100 °C are valid.
        let s = LocalSensors::from_query(Some(-12.5), None, None);
        assert_eq!(s.indoor_temp, Some(-12.5));
    }

    #[test]
    fn from_query_rejects_nan_and_infinity() {
        let s =
            LocalSensors::from_query(Some(f64::NAN), Some(f64::INFINITY), Some(f64::NEG_INFINITY));
        assert_eq!(s, LocalSensors::default());
    }

    #[test]
    fn cache_key_rounds_to_integer_units() {
        let s = LocalSensors::from_query(Some(21.49), Some(48.51), Some(73.0));
        let (t, h, b) = s.cache_key();
        assert_eq!(t, Some(21));
        assert_eq!(h, Some(49));
        assert_eq!(b, Some(73));
    }

    #[test]
    fn cache_key_preserves_none_for_missing_values() {
        let s = LocalSensors::default();
        assert_eq!(s.cache_key(), (None, None, None));
    }
}
