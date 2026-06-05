//! Page 0 — the main dashboard.
//!
//! Layout regions (mirrors `addon/app/render/pages/00_dashboard.py`):
//!
//! ```text
//! +---------------------+------------------------------------+
//! |  QR                 |  Weather                           |
//! +---------------------+------------------------------------+
//! |  Indoors            |  Calendar list                     |
//! +---------------------+                                    |
//! |  Tesla              |                                    |
//! +---------------------+                                    |
//! |  date+hh:mm refresh |                                    |
//! +---------------------+------------------------------------+
//! ```
//!
//! All page-specific layout decisions live in this file. Per-widget
//! drawing is delegated to `render::widgets::*`.

use chrono::Utc;
use chrono_tz::Tz;
use image::{GrayImage, Luma};
use imageproc::drawing::draw_line_segment_mut;

use crate::config::Settings;
use crate::render::badge::draw_version_badge;
use crate::render::blank_canvas;
use crate::render::fonts::{draw_crisp_text, draw_text, font_for, text_width, Weight};
use crate::render::pages::RenderFuture;
use crate::render::widgets::{calendar_list, local_sensors, qr, tesla, weather, Rect};
use crate::sources;
use crate::sources::local_sensors::LocalSensors;

// === Layout constants (page-specific; do NOT lift to a shared module) ===
//
// Tweak these in one place to retune the dashboard. All values are in
// pixels; the page works on any panel size because the right column,
// calendar height, and footer position are derived from
// `settings.width`/`settings.height` rather than hard-coded.
const LEFT_COL_WIDTH: i32 = 260;
const WEATHER_ROW_HEIGHT: i32 = 124;
/// Reserved strip at the bottom of the left column for the date +
/// refresh time stamp.
const FOOTER_HEIGHT: i32 = 38;

/// Resolve `settings.timezone` to a chrono-tz zone, falling back to
/// UTC for unparseable names. Same fallback as the widget layer.
fn timezone(settings: &Settings) -> Tz {
    settings.timezone.parse::<Tz>().unwrap_or(chrono_tz::UTC)
}

/// Draw the column/row separator lines that frame the layout.
fn draw_chrome(canvas: &mut GrayImage, width: i32, height: i32) {
    // Vertical divider between left and right columns.
    draw_line_segment_mut(
        canvas,
        (LEFT_COL_WIDTH as f32, 0.0),
        (LEFT_COL_WIDTH as f32, (height - 1) as f32),
        Luma([0]),
    );
    // Horizontal divider in the right column under the weather row.
    draw_line_segment_mut(
        canvas,
        (LEFT_COL_WIDTH as f32, WEATHER_ROW_HEIGHT as f32),
        ((width - 1) as f32, WEATHER_ROW_HEIGHT as f32),
        Luma([0]),
    );
}

/// Thin horizontal rule across the left column, slightly inset.
fn left_column_rule(canvas: &mut GrayImage, left: Rect, y: i32) {
    draw_line_segment_mut(
        canvas,
        ((left.x + 8) as f32, y as f32),
        ((left.x + left.w - 8) as f32, y as f32),
        Luma([0]),
    );
}

/// Bottom-left footer: today's date + a "Refreshed HH:MM" stamp.
fn draw_footer(canvas: &mut GrayImage, settings: &Settings, footer_top: i32) {
    let tz = timezone(settings);
    let now = Utc::now().with_timezone(&tz);

    // Thin separator above the footer.
    draw_line_segment_mut(
        canvas,
        (8.0, footer_top as f32),
        ((LEFT_COL_WIDTH - 8) as f32, footer_top as f32),
        Luma([0]),
    );

    // Top line: today's date, centred. Format mirrors Python's
    // strftime("%A %d %B") — locale C, e.g. "Monday 25 May".
    let date_text = now.format("%A %d %B").to_string();
    let date_f = font_for(Weight::Bold);
    let date_size: f32 = 16.0;
    let date_w = text_width(date_f, date_size, &date_text).ceil() as i32;
    let date_x = (LEFT_COL_WIDTH - date_w) / 2;
    draw_text(
        canvas,
        date_x,
        footer_top + 3,
        &date_text,
        date_f,
        date_size,
        0,
    );

    // Bottom line: "Refreshed HH:MM", centred and crisp.
    let refreshed_text = format!("Refreshed {}", now.format("%H:%M"));
    let refreshed_f = font_for(Weight::Regular);
    let refreshed_size: f32 = 12.0;
    let ref_w = text_width(refreshed_f, refreshed_size, &refreshed_text).ceil() as i32;
    let ref_x = (LEFT_COL_WIDTH - ref_w) / 2;
    draw_crisp_text(
        canvas,
        ref_x,
        footer_top + 22,
        &refreshed_text,
        refreshed_f,
        refreshed_size,
        0,
    );
}

