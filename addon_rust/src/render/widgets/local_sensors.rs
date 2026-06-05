//! Indoors widget — onboard temp/humidity + battery state.
//!
//! Layout:
//!
//! ```text
//!     +-----------------+-----------------+
//!     |  Temp           |  Humidity       |
//!     +-----------------+-----------------+
//!     |  [████████      ]  87%            |
//!     +-----------------+-----------------+
//! ```
//!
//! Top row holds the two readings side-by-side; the bottom strip is a
//! full-width battery bar with its percentage label to the right. All
//! values are optional; missing readings render as an em-dash so cold
//! boot still produces sensible output.
//!
//! Mirrors `addon/app/render/widgets/local_sensors.py`.

use image::{GrayImage, Luma};
use imageproc::drawing::draw_filled_rect_mut;

use crate::render::fonts::{
    draw_crisp_text, draw_text, font_for, text_height, text_width, Weight, TITLE_SIZE,
};
use crate::render::widgets::Rect;
use crate::sources::local_sensors::LocalSensors;

/// Format a single reading. `None` renders as an em-dash.
fn fmt(value: Option<f64>, suffix: &str) -> String {
    match value {
        None => "\u{2014}".to_string(),
        Some(v) => format!("{}{suffix}", v.round() as i64),
    }
}

/// Draw a battery-shaped bar with a small positive-terminal nub.
///
/// `w` is the *total* width including the nub, so callers can size
/// the whole battery to a fixed region without separately accounting
/// for the terminal.
fn draw_battery_bar(canvas: &mut GrayImage, x: i32, y: i32, w: i32, h: i32, pct: Option<f64>) {
    let nub_w: i32 = 4;
    let body_w = w - nub_w;
    if body_w <= 0 || h <= 0 {
        return;
    }

    // 2-pixel-thick body outline. imageproc's `draw_hollow_rect_mut`
    // draws a 1-px outline, so emulate width=2 by drawing an outer
    // black rect and filling the centre back to white.
    draw_filled_rect_mut(
        canvas,
        imageproc::rect::Rect::at(x, y).of_size(body_w as u32, (h + 1) as u32),
        Luma([0]),
    );
    if body_w > 4 && h > 3 {
        // Restore the white interior, leaving a 2-px frame.
        draw_filled_rect_mut(
            canvas,
            imageproc::rect::Rect::at(x + 2, y + 2).of_size((body_w - 4) as u32, (h - 3) as u32),
            Luma([255]),
        );
    }

    // Positive-terminal nub on the right edge — Pillow uses
    // ``y + h // 4`` ... ``y + h - h // 4`` so the nub spans roughly
    // the middle half of the body height. Note: integer arithmetic
    // matters — `h // 4` and `h - h // 4` are not symmetric for odd `h`.
    let nub_top = y + h / 4;
    let nub_bottom = y + h - h / 4;
    let nub_h = (nub_bottom - nub_top).max(1) as u32;
    if nub_h > 0 {
        draw_filled_rect_mut(
            canvas,
            imageproc::rect::Rect::at(x + body_w, nub_top).of_size(nub_w as u32, nub_h),
            Luma([0]),
        );
    }

    if let Some(pct) = pct {
        let inner_x = x + 3;
        let inner_y = y + 3;
        let inner_w = body_w - 6;
        let inner_h = h - 6;
        if inner_w > 0 && inner_h > 0 {
            let clamped = pct.clamp(0.0, 100.0);
            let fill_w = ((inner_w as f64) * clamped / 100.0).floor() as i32;
            if fill_w > 0 {
                draw_filled_rect_mut(
                    canvas,
                    imageproc::rect::Rect::at(inner_x, inner_y)
                        .of_size(fill_w as u32, inner_h as u32),
                    Luma([0]),
                );
            }
        }
    }
}

/// Render a labeled metric (label small on top, value big below).
fn draw_metric(canvas: &mut GrayImage, x: i32, y: i32, label: &str, value: &str) {
    let label_f = font_for(Weight::Regular);
    let label_size: f32 = 13.0;
    let value_f = font_for(Weight::Bold);
    let value_size: f32 = 22.0;
    draw_crisp_text(canvas, x, y, label, label_f, label_size, 0);
    draw_text(canvas, x, y + 14, value, value_f, value_size, 0);
}

