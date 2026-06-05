//! Page 4 — Energy + indoor environment.
//!
//! Three top-to-bottom sections:
//!
//! 1. **Phase table** — 4 rows × (label + L1 + L2 + L3 + 24h sparkline):
//!    Power Produced (kW), Power Consumed (kW), Voltage (V), Current (A).
//!    The sparkline column shows the L1 trace only (matches the
//!    original spec); per-phase numbers still appear as text.
//! 2. **Indoor row** — Temperature + Humidity, big number plus 24h sparkline.
//! 3. **Totals row** — cumulative Tariff 1, Tariff 2, Gas counters.
//!
//! All data comes from a single [`energy_src::fetch`] call. No widgets
//! are reused; the layout is dense enough that bespoke drawing is
//! clearer than parameterising existing widgets to handle a 4-column
//! table.
//!
//! Mirrors `addon/app/render/pages/04_energy.py`.

use image::{GrayImage, Luma};
use imageproc::drawing::draw_line_segment_mut;
use tracing::info;

use crate::config::Settings;
use crate::render::badge::draw_version_badge;
use crate::render::blank_canvas;
use crate::render::fonts::{draw_crisp_text, draw_text, font_for, text_height, text_width, Weight};
use crate::render::pages::RenderFuture;
use crate::render::sparkline::{draw_sparkline, SparklineOptions};
use crate::render::widgets::Rect;
use crate::sources::energy::{self as energy_src, EnergyState, PhaseValues};
use crate::sources::local_sensors::LocalSensors;

// === Layout constants ====================================================
// Vertical sums must fit within `settings.height` (480 on the E1001).
// Total budget: 12 + 28 + 232 + 10 + 1 + 10 + 102 + 10 + 1 + 10 + 64 ≈ 480,
// with the totals section dropping to ~46px in practice (small label +
// one big value), leaving ~6 px breathing room above the bottom edge.
const SIDE_INSET: i32 = 20;
const TOP_INSET: i32 = 12;

// Page header
const PAGE_TITLE_H: i32 = 28;

// Phase table
const TABLE_HEADER_H: i32 = 22;
const TABLE_ROW_H: i32 = 50;
const TABLE_LABEL_W: i32 = 100;
const TABLE_PHASE_W: i32 = 84; // each of L1, L2, L3
const SPARK_INSET: i32 = 6; // padding around sparkline within its cell

// Indoor section
const INDOOR_HEADER_H: i32 = 22;
const INDOOR_ROW_H: i32 = 38;
const INDOOR_LABEL_W: i32 = 110;
const INDOOR_VALUE_W: i32 = 130;

// Totals section
const TOTALS_HEADER_H: i32 = 22;
const TOTALS_BODY_H: i32 = 42;

// Vertical gap between sections (top + horizontal rule + bottom).
const SECTION_GAP: i32 = 10;

// === Formatters ==========================================================
// Each value column is ~84 px wide at 16px bold, so each formatted
// string caps at ~7-8 characters for safety. Format choice per quantity:
//   power:   small in W (no decimals), large in kW (2 decimals)
//   voltage: 1 decimal V
//   current: 2 decimals A
//   energy:  1 decimal kWh, thousands separator
//   gas:     3 decimals m³ (smart meters report mm³ resolution)

const EM_DASH: &str = "\u{2014}";

/// Format `n` with comma-separated thousands and `decimals` digits
/// after the point. Rust's standard library has no built-in
/// thousands-separator format, so we bolt one on by hand.
///
/// Negative numbers keep their leading minus and the separator is
/// inserted only into the integer portion.
fn fmt_thousands(n: f64, decimals: usize) -> String {
    let formatted = format!("{:.*}", decimals, n);
    let (sign, rest) = if let Some(stripped) = formatted.strip_prefix('-') {
        ("-", stripped)
    } else {
        ("", formatted.as_str())
    };
    let (int_part, frac_part) = match rest.find('.') {
        Some(i) => (&rest[..i], &rest[i..]), // include the leading '.'
        None => (rest, ""),
    };
    // Insert a comma every 3 chars from the right of the integer part.
    let mut grouped = String::with_capacity(int_part.len() + int_part.len() / 3);
    for (i, ch) in int_part.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    let int_with_commas: String = grouped.chars().rev().collect();
    format!("{sign}{int_with_commas}{frac_part}")
}

