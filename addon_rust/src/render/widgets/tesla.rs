//! Finn McCool widget — Tesla status.
//!
//! Title line shows an A/C indicator (small snowflake) when the car's
//! climate system is on (and only when we know — hidden when the car
//! is asleep).
//!
//! Two stacked lines under the title:
//!   - Battery percentage  ·  range in km
//!   - Cabin (inside) temperature
//!
//! Each value shows an em-dash when unavailable.
//!
//! Mirrors `addon/app/render/widgets/tesla.py`.

use image::{GrayImage, Luma};
use imageproc::drawing::{draw_filled_rect_mut, draw_line_segment_mut};

use crate::render::fonts::{draw_text, font_for, text_width, Weight, TITLE_SIZE};
use crate::render::widgets::Rect;
use crate::sources::tesla::TeslaState;

/// Widget label — kept as a constant so smoke tests can reference it.
pub const NAME: &str = "Finn McCool";

/// Tiny 6-arm snowflake centred on `(cx, cy)` with arm length `r`.
fn draw_snowflake(canvas: &mut GrayImage, cx: i32, cy: i32, r: i32) {
    use std::f32::consts::PI;
    for i in 0..6 {
        let angle = (i as f32) * (PI / 3.0);
        let x2 = cx as f32 + (r as f32) * angle.cos();
        let y2 = cy as f32 + (r as f32) * angle.sin();
        draw_line_segment_mut(canvas, (cx as f32, cy as f32), (x2, y2), Luma([0]));
    }
    // Small dot in centre for crispness on ePaper.
    let centre = imageproc::rect::Rect::at(cx - 1, cy - 1).of_size(3, 3);
    draw_filled_rect_mut(canvas, centre, Luma([0]));
}

/// Render the Tesla widget into `rect`.
pub fn render(canvas: &mut GrayImage, state: &TeslaState, rect: Rect) {
    let title_f = font_for(Weight::Bold);
    let title_x = rect.x + 8;
    let title_y = rect.y + 4;
    draw_text(canvas, title_x, title_y, NAME, title_f, TITLE_SIZE, 0);

    // A/C indicator: only render when we know climate is on.
    // `None` (asleep) and `Some(false)` both render nothing.
    if state.climate_on == Some(true) {
        let title_w = text_width(title_f, TITLE_SIZE, NAME).round() as i32;
        let cx = title_x + title_w + 14;
        let cy = title_y + 12;
        draw_snowflake(canvas, cx, cy, 8);
    }

    let line_f = font_for(Weight::Bold);
    let line_size: f32 = 18.0;
    let line_x = rect.x + 10;
    let line_y = rect.y + 30;
    let line_gap: i32 = 22;

    let pct_text = match state.battery_pct {
        None => "\u{2014}".to_string(),
        Some(v) => format!("{}%", v.round() as i64),
    };
    let range_text = match state.range_km {
        None => "\u{2014}".to_string(),
        Some(v) => format!("{} km", v.round() as i64),
    };
    let temp_text = match state.inside_temp_c {
        None => "\u{2014}".to_string(),
        Some(v) => format!("{}\u{00b0}C cabin", v.round() as i64),
    };

    // U+00B7 MIDDLE DOT — same separator the Python addon uses.
    let line1 = format!("{pct_text}  \u{00b7}  {range_text}");
    draw_text(canvas, line_x, line_y, &line1, line_f, line_size, 0);
    draw_text(
        canvas,
        line_x,
        line_y + line_gap,
        &temp_text,
        line_f,
        line_size,
        0,
    );
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

    fn state_full() -> TeslaState {
        TeslaState {
            battery_pct: Some(73.0),
            range_km: Some(309.0),
            inside_temp_c: Some(21.0),
            climate_on: Some(false),
        }
    }

    #[test]
    fn name_is_finn_mccool() {
        // Smoke check that future renames stay deliberate.
        assert_eq!(NAME, "Finn McCool");
    }

    #[test]
    fn render_full_state_writes_pixels() {
        let mut canvas = fresh_canvas(400, 80);
        render(&mut canvas, &state_full(), Rect::new(0, 0, 400, 80));
        assert!(dark_count(&canvas) > 0);
    }

    #[test]
    fn render_handles_all_unknowns() {
        let mut canvas = fresh_canvas(400, 80);
        let state = TeslaState {
            battery_pct: None,
            range_km: None,
            inside_temp_c: None,
            climate_on: None,
        };
        render(&mut canvas, &state, Rect::new(0, 0, 400, 80));
        // Three em-dashes + title -> still some dark pixels.
        assert!(dark_count(&canvas) > 0);
    }

    #[test]
    fn snowflake_only_drawn_when_climate_on() {
        // climate_on = Some(true) should add pixels somewhere on the canvas
        // beyond what the climate_on=Some(false) baseline draws. Compare
        // whole-canvas dark counts so the test stays correct even if the
        // title font's exact advance widths shift between Inter releases.
        let mut canvas_on = fresh_canvas(400, 80);
        let mut state_on = state_full();
        state_on.climate_on = Some(true);
        render(&mut canvas_on, &state_on, Rect::new(0, 0, 400, 80));

        let mut canvas_off = fresh_canvas(400, 80);
        let mut state_off = state_full();
        state_off.climate_on = Some(false);
        render(&mut canvas_off, &state_off, Rect::new(0, 0, 400, 80));

        let count_on = dark_count(&canvas_on);
        let count_off = dark_count(&canvas_off);
        assert!(
            count_on > count_off,
            "climate_on=true should add snowflake pixels; on={count_on}, off={count_off}"
        );
    }

    #[test]
    fn snowflake_hidden_when_climate_unknown() {
        // climate_on = None (car asleep) should render exactly the same
        // pixel set as Some(false) — both are "no indicator" cases.
        let mut canvas_none = fresh_canvas(400, 80);
        let mut state_none = state_full();
        state_none.climate_on = None;
        render(&mut canvas_none, &state_none, Rect::new(0, 0, 400, 80));

        let mut canvas_off = fresh_canvas(400, 80);
        let mut state_off = state_full();
        state_off.climate_on = Some(false);
        render(&mut canvas_off, &state_off, Rect::new(0, 0, 400, 80));

        assert_eq!(dark_count(&canvas_none), dark_count(&canvas_off));
    }
}
