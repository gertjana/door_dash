//! Page registry.
//!
//! A "page" is a full-screen renderer that returns an 8-bit grayscale
//! image sized to `settings.width × settings.height`. Mirrors the
//! Python addon's `addon/app/render/pages/__init__.py` plug-in style,
//! but the registry is a static array rather than a filesystem scan
//! — Rust's compile-time module list makes runtime discovery
//! unnecessary.
//!
//! Each page module exposes:
//!
//! ```ignore
//! pub async fn render(
//!     settings: Settings,
//!     sensors: LocalSensors,
//!     fw_version: Option<String>,
//! ) -> GrayImage;
//!
//! pub fn render_boxed(
//!     settings: Settings,
//!     sensors: LocalSensors,
//!     fw_version: Option<String>,
//! ) -> RenderFuture;
//! ```
//!
//! `render_boxed` is a thin adapter that boxes the future so the
//! signatures unify into a single function pointer type ([`PageRenderFn`]).
//! The underlying `render` is the natural async fn — `render_boxed`
//! exists purely so the registry can store a homogeneous fn pointer.
//!
//! Pages take **owned** `Settings` and `LocalSensors` so the returned
//! future is `'static` and can be `tokio::spawn`-ed by the HTTP layer
//! without lifetime gymnastics. Both types are cheap to clone (~30
//! short strings + a few primitives).

use std::future::Future;
use std::pin::Pin;

use image::GrayImage;

use crate::config::Settings;
use crate::sources::local_sensors::LocalSensors;

pub mod calendar;
pub mod dashboard;
pub mod energy;
pub mod heart;
pub mod weather;

/// Boxed future returned by every page's render adapter. We require
/// `Send` so the HTTP handler can `tokio::spawn` the work; `'static`
/// so the future doesn't carry borrows back to the caller's stack.
pub type RenderFuture = Pin<Box<dyn Future<Output = GrayImage> + Send + 'static>>;

/// Function pointer to a page's boxed-future render adapter. Stored
/// in the [`Page`] registry so dispatch is a single indirect call.
pub type PageRenderFn =
    fn(settings: Settings, sensors: LocalSensors, fw_version: Option<String>) -> RenderFuture;

/// One registered page. `index` and `name` identify the page in URLs;
/// `title` is the human-readable label exposed in the preview UI.
#[derive(Clone, Copy)]
pub struct Page {
    pub index: usize,
    pub name: &'static str,
    pub title: &'static str,
    pub render_fn: PageRenderFn,
}

impl Page {
    /// Render the page — equivalent to calling the underlying
    /// `render(settings, sensors, fw_version)` async fn.
    pub fn render(
        &self,
        settings: Settings,
        sensors: LocalSensors,
        fw_version: Option<String>,
    ) -> RenderFuture {
        (self.render_fn)(settings, sensors, fw_version)
    }
}

/// Hard-coded ordered list of pages. Order matches the numeric
/// `NN_*.py` prefix on the corresponding Python module so the firmware
/// (which stores the active page index in RTC memory) keeps the same
/// numbering across the Python → Rust port.
pub const PAGES: &[Page] = &[
    Page {
        index: 0,
        name: "dashboard",
        title: "Dashboard",
        render_fn: dashboard::render_boxed,
    },
    Page {
        index: 1,
        name: "calendar",
        title: "Calendar — Month",
        render_fn: calendar::render_boxed,
    },
    Page {
        index: 2,
        name: "weather",
        title: "Weather",
        render_fn: weather::render_boxed,
    },
    Page {
        index: 3,
        name: "heart",
        title: "Yes please Maureen",
        render_fn: heart::render_boxed,
    },
    Page {
        index: 4,
        name: "energy",
        title: "Energy",
        render_fn: energy::render_boxed,
    },
];