/// kW input → compact "W" or "kW" string. Break point is 10 kW —
/// residential phases very rarely exceed this even when EV-charging
/// on three phases, so the W-form is the dominant case in the wild.
fn fmt_power(v: Option<f64>) -> String {
    match v {
        None => EM_DASH.to_string(),
        Some(kw) => {
            let watts = kw * 1000.0;
            if watts.abs() >= 10_000.0 {
                format!("{kw:.2} kW")
            } else {
                format!("{} W", fmt_thousands(watts, 0))
            }
        }
    }
}

fn fmt_voltage(v: Option<f64>) -> String {
    match v {
        None => EM_DASH.to_string(),
        Some(x) => format!("{x:.1} V"),
    }
}

fn fmt_current(v: Option<f64>) -> String {
    match v {
        None => EM_DASH.to_string(),
        Some(x) => format!("{x:.2} A"),
    }
}

fn fmt_kwh(v: Option<f64>) -> String {
    match v {
        None => EM_DASH.to_string(),
        Some(x) => format!("{} kWh", fmt_thousands(x, 1)),
    }
}

fn fmt_gas(v: Option<f64>) -> String {
    match v {
        None => EM_DASH.to_string(),
        Some(x) => format!("{} m\u{00B3}", fmt_thousands(x, 3)),
    }
}

fn fmt_temp(v: Option<f64>) -> String {
    match v {
        None => EM_DASH.to_string(),
        Some(x) => format!("{x:.1} \u{00B0}C"),
    }
}

fn fmt_pct(v: Option<f64>) -> String {
    match v {
        None => EM_DASH.to_string(),
        Some(x) => format!("{} %", x.round() as i64),
    }
}

// === Section drawers =====================================================

/// Big bold "Energy" title at the top of the page.
fn draw_page_title(canvas: &mut GrayImage, x: i32, y: i32) {
    let f = font_for(Weight::Bold);
    draw_text(canvas, x, y, "Energy", f, 22.0, 0);
}