/// Render the dashboard. See module docstring for layout details.
pub async fn render(
    settings: Settings,
    sensors: LocalSensors,
    fw_version: Option<String>,
) -> GrayImage {
    // Three independent HA fetches — kick them off in parallel and
    // await all three at once. Halves the page render latency vs the
    // sequential Python implementation when HA is on the slow side.
    let (weather_state, tesla_state, calendar_events) = tokio::join!(
        sources::weather::fetch(&settings),
        sources::tesla::fetch(&settings),
        sources::calendar::fetch(&settings),
    );

    let w = settings.width as i32;
    let h = settings.height as i32;
    let mut img = blank_canvas(settings.width, settings.height);

    draw_chrome(&mut img, w, h);

    let right_w = w - LEFT_COL_WIDTH;
    let main_h = h;
    // Left column reserves the bottom strip for the timestamp footer.
    let left_widget_h = main_h - FOOTER_HEIGHT;

    let weather_box = Rect::new(LEFT_COL_WIDTH, 0, right_w, WEATHER_ROW_HEIGHT);
    let calendar_box = Rect::new(
        LEFT_COL_WIDTH,
        WEATHER_ROW_HEIGHT,
        right_w,
        main_h - WEATHER_ROW_HEIGHT,
    );
    let left = Rect::new(0, 0, LEFT_COL_WIDTH, left_widget_h);

    let show_sensors = settings.show_local_sensors;
    let show_tesla = settings.show_tesla;

    // Left-column row weights (sum normalised to fill the available
    // height). When a section is disabled we leave it out of the
    // weight list, so the remaining sections grow to fill the space.
    let mut weights: Vec<(&str, f32)> = vec![("qr", 0.46)];
    if show_sensors {
        weights.push(("sensors", 0.26));
    }
    if show_tesla {
        weights.push(("tesla", 0.28));
    }

    let total_weight: f32 = weights.iter().map(|(_, w)| *w).sum();
    let mut heights: Vec<i32> = weights
        .iter()
        .map(|(_, w)| ((left_widget_h as f32) * (*w / total_weight)) as i32)
        .collect();
    // Push any rounding drift into the last section so we always fill
    // the column exactly — avoids a 1-2 px gap above the footer rule.
    let drift = left_widget_h - heights.iter().sum::<i32>();
    if let Some(last) = heights.last_mut() {
        *last += drift;
    }

    // Walk the row weights, computing each section's box and drawing
    // separator rules between adjacent sections.
    let mut y = 0;
    let mut qr_box: Option<Rect> = None;
    let mut sensors_box: Option<Rect> = None;
    let mut tesla_box: Option<Rect> = None;
    for (i, &(name, _)) in weights.iter().enumerate() {
        let row_h = heights[i];
        let bx = Rect::new(left.x, y, left.w, row_h);
        match name {
            "qr" => qr_box = Some(bx),
            "sensors" => sensors_box = Some(bx),
            "tesla" => tesla_box = Some(bx),
            _ => {}
        }
        y += row_h;
        if i < weights.len() - 1 {
            left_column_rule(&mut img, left, y);
        }
    }

    // === Render widgets ==================================================
    if let Some(b) = qr_box {
        qr::render(&mut img, &settings, b);
    }
    weather::render(&mut img, &settings, &weather_state, weather_box);
    if let (true, Some(b)) = (show_sensors, sensors_box) {
        local_sensors::render(&mut img, &sensors, b);
    }
    if let (true, Some(b)) = (show_tesla, tesla_box) {
        tesla::render(&mut img, &tesla_state, b);
    }
    calendar_list::render(&mut img, &settings, &calendar_events, calendar_box);

    // Footer + version badge.
    draw_footer(&mut img, &settings, left_widget_h);
    draw_version_badge(&mut img, &settings, fw_version.as_deref());

    img
}

/// Boxed-future adapter over [`render`] — see [`super::PageRenderFn`].
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

    fn fresh_settings() -> Settings {
        // Disable HA fetch paths by leaving the supervisor token unset
        // and using the default base URL — `HAClient::available()`
        // returns false and the sources fall through to fallbacks.
        Settings::default()
    }

    #[tokio::test]
    async fn render_returns_canvas_of_settings_dimensions() {
        let s = fresh_settings();
        let img = render(s.clone(), LocalSensors::default(), None).await;
        assert_eq!(img.width(), s.width);
        assert_eq!(img.height(), s.height);
    }

    #[tokio::test]
    async fn render_writes_some_dark_pixels() {
        // Smoke check: the page should leave at least *some* black
        // pixels behind (chrome lines, widget titles, etc.).
        let s = fresh_settings();
        let img = render(s, LocalSensors::default(), Some("1.2.3".to_string())).await;
        let dark = img.pixels().filter(|p| p[0] < 200).count();
        assert!(dark > 0, "dashboard render produced no dark pixels");
    }

    #[tokio::test]
    async fn render_with_show_tesla_off_still_succeeds() {
        // Disabling sections changes the weight distribution; verify
        // we don't divide-by-zero or panic when a section is skipped.
        let mut s = fresh_settings();
        s.show_tesla = false;
        s.show_local_sensors = false;
        let img = render(s, LocalSensors::default(), None).await;
        let dark = img.pixels().filter(|p| p[0] < 200).count();
        assert!(dark > 0);
    }
}
