//! HTTP server — axum routes that serve the rendered dashboard.
//!
//! Parity with `addon/app/main.py`. The handlers cover everything the
//! firmware uses (`/dashboard.{png,bmp}`, `/pages`, `/refresh`,
//! `/healthz`) plus the in-browser preview UI at `/`.
//!
//! ## Caching
//!
//! Image rendering is the dominant cost on every request, so we keep
//! a small LRU cache (8 entries) keyed on
//!
//! ```text
//! (sensors.cache_key, time_bucket, fw_version, page_name)
//! ```
//!
//! `time_bucket` rolls over every `settings.refresh_cache_seconds`
//! (default 60 s) which gives the cache a soft TTL: once the bucket
//! changes, every previously-cached entry will miss on its next
//! request and be re-rendered. `nocache=1` bypasses the cache for
//! debugging.
//!
//! Both the BMP and PNG byte streams are stored together so the
//! second-format request after a render is a pure cache hit (BMP and
//! PNG are encoded once each, in parallel, after the initial render).
//!
//! ## Concurrency
//!
//! The cache is wrapped in a `parking_lot::Mutex`. We hold the lock
//! only across hash-table updates — never while rendering or
//! encoding — so request throughput isn't bottlenecked by render
//! time. (Pages do their own async I/O via reqwest, which the
//! Tokio runtime schedules concurrently.)
//!
//! ## Error handling
//!
//! Page rendering is infallible (every page returns a `GrayImage`).
//! BMP encoding is infallible. PNG encoding *can* fail in principle
//! but never does for an in-memory `GrayImage`, so we map any
//! encoding error to HTTP 500 rather than thread the error through
//! every handler.

use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    body::Body,
    extract::{Query, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use image::GrayImage;
use lru::LruCache;
use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::json;
use tracing::info;

use crate::config::Settings;
use crate::render::image_io::{to_bmp_bytes, to_png_bytes};
use crate::render::pages::{get_page, PAGES};
use crate::sources::local_sensors::LocalSensors;
use crate::ADDON_VERSION;

/// Maximum number of cached rendered-page entries. Each entry holds
/// one PNG + one BMP for an 800×480 panel (~30–50 KB each), so 8
/// entries cap memory at well under a megabyte. Eight is enough to
/// cover all 5 pages plus a couple of "with vs without sensors"
/// variants so the firmware's hot path always hits.
const CACHE_MAX: usize = 8;

/// Bundled HTML for the in-browser preview UI. Bytes are baked into
/// the binary at compile time so the runtime image is purely the
/// statically-linked binary — no on-disk asset to serve.
const INDEX_HTML: &str = include_str!("../assets/index.html");

/// Cache key: rounds-and-quantises the per-request inputs to keep
/// near-identical requests sharing a single cache slot.
///
/// Components:
///
/// * `LocalSensors::cache_key()` — sensor readings rounded to whole
///   units (sub-degree drift collapses to one slot).
/// * Time bucket — `now_s / refresh_cache_seconds`. Rolls every
///   refresh window so cached output ages out naturally.
/// * Firmware version — the badge text differs per fw, so different
///   firmwares get different cache entries.
/// * Page name — different pages rendered for the same sensor
///   readings live in different slots.
type CacheKey = ((Option<i64>, Option<i64>, Option<i64>), i64, String, String);

/// One cache entry: both encoded forms of the rendered image.
/// Storing both avoids re-encoding on the second-format request
/// (firmware fetches BMP, browser preview fetches PNG).
#[derive(Clone)]
struct CacheEntry {
    bmp: Vec<u8>,
    png: Vec<u8>,
}

/// Application-wide state shared across all axum handlers via the
/// `axum::extract::State` extractor. Cheap to clone — `Arc` for the
/// cache + a borrowed `Settings` clone (~30 short strings).
#[derive(Clone)]
pub struct AppState {
    pub settings: Settings,
    cache: Arc<Mutex<LruCache<CacheKey, CacheEntry>>>,
}

impl AppState {
    /// Build a fresh app state with an empty cache and the given
    /// pre-loaded `Settings`. Public so `main.rs` and tests can both
    /// construct one.
    pub fn new(settings: Settings) -> Self {
        let cache = LruCache::new(
            NonZeroUsize::new(CACHE_MAX).expect("CACHE_MAX is a non-zero compile-time constant"),
        );
        Self {
            settings,
            cache: Arc::new(Mutex::new(cache)),
        }
    }

    /// Number of entries currently cached. Exposed for `/healthz`.
    pub fn cache_entries(&self) -> usize {
        self.cache.lock().len()
    }

    /// Drop every cached entry. Called by `POST /refresh`.
    pub fn clear_cache(&self) {
        self.cache.lock().clear();
    }
}

/// Build the axum router. Public so tests can spin up the same
/// router an HTTP request would hit, with a custom `AppState`.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(handle_index))
        .route("/healthz", get(handle_healthz))
        .route("/dashboard.png", get(handle_dashboard_png))
        .route("/dashboard.bmp", get(handle_dashboard_bmp))
        .route("/pages", get(handle_pages))
        .route("/refresh", post(handle_refresh))
        .with_state(state)
}