/// Draw the 4-row × (label + L1 + L2 + L3 + sparkline) table.
/// Returns the y-coordinate just below the bottom border.
fn draw_phase_table(canvas: &mut GrayImage, state: &EnergyState, x: i32, y: i32, w: i32) -> i32 {
    let label_x = x;
    let l1_x = x + TABLE_LABEL_W;
    let l2_x = l1_x + TABLE_PHASE_W;
    let l3_x = l2_x + TABLE_PHASE_W;
    let graph_x = l3_x + TABLE_PHASE_W;
    let graph_w = (w - (graph_x - x)).max(0);

    // === Header row =====================================================
    let head_f = font_for(Weight::Bold);
    let head_size: f32 = 13.0;
    let head_baseline = y + (TABLE_HEADER_H - head_size as i32) / 2;

    for (col_x, col_w, label) in [
        (l1_x, TABLE_PHASE_W, "L1"),
        (l2_x, TABLE_PHASE_W, "L2"),
        (l3_x, TABLE_PHASE_W, "L3"),
    ] {
        let tw = text_width(head_f, head_size, label).ceil() as i32;
        draw_crisp_text(
            canvas,
            col_x + (col_w - tw) / 2,
            head_baseline,
            label,
            head_f,
            head_size,
            0,
        );
    }

    let graph_label = "Last 24 h";
    let tw = text_width(head_f, head_size, graph_label).ceil() as i32;
    draw_crisp_text(
        canvas,
        graph_x + (graph_w - tw) / 2,
        head_baseline,
        graph_label,
        head_f,
        head_size,
        0,
    );

    // Header underline.
    draw_line_segment_mut(
        canvas,
        (x as f32, (y + TABLE_HEADER_H - 1) as f32),
        ((x + w - 1) as f32, (y + TABLE_HEADER_H - 1) as f32),
        Luma([0]),
    );

    // === Data rows ======================================================
    // (label, phase-values, history slice, include_zero, formatter fn).
    // `include_zero` is false only for voltage — it sits around 230 V
    // and a 0-anchored axis would flatten the line into a thin smear.
    type Fmt = fn(Option<f64>) -> String;
    /// One row of the phase table: a label, its three live phase
    /// values, the L1 history slice for the sparkline, an
    /// include-zero flag, and the value formatter to apply.
    type RowSpec<'a> = (&'a str, &'a PhaseValues, &'a Vec<Option<f64>>, bool, Fmt);
    let rows: [RowSpec; 4] = [
        (
            "Produced",
            &state.power_produced,
            &state.history_power_produced_l1,
            true,
            fmt_power,
        ),
        (
            "Consumed",
            &state.power_consumed,
            &state.history_power_consumed_l1,
            true,
            fmt_power,
        ),
        (
            "Voltage",
            &state.voltage,
            &state.history_voltage_l1,
            false,
            fmt_voltage,
        ),
        (
            "Current",
            &state.current,
            &state.history_current_l1,
            true,
            fmt_current,
        ),
    ];

    let label_f = font_for(Weight::Bold);
    let label_size: f32 = 15.0;
    let value_f = font_for(Weight::Bold);
    let value_size: f32 = 16.0;

    let row_top = y + TABLE_HEADER_H;

    for (i, (label, vals, hist, include_zero, fmt)) in rows.iter().enumerate() {
        let ry = row_top + (i as i32) * TABLE_ROW_H;

        // Row label (left-aligned, vertically centred).
        let lh = text_height(label_f, label_size).ceil() as i32;
        draw_text(
            canvas,
            label_x + 6,
            ry + (TABLE_ROW_H - lh) / 2 - 1,
            label,
            label_f,
            label_size,
            0,
        );

        // L1 / L2 / L3 numeric values (centred per column).
        for (col_x, col_w, v) in [
            (l1_x, TABLE_PHASE_W, vals.l1),
            (l2_x, TABLE_PHASE_W, vals.l2),
            (l3_x, TABLE_PHASE_W, vals.l3),
        ] {
            let text = fmt(v);
            let tw = text_width(value_f, value_size, &text).ceil() as i32;
            let th = text_height(value_f, value_size).ceil() as i32;
            draw_text(
                canvas,
                col_x + (col_w - tw) / 2,
                ry + (TABLE_ROW_H - th) / 2 - 1,
                &text,
                value_f,
                value_size,
                0,
            );
        }

        // Sparkline cell (L1 trace only). `max(1, ...)` guards against
        // the inset eating the rect's entire width/height when fonts
        // shift the layout slightly.
        let spark = Rect {
            x: graph_x + SPARK_INSET,
            y: ry + SPARK_INSET,
            w: (graph_w - 2 * SPARK_INSET).max(1),
            h: (TABLE_ROW_H - 2 * SPARK_INSET).max(1),
        };
        draw_sparkline(
            canvas,
            spark,
            hist,
            SparklineOptions {
                include_zero: *include_zero,
                ..SparklineOptions::default()
            },
        );

        // Inter-row separator (skip after the last row — the bottom
        // border below renders that line for free).
        if i < rows.len() - 1 {
            let sep_y = ry + TABLE_ROW_H;
            draw_line_segment_mut(
                canvas,
                (x as f32, sep_y as f32),
                ((x + w - 1) as f32, sep_y as f32),
                Luma([0]),
            );
        }
    }

    // === Borders ========================================================
    let table_top = y + TABLE_HEADER_H;
    let table_bot = row_top + (rows.len() as i32) * TABLE_ROW_H;

    // Bottom border closes the table; the top border is implicit
    // (the page's section separator above the table provides it).
    draw_line_segment_mut(
        canvas,
        (x as f32, table_bot as f32),
        ((x + w - 1) as f32, table_bot as f32),
        Luma([0]),
    );

    // Vertical separators between columns. Start below the header
    // underline so we don't double up on that line.
    for sep_x in [l1_x, l2_x, l3_x, graph_x] {
        draw_line_segment_mut(
            canvas,
            (sep_x as f32, table_top as f32),
            (sep_x as f32, table_bot as f32),
            Luma([0]),
        );
    }

    table_bot
}

