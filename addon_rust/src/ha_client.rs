//! Tiny Home Assistant REST client.
//!
//! Inside an HA add-on we get a `SUPERVISOR_TOKEN` env var and reach the
//! Core API at `http://supervisor/core/api/...`. Outside HA (local dev)
//! the user can set `EPDASH_HA_BASE_URL` + `EPDASH_SUPERVISOR_TOKEN` to
//! point at a real instance.
//!
//! Mirrors `addon/app/ha_client.py` method-for-method. All four endpoints
//! return either typed JSON or an empty fallback (`None` / `Vec::new`)
//! on any error so callers can render gracefully without try/except
//! gymnastics — the Python addon's design too.
//!
//! ## Async vs sync
//!
//! Unlike the Python addon (which uses sync `httpx`), the Rust port
//! uses async `reqwest`. Sources are therefore async too. The actual
//! image-drawing step is CPU-bound and runs inside `spawn_blocking`;
//! that's the only sync hand-off in the pipeline.

use std::time::Duration;

use anyhow::Result;
use serde_json::Value;
use tracing::{debug, warn};

use crate::config::Settings;

/// Default per-request timeouts. Mirror the Python defaults so flaky-HA
/// behaviour is observably identical between the two addons.
const TIMEOUT_STATE: Duration = Duration::from_secs(8);
const TIMEOUT_HISTORY: Duration = Duration::from_secs(15);
const TIMEOUT_CALENDAR: Duration = Duration::from_secs(8);
const TIMEOUT_SERVICE: Duration = Duration::from_secs(10);

/// Async Home Assistant REST client.
///
/// Cheap to clone — wraps a single `reqwest::Client` plus the configured
/// base URL and bearer token, both of which are immutable for the
/// lifetime of the addon process.
#[derive(Debug, Clone)]
pub struct HAClient {
    base: String,
    token: Option<String>,
    http: reqwest::Client,
}

impl HAClient {
    /// Build a client against the configured HA endpoint.
    ///
    /// The reqwest client is configured with sensible defaults for the
    /// dashboard's traffic profile: small bodies, infrequent requests,
    /// per-request timeouts. We don't enable connection pooling tuning
    /// because HA's Supervisor proxy keeps connections short-lived.
    pub fn new(settings: &Settings) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(concat!("epaper-dashboard-rust/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("reqwest client builds with default settings");
        Self {
            base: settings.ha_base_url.trim_end_matches('/').to_owned(),
            token: settings.supervisor_token.clone(),
            http,
        }
    }

    /// Whether the client has a Supervisor token (and is therefore
    /// expected to actually reach HA). Mirrors Python's `available`.
    pub fn available(&self) -> bool {
        self.token.as_deref().is_some_and(|t| !t.is_empty())
    }

    /// Build the `Authorization` header for a request. None when no
    /// token is configured — callers should usually short-circuit on
    /// `available()` before reaching this.
    fn bearer(&self) -> Option<String> {
        self.token.as_ref().map(|t| format!("Bearer {t}"))
    }

    /// `GET /api/states/<entity_id>`. Returns the raw state JSON object
    /// or `None` if the entity is missing / HA is unreachable.
    pub async fn get_state(&self, entity_id: &str) -> Option<Value> {
        if !self.available() {
            return None;
        }
        let url = format!("{}/api/states/{}", self.base, entity_id);
        match self
            .http
            .get(&url)
            .header("Authorization", self.bearer()?)
            .header("Content-Type", "application/json")
            .timeout(TIMEOUT_STATE)
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => match resp.json::<Value>().await {
                Ok(v) => Some(v),
                Err(e) => {
                    debug!(entity = entity_id, error = %e, "get_state: body decode failed");
                    None
                }
            },
            Ok(resp) => {
                debug!(entity = entity_id, status = %resp.status(), "get_state: non-success");
                None
            }
            Err(e) => {
                debug!(entity = entity_id, error = %e, "get_state: request failed");
                None
            }
        }
    }

