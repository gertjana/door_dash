//! Page 3 — "Yes please Maureen".
//!
//! Renders a large filled heart centred horizontally with a romantic
//! prompt below it. Self-contained: no data sources, no widgets — just
//! direct primitive drawing.
//!
//! The heart is sampled from the classic parametric curve rather than
//! constructed from circles + triangle so we get a single clean
//! silhouette with no fusion seams. Sampling at 360 points produces a
//! polygon that's smooth at any reasonable size.
//!
//! Mirrors `addon/app/render/pages/03_heart.py`.

use image::GrayImage;
use imageproc::drawing::draw_polygon_mut;
use imageproc::point::Point;
use std::f32::consts::PI;
use tracing::info;

use crate::config::Settings;
use crate::render::badge::draw_version_badge;
use crate::render::blank_canvas;
use crate::render::fonts::{draw_text, font_for, text_height, text_width, Weight};
use crate::render::pages::RenderFuture;
use crate::sources::local_sensors::LocalSensors;

// === Layout tunables =====================================================
/// Heart's longer dimension is sized as a fraction of the panel's
/// shorter dimension so it scales gracefully with display size.
const HEART_SIZE_FRACTION: f32 = 0.55;
const PROMPT: &str = "Yes please Maureen";
const PROMPT_FONT_SIZE: f32 = 44.0;
/// Vertical gap between the heart's bottom edge and the prompt's top.
const GAP_BELOW_HEART: i32 = 24;
/// Number of polygon samples around the parametric curve. 360 gives
/// degree-resolution which is smooth enough at any panel size we'd
/// realistically render on.
const HEART_SAMPLES: usize = 360;

/// Generate the points of a filled heart centred on `(cx, cy)` with
/// overall longest-dimension `size`.
///
/// Uses the parametric curve
///
/// ```text
///   x(t) = 16 · sin³(t)
///   y(t) = 13 · cos(t) − 5 · cos(2t) − 2 · cos(3t) − cos(4t)
/// ```
///
/// First pass computes raw `(x, y)` so we can measure the bounding
/// box; second pass rescales to fit `size` and flips Y because the
/// curve's positive-Y points upward in math convention but the image
/// plane's Y grows downward.
///
/// `imageproc::draw_polygon_mut` closes the polygon automatically and
/// **panics if the first and last point are equal**. The samples
/// `i = 0..N` use `t = 2π·i/N`, so `t = 2π` is never sampled and
/// duplicate endpoints never occur — but we explicitly drop any
/// trailing duplicate as a defence in depth in case the formula is
/// ever changed.
fn heart_polygon(cx: i32, cy: i32, size: i32) -> Vec<Point<i32>> {
    // First pass: raw curve points so we can measure the bbox.
    let mut raw = Vec::with_capacity(HEART_SAMPLES);
    for i in 0..HEART_SAMPLES {
        let t = 2.0 * PI * (i as f32) / (HEART_SAMPLES as f32);
        let x = 16.0 * t.sin().powi(3);
        let y = 13.0 * t.cos() - 5.0 * (2.0 * t).cos() - 2.0 * (3.0 * t).cos() - (4.0 * t).cos();
        raw.push((x, y));
    }
    let (mut xmin, mut xmax) = (f32::INFINITY, f32::NEG_INFINITY);
    let (mut ymin, mut ymax) = (f32::INFINITY, f32::NEG_INFINITY);
    for &(x, y) in &raw {
        xmin = xmin.min(x);
        xmax = xmax.max(x);
        ymin = ymin.min(y);
        ymax = ymax.max(y);
    }
    let raw_w = xmax - xmin;
    let raw_h = ymax - ymin;
    let scale = size as f32 / raw_w.max(raw_h);
    let raw_cx = (xmin + xmax) * 0.5;
    let raw_cy = (ymin + ymax) * 0.5;

    let mut pts: Vec<Point<i32>> = raw
        .into_iter()
        .map(|(x, y)| {
            // Flip Y so the apex points down (image-space orientation).
            let px = (cx as f32 + (x - raw_cx) * scale).round() as i32;
            let py = (cy as f32 - (y - raw_cy) * scale).round() as i32;
            Point::new(px, py)
        })
        .collect();

    // Defence in depth: imageproc panics if first==last. After
    // rounding to integer pixels two distinct sample points can
    // collapse onto the same pixel, so drop trailing duplicates of
    // the leading vertex.
    while pts.len() > 1 && pts.last() == pts.first() {
        pts.pop();
    }
    pts
}