/// Indoor temperature + humidity rows with 24h sparklines.
/// Returns the y-coordinate just below the section.
fn draw_indoor(canvas: &mut GrayImage, state: &EnergyState, x: i32, y: i32, w: i32) -> i32 {
    let head_f = font_for(Weight::Bold);
    let head_size: f32 = 15.0;
    draw_crisp_text(canvas, x, y, "Indoor", head_f, head_size, 0);
    let body_top = y + INDOOR_HEADER_H;

    let rows: [(&str, String, &Vec<Option<f64>>); 2] = [
        (
            "Temperature",
            fmt_temp(state.indoor_temp),
            &state.history_indoor_temp,
        ),
        (
            "Humidity",
            fmt_pct(state.indoor_humidity),
            &state.history_indoor_humidity,
        ),
    ];

    let label_f = font_for(Weight::Regular);
    let label_size: f32 = 13.0;
    let value_f = font_for(Weight::Bold);
    let value_size: f32 = 20.0;

    let graph_x = x + INDOOR_LABEL_W + INDOOR_VALUE_W + 12;
    let graph_w = (w - (graph_x - x)).max(1);

    for (i, (label, value, hist)) in rows.iter().enumerate() {
        let ry = body_top + (i as i32) * INDOOR_ROW_H;

        // Label (left).
        let lh = text_height(label_f, label_size).ceil() as i32;
        draw_crisp_text(
            canvas,
            x + 4,
            ry + (INDOOR_ROW_H - lh) / 2,
            label,
            label_f,
            label_size,
            0,
        );

        // Value (right-aligned within its column for tabular feel).
        let vw = text_width(value_f, value_size, value).ceil() as i32;
        let vh = text_height(value_f, value_size).ceil() as i32;
        draw_text(
            canvas,
            x + INDOOR_LABEL_W + INDOOR_VALUE_W - vw - 4,
            ry + (INDOOR_ROW_H - vh) / 2 - 1,
            value,
            value_f,
            value_size,
            0,
        );

        // Sparkline (auto-scale; indoor swings are small so a 0-anchor
        // would flatten the trace into a thin smear at the top).
        let spark = Rect {
            x: graph_x,
            y: ry + 4,
            w: graph_w,
            h: INDOOR_ROW_H - 8,
        };
        draw_sparkline(
            canvas,
            spark,
            hist,
            SparklineOptions {
                include_zero: false,
                ..SparklineOptions::default()
            },
        );
    }

    body_top + (rows.len() as i32) * INDOOR_ROW_H
}

/// T1 / T2 / Gas counters in three centred columns.
/// Returns the y-coordinate just below the section.
fn draw_totals(canvas: &mut GrayImage, state: &EnergyState, x: i32, y: i32, w: i32) -> i32 {
    let head_f = font_for(Weight::Bold);
    let head_size: f32 = 15.0;
    draw_crisp_text(canvas, x, y, "Totals", head_f, head_size, 0);
    let body_top = y + TOTALS_HEADER_H;

    let cols: [(&str, String); 3] = [
        ("Tariff 1 (low)", fmt_kwh(state.energy_tariff1)),
        ("Tariff 2 (high)", fmt_kwh(state.energy_tariff2)),
        ("Gas", fmt_gas(state.gas)),
    ];

    let n = cols.len() as i32;
    let col_w = w / n;
    let label_f = font_for(Weight::Regular);
    let label_size: f32 = 12.0;
    let value_f = font_for(Weight::Bold);
    let value_size: f32 = 20.0;

    for (i, (label, value)) in cols.iter().enumerate() {
        let cx = x + (i as i32) * col_w;

        // Label centred above value.
        let lw = text_width(label_f, label_size, label).ceil() as i32;
        draw_crisp_text(
            canvas,
            cx + (col_w - lw) / 2,
            body_top,
            label,
            label_f,
            label_size,
            0,
        );

        // Big value below.
        let vw = text_width(value_f, value_size, value).ceil() as i32;
        draw_text(
            canvas,
            cx + (col_w - vw) / 2,
            body_top + 14,
            value,
            value_f,
            value_size,
            0,
        );
    }

    body_top + TOTALS_BODY_H
}