    /// `GET /api/history/period/<start>?filter_entity_id=A,B,...`.
    ///
    /// Returns one inner list per requested entity, in the same order
    /// as `entity_ids`. HA only returns inner series for entities that
    /// have any recorded history, so we re-key by `entity_id` to keep
    /// the output positional with the input even when an entity is
    /// missing. Empty inner list = "no history available".
    ///
    /// `minimal_response` and `no_attributes` are passed as flag-style
    /// empty values; HA's flag parser treats any presence as truthy
    /// regardless of the value. Cuts the payload size by an order of
    /// magnitude on dense entities (e.g. SlimmeLezer power readings
    /// updating every second).
    pub async fn get_history(
        &self,
        entity_ids: &[&str],
        start_iso: &str,
        end_iso: &str,
    ) -> Vec<Vec<Value>> {
        let empty = || vec![Vec::<Value>::new(); entity_ids.len()];
        if !self.available() || entity_ids.is_empty() {
            return empty();
        }
        let bearer = match self.bearer() {
            Some(b) => b,
            None => return empty(),
        };
        let url = format!("{}/api/history/period/{}", self.base, start_iso);
        let filter = entity_ids.join(",");
        let req = self
            .http
            .get(&url)
            .header("Authorization", bearer)
            .header("Content-Type", "application/json")
            .timeout(TIMEOUT_HISTORY)
            // Empty values mean "flag is set" for HA's parsing.
            .query(&[
                ("filter_entity_id", filter.as_str()),
                ("end_time", end_iso),
                ("minimal_response", ""),
                ("no_attributes", ""),
            ]);
        let resp = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                warn!(error = %e, n = entity_ids.len(), "get_history: request failed");
                return empty();
            }
        };
        if !resp.status().is_success() {
            warn!(status = %resp.status(), n = entity_ids.len(), "get_history: non-success");
            return empty();
        }
        let data: Vec<Vec<Value>> = match resp.json().await {
            Ok(d) => d,
            Err(e) => {
                warn!(error = %e, "get_history: body decode failed");
                return empty();
            }
        };
        // Re-key by entity_id so the response stays positional with the
        // input even when HA omits an entity (no recorded history yet,
        // or the entity is misspelled / disabled). HA's filter_entity_id
        // is positional in *most* cases but silently drops gaps, which
        // would otherwise misalign callers using zip(entity_ids, result).
        let mut by_id: std::collections::HashMap<String, Vec<Value>> =
            std::collections::HashMap::with_capacity(data.len());
        for series in data {
            let Some(first) = series.first() else {
                continue;
            };
            let Some(eid) = first.get("entity_id").and_then(Value::as_str) else {
                continue;
            };
            by_id.insert(eid.to_owned(), series);
        }
        entity_ids
            .iter()
            .map(|eid| by_id.remove(*eid).unwrap_or_default())
            .collect()
    }

    /// `GET /api/calendars/<entity_id>?start=...&end=...`.
    ///
    /// Returns the list of calendar event objects, or empty on error.
    pub async fn get_calendar(
        &self,
        entity_id: &str,
        start_iso: &str,
        end_iso: &str,
    ) -> Vec<Value> {
        if !self.available() {
            return Vec::new();
        }
        let url = format!("{}/api/calendars/{}", self.base, entity_id);
        let bearer = match self.bearer() {
            Some(b) => b,
            None => return Vec::new(),
        };
        let resp = self
            .http
            .get(&url)
            .header("Authorization", bearer)
            .header("Content-Type", "application/json")
            .query(&[("start", start_iso), ("end", end_iso)])
            .timeout(TIMEOUT_CALENDAR)
            .send()
            .await;
        match resp {
            Ok(r) if r.status().is_success() => r.json::<Vec<Value>>().await.unwrap_or_default(),
            Ok(r) => {
                debug!(entity = entity_id, status = %r.status(), "get_calendar: non-success");
                Vec::new()
            }
            Err(e) => {
                debug!(entity = entity_id, error = %e, "get_calendar: request failed");
                Vec::new()
            }
        }
    }

    /// `POST /api/services/<domain>/<service>`. With `return_response`,
    /// asks HA to include the service response payload in the body
    /// (HA 2024.x+ supports this for `weather.get_forecasts`,
    /// `calendar.get_events`, etc.).
    pub async fn call_service(
        &self,
        domain: &str,
        service: &str,
        data: Option<Value>,
        return_response: bool,
    ) -> Option<Value> {
        if !self.available() {
            return None;
        }
        let mut url = format!("{}/api/services/{}/{}", self.base, domain, service);
        if return_response {
            url.push_str("?return_response");
        }
        let bearer = self.bearer()?;
        let body = data.unwrap_or_else(|| Value::Object(Default::default()));
        match self
            .http
            .post(&url)
            .header("Authorization", bearer)
            .header("Content-Type", "application/json")
            .timeout(TIMEOUT_SERVICE)
            .json(&body)
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => {
                if return_response {
                    r.json().await.ok()
                } else {
                    Some(Value::Object(Default::default()))
                }
            }
            Ok(r) => {
                debug!(domain, service, status = %r.status(), "call_service: non-success");
                None
            }
            Err(e) => {
                debug!(domain, service, error = %e, "call_service: request failed");
                None
            }
        }
    }
}