/// Compute the current time bucket (epoch seconds floor-divided by
/// the refresh interval). A fresh bucket invalidates all cache
/// entries from the previous one.
fn time_bucket(settings: &Settings) -> i64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let period = settings.refresh_cache_seconds.max(1);
    (now / period) as i64
}

/// Encode a rendered image to BMP + PNG byte streams. Encoding is
/// lightweight (microseconds) so we always do both — the second
/// format request after a render is then a free hit.
///
/// Returns the encoded entry or an error string suitable for an
/// HTTP 500 body.
fn encode_entry(img: &GrayImage) -> Result<CacheEntry, String> {
    let bmp = to_bmp_bytes(img);
    let png = to_png_bytes(img).map_err(|e| format!("PNG encode failed: {e}"))?;
    Ok(CacheEntry { bmp, png })
}

/// Per-image-request query parameters. All optional; missing params
/// fall through to "no value" which renders the no-sensors variant.
///
/// `nocache` bypasses the cache when truthy (any non-zero number).
/// The Python addon accepts `nocache=1`; we accept any non-zero u32
/// and treat it as a boolean flag.
#[derive(Debug, Deserialize)]
struct DashboardQuery {
    indoor_temp: Option<f64>,
    indoor_hum: Option<f64>,
    battery_pct: Option<f64>,
    fw: Option<String>,
    page: Option<String>,
    #[serde(default)]
    nocache: Option<u32>,
}

/// Look up the requested page in the cache, or render + insert.
/// Returns the entry plus a hit/miss flag (for the `X-Cache` header).
async fn lookup_or_render(
    state: &AppState,
    q: &DashboardQuery,
) -> Result<(CacheEntry, bool), String> {
    let sensors = LocalSensors::from_query(q.indoor_temp, q.indoor_hum, q.battery_pct);
    let page = get_page(q.page.as_deref());
    let fw = q.fw.clone().unwrap_or_default();
    let key: CacheKey = (
        sensors.cache_key(),
        time_bucket(&state.settings),
        fw.clone(),
        page.name.to_string(),
    );

    let bypass = matches!(q.nocache, Some(n) if n != 0);
    if !bypass {
        if let Some(entry) = state.cache.lock().get(&key).cloned() {
            // `LruCache::get` already moves the entry to the
            // most-recently-used end — no extra bookkeeping needed.
            return Ok((entry, true));
        }
    }

    // Render outside the lock — page futures are async, can take
    // tens of ms, and absolutely must not block other handlers.
    let img = page
        .render(state.settings.clone(), sensors, q.fw.clone())
        .await;
    let entry = encode_entry(&img)?;
    state.cache.lock().put(key, entry.clone());
    Ok((entry, false))
}

/// Build a 200 response with the encoded image + standard headers.
/// `X-Cache` is `"hit"` or `"miss"` so the firmware can distinguish
/// served-from-cache responses (no need to wake any data sources)
/// from full re-renders.
fn image_response(content_type: &'static str, bytes: Vec<u8>, hit: bool) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header("X-Cache", if hit { "hit" } else { "miss" })
        .body(Body::from(bytes))
        .expect("constructing a static-shape response can't fail")
}

// === Handlers ============================================================

/// `GET /` — preview UI. Static HTML with a tiny JS shim that lets
/// you flip between pages and toggle synthesised sensor values.
async fn handle_index() -> impl IntoResponse {
    Html(INDEX_HTML)
}