/// Render the indoors widget into `rect`.
pub fn render(canvas: &mut GrayImage, sensors: &LocalSensors, rect: Rect) {
    let title_f = font_for(Weight::Bold);
    draw_text(
        canvas,
        rect.x + 8,
        rect.y + 4,
        "Indoors",
        title_f,
        TITLE_SIZE,
        0,
    );

    // Geometry ----------------------------------------------------------
    // The widget is split into two horizontal strips:
    //   * top: temp + humidity side-by-side (the bulk of the height)
    //   * bottom: battery bar spanning the widget width
    let content_top = rect.y + 32;
    let side_inset: i32 = 10;
    let bottom_pad: i32 = 8; // gap above the next widget's separator

    // Battery row reserves a fixed strip at the bottom; everything
    // above it is for the temp/hum readings.
    let battery_h: i32 = 16;
    let battery_y = rect.y + rect.h - bottom_pad - battery_h;
    let pct_label_w: i32 = 56; // room for "100%" in the bold label font

    // Top row: temp + humidity, evenly split.
    let half_w = rect.w / 2;
    let tl_x = rect.x + side_inset;
    let tr_x = rect.x + half_w + side_inset - 6;
    let top_y = content_top + 4;

    draw_metric(
        canvas,
        tl_x,
        top_y,
        "Temp",
        &fmt(sensors.indoor_temp, "\u{00b0}C"),
    );
    draw_metric(canvas, tr_x, top_y, "Hum.", &fmt(sensors.indoor_hum, "%"));

    // Bottom: full-width battery bar + percentage label to the right.
    let bar_x = rect.x + side_inset;
    let bar_right_limit = rect.x + rect.w - side_inset;
    let bar_w = bar_right_limit - bar_x - pct_label_w - 6;
    draw_battery_bar(
        canvas,
        bar_x,
        battery_y,
        bar_w,
        battery_h,
        sensors.battery_pct,
    );

    let pct_text = fmt(sensors.battery_pct, "%");
    let pct_f = font_for(Weight::Bold);
    let pct_size: f32 = 14.0;
    let pct_w = text_width(pct_f, pct_size, &pct_text).ceil() as i32;
    let pct_h = text_height(pct_f, pct_size).ceil() as i32;
    // Right-align label within its reserved column; vertically centre
    // against the bar.
    let pct_x = bar_right_limit - pct_w;
    let pct_y = battery_y + (battery_h - pct_h) / 2 - 1;
    draw_crisp_text(canvas, pct_x, pct_y, &pct_text, pct_f, pct_size, 0);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_canvas(w: u32, h: u32) -> GrayImage {
        GrayImage::from_pixel(w, h, Luma([255]))
    }

    fn dark_count(canvas: &GrayImage) -> usize {
        canvas.pixels().filter(|p| p[0] < 200).count()
    }

    #[test]
    fn fmt_dash_for_none() {
        assert_eq!(fmt(None, "%"), "\u{2014}");
        assert_eq!(fmt(None, "\u{00b0}C"), "\u{2014}");
    }

    #[test]
    fn fmt_rounds_and_appends_suffix() {
        assert_eq!(fmt(Some(21.4), "\u{00b0}C"), "21\u{00b0}C");
        assert_eq!(fmt(Some(86.6), "%"), "87%");
    }

    #[test]
    fn battery_bar_with_full_pct_fills_inside_frame() {
        let mut canvas = fresh_canvas(120, 30);
        // Drawing pct=100 should leave dark pixels well inside the
        // outer frame.
        draw_battery_bar(&mut canvas, 5, 5, 100, 16, Some(100.0));
        // Sample a pixel near the centre of the bar — should be black.
        let centre = canvas.get_pixel(50, 12)[0];
        assert!(
            centre < 50,
            "centre pixel should be filled black, got {centre}"
        );
    }

    #[test]
    fn battery_bar_with_empty_pct_leaves_inside_white() {
        let mut canvas = fresh_canvas(120, 30);
        draw_battery_bar(&mut canvas, 5, 5, 100, 16, Some(0.0));
        // Inside the frame should still be white.
        let centre = canvas.get_pixel(50, 12)[0];
        assert!(
            centre > 200,
            "centre should be white for 0% fill, got {centre}"
        );
    }

    #[test]
    fn battery_bar_clamps_above_100() {
        let mut canvas = fresh_canvas(120, 30);
        draw_battery_bar(&mut canvas, 5, 5, 100, 16, Some(500.0));
        // Right-most inner pixel should be filled (treated as 100%).
        let near_right = canvas.get_pixel(95, 12)[0];
        assert!(
            near_right < 50,
            "above-100 pct should clamp to full fill, got {near_right}"
        );
    }

    #[test]
    fn battery_bar_with_none_draws_outline_only() {
        let mut canvas = fresh_canvas(120, 30);
        draw_battery_bar(&mut canvas, 5, 5, 100, 16, None);
        // Outline present (non-zero dark count); interior white.
        assert!(dark_count(&canvas) > 0);
        let centre = canvas.get_pixel(50, 12)[0];
        assert!(centre > 200, "no fill expected for None, got {centre}");
    }

    #[test]
    fn battery_bar_zero_size_does_not_panic() {
        let mut canvas = fresh_canvas(20, 10);
        draw_battery_bar(&mut canvas, 5, 5, 0, 0, Some(50.0));
        draw_battery_bar(&mut canvas, 5, 5, 1, 1, Some(50.0));
    }

    #[test]
    fn render_full_state_writes_pixels() {
        let mut canvas = fresh_canvas(400, 200);
        let sensors = LocalSensors {
            indoor_temp: Some(21.0),
            indoor_hum: Some(45.0),
            battery_pct: Some(73.0),
        };
        render(&mut canvas, &sensors, Rect::new(0, 0, 400, 200));
        assert!(dark_count(&canvas) > 0);
    }

    #[test]
    fn render_handles_all_unknowns() {
        let mut canvas = fresh_canvas(400, 200);
        let sensors = LocalSensors {
            indoor_temp: None,
            indoor_hum: None,
            battery_pct: None,
        };
        render(&mut canvas, &sensors, Rect::new(0, 0, 400, 200));
        // Title + three em-dashes + battery outline -> still some pixels.
        assert!(dark_count(&canvas) > 0);
    }

    #[test]
    fn render_higher_pct_fills_more_pixels_than_lower() {
        let mut canvas_lo = fresh_canvas(400, 200);
        let mut canvas_hi = fresh_canvas(400, 200);
        let sensors_lo = LocalSensors {
            indoor_temp: Some(21.0),
            indoor_hum: Some(45.0),
            battery_pct: Some(10.0),
        };
        let sensors_hi = LocalSensors {
            indoor_temp: Some(21.0),
            indoor_hum: Some(45.0),
            battery_pct: Some(90.0),
        };
        render(&mut canvas_lo, &sensors_lo, Rect::new(0, 0, 400, 200));
        render(&mut canvas_hi, &sensors_hi, Rect::new(0, 0, 400, 200));
        assert!(
            dark_count(&canvas_hi) > dark_count(&canvas_lo),
            "higher battery_pct should fill more bar pixels"
        );
    }
}
