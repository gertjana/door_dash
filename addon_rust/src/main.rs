//! reTerminal ePaper dashboard — Rust port.
//!
//! Serves an 800×480 1-bit dashboard image over HTTP. The compiled
//! binary is the entire add-on — fonts, MDI icons and HTML preview UI
//! are all bundled at compile time via `include_bytes!` / `include_str!`,
//! so the runtime image is just the binary plus an empty Alpine base.
//!
//! Layered out to mirror the Python addon under `addon/`:
//!
//! ```text
//! src/
//! ├── main.rs              — process entry: build app router, bind, run
//! ├── lib.rs               — module re-exports + ADDON_VERSION constant
//! ├── config.rs            — Settings (env + /data/options.json)
//! ├── ha_client.rs         — minimal Home Assistant REST client
//! ├── render/
//! │   ├── image_io.rs      — to_mono / to_bmp_bytes / to_png_bytes
//! │   ├── fonts.rs         — TTF cache, draw_crisp_text helpers
//! │   ├── icons.rs         — MDI codepoint table + icon_for_weather_state
//! │   ├── badge.rs         — version badge helper (top-right corner)
//! │   ├── sparkline.rs     — 1-bit polyline renderer
//! │   ├── widgets/         — Dashboard sub-panels
//! │   └── pages/           — Full-screen page renderers + registry
//! └── sources/             — Data fetchers (weather, calendar, …)
//! ```
//!
//! See the page registry in `render::pages` for how individual pages
//! are wired up.

use std::net::SocketAddr;

use anyhow::{Context, Result};
use axum::{routing::get, Router};
use tracing::info;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use epaper_dashboard_rust::ADDON_VERSION;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    info!(
        version = ADDON_VERSION,
        "starting reTerminal ePaper dashboard (Rust port)"
    );

    // Subsequent phases will add dashboard.png/.bmp, /pages, /refresh
    // etc; for now a /healthz probe is enough to verify the binary
    // boots inside the HA add-on container.
    let app = Router::new().route("/healthz", get(healthz));

    // Port is overridable via EPDASH_PORT for local dev; defaults to
    // 8099 to match config.yaml's `ingress_port`. Lets us boot the
    // Rust port alongside a still-running Python addon during the
    // parity-check phase.
    let port: u16 = std::env::var("EPDASH_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8099);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("bind {addr}"))?;
    info!(%addr, "listening");
    axum::serve(listener, app)
        .await
        .context("axum server failure")?;
    Ok(())
}

/// `/healthz` — simple "is the binary up?" probe. Mirrors the Python
/// addon's response shape so health-check tooling can be reused 1:1.
async fn healthz() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "ok": true,
        "addon_version": ADDON_VERSION,
    }))
}

/// Subscriber that respects `RUST_LOG` (defaulting to `info`) and emits
/// to stderr in HA's expected `<ts> <LEVEL> ...` format. Mirrors the
/// `logging.basicConfig` setup in the Python addon's `main.py`.
fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(true).with_level(true))
        .init();
}