/// `GET /healthz` — liveness + cache-size probe. Mirrors the Python
/// addon's response shape so existing healthcheck tooling keeps
/// working unchanged.
async fn handle_healthz(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(json!({
        "ok": true,
        "cache_entries": state.cache_entries(),
        "addon_version": ADDON_VERSION,
    }))
}

/// `GET /pages` — page registry dump. Used by the firmware to
/// discover the page count + by the preview UI to populate its
/// arrow-key navigation.
async fn handle_pages() -> Json<serde_json::Value> {
    let pages: Vec<serde_json::Value> = PAGES
        .iter()
        .map(|p| json!({"index": p.index, "name": p.name, "title": p.title}))
        .collect();
    Json(json!({
        "count": pages.len(),
        "pages": pages,
    }))
}

/// `POST /refresh` — drop the entire cache. Useful when manually
/// editing config without restarting the addon.
async fn handle_refresh(State(state): State<AppState>) -> Json<serde_json::Value> {
    state.clear_cache();
    info!("cache cleared via POST /refresh");
    Json(json!({"ok": true}))
}

/// `GET /dashboard.png` — render or serve a cached PNG.
async fn handle_dashboard_png(
    State(state): State<AppState>,
    Query(q): Query<DashboardQuery>,
) -> Response {
    match lookup_or_render(&state, &q).await {
        Ok((entry, hit)) => image_response("image/png", entry.png, hit),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

/// `GET /dashboard.bmp` — render or serve a cached BMP.
async fn handle_dashboard_bmp(
    State(state): State<AppState>,
    Query(q): Query<DashboardQuery>,
) -> Response {
    match lookup_or_render(&state, &q).await {
        Ok((entry, hit)) => image_response("image/bmp", entry.bmp, hit),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::to_bytes,
        http::{Method, Request},
    };
    use tower::ServiceExt; // for `.oneshot()`

    /// Build a router on a fresh, empty cache.
    fn fresh_router() -> Router {
        router(AppState::new(Settings::default()))
    }

    /// Helper: send a GET and return (status, headers, body bytes).
    async fn send(
        router: &Router,
        method: Method,
        uri: &str,
    ) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
        let req = Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .expect("test request");
        let resp = router.clone().oneshot(req).await.expect("router serve");
        let status = resp.status();
        let headers = resp.headers().clone();
        // `usize::MAX` keeps the helper general — every test image is
        // bounded by the 800x480 canvas so we never approach it.
        let bytes = to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("collect body");
        (status, headers, bytes.to_vec())
    }

    #[tokio::test]
    async fn healthz_returns_ok_payload() {
        let r = fresh_router();
        let (status, _, body) = send(&r, Method::GET, "/healthz").await;
        assert_eq!(status, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["cache_entries"], 0);
        assert_eq!(v["addon_version"], ADDON_VERSION);
    }

    #[tokio::test]
    async fn pages_returns_five_pages_in_order() {
        let r = fresh_router();
        let (status, _, body) = send(&r, Method::GET, "/pages").await;
        assert_eq!(status, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["count"], 5);
        let names: Vec<&str> = v["pages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            vec!["dashboard", "calendar", "weather", "heart", "energy"]
        );
    }

    #[tokio::test]
    async fn index_returns_html_with_preview_markup() {
        let r = fresh_router();
        let (status, headers, body) = send(&r, Method::GET, "/").await;
        assert_eq!(status, StatusCode::OK);
        assert!(headers
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("text/html"));
        let body_str = std::str::from_utf8(&body).unwrap();
        // Sanity: the bundled HTML mentions the preview-page elements.
        assert!(body_str.contains("dashboard.png"));
        assert!(body_str.contains("pages"));
    }

    #[tokio::test]
    async fn dashboard_png_renders_and_caches() {
        // Use a single shared state across calls so the second call
        // can hit the cache. `oneshot` consumes the router but a
        // shared AppState lets us build fresh routers per request.
        let state = AppState::new(Settings::default());
        let r1 = router(state.clone());
        let r2 = router(state.clone());

        let (status, headers, body) = send(&r1, Method::GET, "/dashboard.png").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            headers.get(header::CONTENT_TYPE).unwrap().to_str().unwrap(),
            "image/png"
        );
        assert_eq!(headers.get("X-Cache").unwrap().to_str().unwrap(), "miss");
        // PNG signature: 89 50 4E 47 0D 0A 1A 0A
        assert_eq!(&body[..8], b"\x89PNG\r\n\x1a\n");

        // Second call with identical query → cache hit.
        let (_, headers, _) = send(&r2, Method::GET, "/dashboard.png").await;
        assert_eq!(headers.get("X-Cache").unwrap().to_str().unwrap(), "hit");
    }

    #[tokio::test]
    async fn dashboard_bmp_returns_bmp_bytes() {
        let r = fresh_router();
        let (status, headers, body) = send(&r, Method::GET, "/dashboard.bmp").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            headers.get(header::CONTENT_TYPE).unwrap().to_str().unwrap(),
            "image/bmp"
        );
        // BMP magic: "BM" at offset 0.
        assert_eq!(&body[..2], b"BM");
    }

    #[tokio::test]
    async fn nocache_query_bypasses_cache() {
        // Even if a previous render populated the cache, nocache=1
        // should always render fresh and report `X-Cache: miss`.
        let state = AppState::new(Settings::default());
        let _ = send(&router(state.clone()), Method::GET, "/dashboard.png").await;
        // Now the cache holds an entry.
        assert_eq!(state.cache_entries(), 1);
        let (_, headers, _) = send(
            &router(state.clone()),
            Method::GET,
            "/dashboard.png?nocache=1",
        )
        .await;
        assert_eq!(headers.get("X-Cache").unwrap().to_str().unwrap(), "miss");
    }

    #[tokio::test]
    async fn refresh_clears_the_cache() {
        let state = AppState::new(Settings::default());
        let _ = send(&router(state.clone()), Method::GET, "/dashboard.png").await;
        assert_eq!(state.cache_entries(), 1);

        let (status, _, body) = send(&router(state.clone()), Method::POST, "/refresh").await;
        assert_eq!(status, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(state.cache_entries(), 0);
    }

    #[tokio::test]
    async fn different_pages_get_separate_cache_entries() {
        let state = AppState::new(Settings::default());
        let _ = send(
            &router(state.clone()),
            Method::GET,
            "/dashboard.png?page=dashboard",
        )
        .await;
        let _ = send(
            &router(state.clone()),
            Method::GET,
            "/dashboard.png?page=weather",
        )
        .await;
        // Two distinct page names → two separate cache slots.
        assert_eq!(state.cache_entries(), 2);
    }

    #[tokio::test]
    async fn page_query_accepts_numeric_index_and_wraps() {
        // page=999 should wrap via rem_euclid and resolve to one of
        // the registered pages. The render must still succeed.
        let r = fresh_router();
        let (status, headers, _) = send(&r, Method::GET, "/dashboard.png?page=999").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers.get("X-Cache").unwrap().to_str().unwrap(), "miss");
    }

    #[tokio::test]
    async fn unknown_page_falls_back_to_dashboard() {
        // Bad name → dashboard. Render still succeeds.
        let r = fresh_router();
        let (status, _, _) = send(&r, Method::GET, "/dashboard.png?page=does-not-exist").await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn sensor_values_round_into_same_cache_key() {
        // 21.4 vs 21.49 both round to 21 → same cache key, so the
        // second request hits the cache.
        let state = AppState::new(Settings::default());
        let _ = send(
            &router(state.clone()),
            Method::GET,
            "/dashboard.png?indoor_temp=21.4",
        )
        .await;
        let (_, headers, _) = send(
            &router(state.clone()),
            Method::GET,
            "/dashboard.png?indoor_temp=21.49",
        )
        .await;
        assert_eq!(headers.get("X-Cache").unwrap().to_str().unwrap(), "hit");
    }

    #[tokio::test]
    async fn time_bucket_increments_with_real_clock() {
        // Sanity: bucket should change at most once per
        // `refresh_cache_seconds`. Hard to trigger a rollover in a
        // unit test (would have to wait), but we can at least
        // verify it's a stable function of the current second.
        let s = Settings::default();
        let b1 = time_bucket(&s);
        let b2 = time_bucket(&s);
        // Can differ by 1 if a bucket boundary lands between the
        // two calls; should never differ by more.
        assert!((b2 - b1).abs() <= 1);
    }
}