/// Render the heart + prompt page. Returns an 8-bit greyscale image
/// (white background, black heart, black prompt text).
pub async fn render(
    settings: Settings,
    _sensors: LocalSensors,
    fw_version: Option<String>,
) -> GrayImage {
    let w = settings.width as i32;
    let h = settings.height as i32;
    let mut img = blank_canvas(settings.width, settings.height);

    let heart_size = (w.min(h) as f32 * HEART_SIZE_FRACTION) as i32;

    let prompt_f = font_for(Weight::Bold);
    let prompt_w = text_width(prompt_f, PROMPT_FONT_SIZE, PROMPT).ceil() as i32;
    let prompt_h = text_height(prompt_f, PROMPT_FONT_SIZE).ceil() as i32;

    // Vertically centre the heart+prompt group as a single unit so
    // the page reads as one composition rather than two stacked
    // elements drifting away from the viewport's centre.
    let group_h = heart_size + GAP_BELOW_HEART + prompt_h;
    let group_top = (h - group_h) / 2;

    let heart_cx = w / 2;
    let heart_cy = group_top + heart_size / 2;
    let pts = heart_polygon(heart_cx, heart_cy, heart_size);

    // Polygon needs at least 3 distinct vertices. Anything less and
    // imageproc would panic, so just skip the fill — never going to
    // happen in practice given HEART_SAMPLES = 360.
    if pts.len() >= 3 {
        draw_polygon_mut(&mut img, &pts, image::Luma([0]));
    }

    let prompt_x = (w - prompt_w) / 2;
    let prompt_y = group_top + heart_size + GAP_BELOW_HEART;
    draw_text(
        &mut img,
        prompt_x,
        prompt_y,
        PROMPT,
        prompt_f,
        PROMPT_FONT_SIZE,
        0,
    );

    info!(heart_size, prompt_w, "heart page: rendered");

    draw_version_badge(&mut img, &settings, fw_version.as_deref());
    img
}

/// Boxed-future adapter — see [`super::PageRenderFn`].
pub fn render_boxed(
    settings: Settings,
    sensors: LocalSensors,
    fw_version: Option<String>,
) -> RenderFuture {
    Box::pin(render(settings, sensors, fw_version))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heart_polygon_returns_many_distinct_points() {
        let pts = heart_polygon(400, 240, 200);
        // We expect roughly HEART_SAMPLES vertices; trailing
        // duplicates may have been popped, but we should still have
        // close to the full count.
        assert!(
            pts.len() >= HEART_SAMPLES - 5,
            "expected ~{} points, got {}",
            HEART_SAMPLES,
            pts.len()
        );
    }

    #[test]
    fn heart_polygon_first_not_equal_last() {
        // imageproc panics on first==last; ensure our dedupe holds.
        let pts = heart_polygon(400, 240, 200);
        assert_ne!(
            pts.first(),
            pts.last(),
            "first and last vertices must differ"
        );
    }

    #[test]
    fn heart_polygon_fits_within_size() {
        let cx = 400;
        let cy = 240;
        let size = 200;
        let pts = heart_polygon(cx, cy, size);
        let xs: Vec<i32> = pts.iter().map(|p| p.x).collect();
        let ys: Vec<i32> = pts.iter().map(|p| p.y).collect();
        let span_x = xs.iter().max().unwrap() - xs.iter().min().unwrap();
        let span_y = ys.iter().max().unwrap() - ys.iter().min().unwrap();
        // Longer dimension should be ≤ size (rounding may shave a px).
        assert!(span_x <= size + 1, "x span {span_x} > size {size}");
        assert!(span_y <= size + 1, "y span {span_y} > size {size}");
        // And the polygon roughly straddles the centre.
        let mean_x = xs.iter().sum::<i32>() / xs.len() as i32;
        assert!((mean_x - cx).abs() < 5, "mean_x {mean_x} not near cx {cx}");
    }

    #[tokio::test]
    async fn render_returns_canvas_of_settings_dimensions() {
        let s = Settings::default();
        let img = render(s.clone(), LocalSensors::default(), None).await;
        assert_eq!(img.width(), s.width);
        assert_eq!(img.height(), s.height);
    }

    #[tokio::test]
    async fn render_writes_substantial_dark_pixels() {
        // The heart is large (~55% of the shorter dim filled black), so
        // a meaningful fraction of pixels should end up dark.
        let s = Settings::default();
        let img = render(s.clone(), LocalSensors::default(), None).await;
        let total = (s.width * s.height) as usize;
        let dark = img.pixels().filter(|p| p[0] < 128).count();
        assert!(
            dark > total / 50,
            "heart render produced too few dark pixels: {dark}/{total}"
        );
    }
}