/// Compose the energy page. Returns an 8-bit greyscale image that the
/// rest of the pipeline thresholds to 1-bit at output time.
pub async fn render(
    settings: Settings,
    _sensors: LocalSensors,
    fw_version: Option<String>,
) -> GrayImage {
    let w = settings.width as i32;
    let h = settings.height as i32;
    let mut img = blank_canvas(settings.width, settings.height);

    let state = energy_src::fetch(&settings).await;
    info!(
        produced_l1 = ?state.power_produced.l1,
        consumed_l1 = ?state.power_consumed.l1,
        history_pp_len = state.history_power_produced_l1.len(),
        "energy page: rendering"
    );

    let inset_x = SIDE_INSET;
    let avail_w = w - 2 * SIDE_INSET;

    let mut cy = TOP_INSET;

    // Page title.
    draw_page_title(&mut img, inset_x, cy);
    cy += PAGE_TITLE_H;

    // Section 1: phase table.
    cy = draw_phase_table(&mut img, &state, inset_x, cy, avail_w);
    cy += SECTION_GAP;

    // Horizontal rule between sections — keeps the dense page from
    // feeling like one giant blob of text.
    draw_line_segment_mut(
        &mut img,
        (inset_x as f32, cy as f32),
        ((inset_x + avail_w) as f32, cy as f32),
        Luma([0]),
    );
    cy += SECTION_GAP;

    // Section 2: indoor temp + humidity.
    cy = draw_indoor(&mut img, &state, inset_x, cy, avail_w);
    cy += SECTION_GAP;

    draw_line_segment_mut(
        &mut img,
        (inset_x as f32, cy as f32),
        ((inset_x + avail_w) as f32, cy as f32),
        Luma([0]),
    );
    cy += SECTION_GAP;

    // Section 3: cumulative totals.
    draw_totals(&mut img, &state, inset_x, cy, avail_w);

    // The final `_ = h` prevents an unused-variable warning if a
    // future refactor stops referencing `h` in the section flow.
    let _ = h;

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
    fn fmt_thousands_inserts_separators_and_handles_decimals() {
        assert_eq!(fmt_thousands(1234.0, 0), "1,234");
        assert_eq!(fmt_thousands(1234.5, 1), "1,234.5");
        assert_eq!(fmt_thousands(1_234_567.891, 3), "1,234,567.891");
        assert_eq!(fmt_thousands(0.0, 0), "0");
        assert_eq!(fmt_thousands(-1234.5, 1), "-1,234.5");
        assert_eq!(fmt_thousands(99.0, 0), "99");
    }

    #[test]
    fn fmt_power_uses_watts_below_10kw() {
        assert_eq!(fmt_power(Some(0.123)), "123 W");
        assert_eq!(fmt_power(Some(1.234)), "1,234 W");
        assert_eq!(fmt_power(Some(9.999)), "9,999 W");
    }

    #[test]
    fn fmt_power_uses_kw_at_or_above_10kw() {
        assert_eq!(fmt_power(Some(10.0)), "10.00 kW");
        assert_eq!(fmt_power(Some(15.123)), "15.12 kW");
    }

    #[test]
    fn fmt_power_handles_negative_for_export() {
        // Net export from solar shows up as a negative power_consumed.
        assert_eq!(fmt_power(Some(-2.5)), "-2,500 W");
        assert_eq!(fmt_power(Some(-12.5)), "-12.50 kW");
    }

    #[test]
    fn fmt_voltage_renders_one_decimal() {
        assert_eq!(fmt_voltage(Some(229.7)), "229.7 V");
        assert_eq!(fmt_voltage(None), "\u{2014}");
    }

    #[test]
    fn fmt_current_renders_two_decimals() {
        assert_eq!(fmt_current(Some(2.345)), "2.35 A");
        assert_eq!(fmt_current(None), "\u{2014}");
    }

    #[test]
    fn fmt_kwh_renders_one_decimal_with_separator() {
        assert_eq!(fmt_kwh(Some(12_345.678)), "12,345.7 kWh");
        assert_eq!(fmt_kwh(None), "\u{2014}");
    }

    #[test]
    fn fmt_gas_renders_three_decimals() {
        assert_eq!(fmt_gas(Some(1234.5)), "1,234.500 m\u{00B3}");
        assert_eq!(fmt_gas(None), "\u{2014}");
    }

    #[test]
    fn fmt_temp_uses_one_decimal_with_unit() {
        // 21.5 is exactly representable; 21.46 rounds up. Avoid 21.45
        // — it actually stores as 21.4499... so `{:.1}` truncates to
        // "21.4" rather than the naive "21.5" you'd expect.
        assert_eq!(fmt_temp(Some(21.5)), "21.5 \u{00B0}C");
        assert_eq!(fmt_temp(Some(21.46)), "21.5 \u{00B0}C");
        assert_eq!(fmt_temp(None), "\u{2014}");
    }

    #[test]
    fn fmt_pct_rounds_and_appends_unit() {
        assert_eq!(fmt_pct(Some(55.4)), "55 %");
        assert_eq!(fmt_pct(Some(55.6)), "56 %");
        assert_eq!(fmt_pct(None), "\u{2014}");
    }

    #[tokio::test]
    async fn render_returns_canvas_of_settings_dimensions() {
        let s = Settings::default();
        let img = render(s.clone(), LocalSensors::default(), None).await;
        assert_eq!(img.width(), s.width);
        assert_eq!(img.height(), s.height);
    }

    #[tokio::test]
    async fn render_writes_some_dark_pixels() {
        let img = render(Settings::default(), LocalSensors::default(), None).await;
        let dark = img.pixels().filter(|p| p[0] < 200).count();
        assert!(dark > 0, "energy render produced no dark pixels");
    }
}