/// Build a fresh client. Convenience wrapper for one-shot uses.
pub fn client(settings: &Settings) -> HAClient {
    HAClient::new(settings)
}

#[allow(dead_code)]
fn _types_are_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<HAClient>();
    assert_send_sync::<Result<HAClient>>();
}

#[cfg(test)]
impl HAClient {
    /// Test-only accessor for the normalized base URL.
    pub(crate) fn base_url(&self) -> &str {
        &self.base
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings_with(base: &str, token: Option<&str>) -> Settings {
        Settings {
            ha_base_url: base.to_owned(),
            supervisor_token: token.map(str::to_owned),
            ..Settings::default()
        }
    }

    #[test]
    fn available_is_false_without_token() {
        let c = HAClient::new(&settings_with("http://supervisor/core", None));
        assert!(!c.available(), "no token => not available");
    }

    #[test]
    fn available_is_false_with_empty_token() {
        let c = HAClient::new(&settings_with("http://supervisor/core", Some("")));
        assert!(!c.available(), "empty token string is treated as missing");
    }

    #[test]
    fn available_is_true_with_token() {
        let c = HAClient::new(&settings_with("http://supervisor/core", Some("abc")));
        assert!(c.available());
    }

    #[test]
    fn base_url_trailing_slash_is_stripped() {
        let c = HAClient::new(&settings_with("http://supervisor/core/", Some("t")));
        assert_eq!(c.base_url(), "http://supervisor/core");
    }

    #[test]
    fn base_url_multiple_trailing_slashes_are_stripped() {
        // `trim_end_matches('/')` removes all of them, matching Python's
        // `rstrip("/")`. Worth pinning down since it's a behavioural
        // contract callers can rely on when concatenating paths.
        let c = HAClient::new(&settings_with("http://supervisor/core///", Some("t")));
        assert_eq!(c.base_url(), "http://supervisor/core");
    }

    #[test]
    fn base_url_without_slash_is_unchanged() {
        let c = HAClient::new(&settings_with("http://example.invalid:8123", Some("t")));
        assert_eq!(c.base_url(), "http://example.invalid:8123");
    }

    #[test]
    fn bearer_present_when_token_is_set() {
        let c = HAClient::new(&settings_with("http://supervisor/core", Some("abc")));
        assert_eq!(c.bearer().as_deref(), Some("Bearer abc"));
    }

    #[test]
    fn bearer_absent_when_token_is_missing() {
        let c = HAClient::new(&settings_with("http://supervisor/core", None));
        assert!(c.bearer().is_none());
    }
}