/// Resolve a page by URL key — numeric index or canonical name.
///
/// `None` / empty / unknown all fall back to `PAGES[0]`. Numeric keys
/// wrap around modulo `PAGES.len()` so the firmware can blindly
/// increment past the end without ever landing on a 404.
///
/// Negative numeric keys are also handled (via `rem_euclid`) — a
/// "previous page" key of `-1` resolves to the last page rather than
/// erroring out.
pub fn get_page(key: Option<&str>) -> &'static Page {
    let n = PAGES.len() as i64;
    let Some(k) = key else {
        return &PAGES[0];
    };
    let trimmed = k.trim();
    if trimmed.is_empty() {
        return &PAGES[0];
    }
    if let Ok(idx) = trimmed.parse::<i64>() {
        let i = idx.rem_euclid(n) as usize;
        return &PAGES[i];
    }
    PAGES
        .iter()
        .find(|p| p.name == trimmed)
        .unwrap_or(&PAGES[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_contains_five_pages_in_expected_order() {
        // Pin the registry shape: pages are addressable both by index
        // and by name. Adding/removing/reordering pages is a deliberate
        // API change (the firmware persists a numeric page index), so
        // this test exists to flag it loudly in code review.
        assert_eq!(PAGES.len(), 5);
        let names: Vec<&str> = PAGES.iter().map(|p| p.name).collect();
        assert_eq!(
            names,
            vec!["dashboard", "calendar", "weather", "heart", "energy"]
        );
        for (i, p) in PAGES.iter().enumerate() {
            assert_eq!(p.index, i, "PAGES[{i}].index should equal {i}");
        }
    }

    #[test]
    fn get_page_none_returns_dashboard() {
        let p = get_page(None);
        assert_eq!(p.name, "dashboard");
    }

    #[test]
    fn get_page_empty_string_returns_dashboard() {
        assert_eq!(get_page(Some("")).name, "dashboard");
        assert_eq!(get_page(Some("   ")).name, "dashboard");
    }

    #[test]
    fn get_page_by_numeric_index() {
        assert_eq!(get_page(Some("0")).name, "dashboard");
        assert_eq!(get_page(Some("2")).name, "weather");
        assert_eq!(get_page(Some("4")).name, "energy");
    }

    #[test]
    fn get_page_numeric_wraps_around_with_modulo() {
        // 5 pages: index 5 should wrap to 0, 6 to 1, etc.
        assert_eq!(get_page(Some("5")).name, "dashboard");
        assert_eq!(get_page(Some("7")).name, "weather");
    }

    #[test]
    fn get_page_negative_index_wraps_via_rem_euclid() {
        // Python uses `idx % len(PAGES)` which already wraps negative
        // numbers to the back of the list. Rust's `%` doesn't, so we
        // use `rem_euclid` — verify the same semantics.
        assert_eq!(get_page(Some("-1")).name, "energy");
        assert_eq!(get_page(Some("-5")).name, "dashboard");
    }

    #[test]
    fn get_page_by_canonical_name() {
        assert_eq!(get_page(Some("dashboard")).name, "dashboard");
        assert_eq!(get_page(Some("heart")).name, "heart");
        assert_eq!(get_page(Some("energy")).name, "energy");
    }

    #[test]
    fn get_page_unknown_falls_back_to_dashboard() {
        assert_eq!(get_page(Some("definitely-not-a-page")).name, "dashboard");
    }

    #[tokio::test]
    async fn every_registered_page_renders_via_boxed_dispatch() {
        // End-to-end smoke test: hit every page through the public
        // function-pointer entry point (not the underlying `render`
        // async fn) so we'd catch a page whose `render_boxed`
        // adapter forgot to wire up correctly. Also verifies output
        // dimensions match `Settings`.
        let settings = Settings::default();
        let sensors = LocalSensors::default();
        for page in PAGES {
            let img = page.render(settings.clone(), sensors.clone(), None).await;
            assert_eq!(
                img.width(),
                settings.width,
                "{} produced wrong width",
                page.name
            );
            assert_eq!(
                img.height(),
                settings.height,
                "{} produced wrong height",
                page.name
            );
            // Every page should put at least *some* ink on the
            // canvas (title, badge, or content). All-white = bug.
            let dark = img.pixels().filter(|p| p[0] < 200).count();
            assert!(dark > 0, "{} produced an all-white canvas", page.name);
        }
    }
}
