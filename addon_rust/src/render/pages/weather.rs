//! Page 2 — full-screen weather.
//!
//! Three-row layout (mirrors `addon/app/render/pages/02_weather.py`):
//!
//! ```text
//!   ┌──────────────────────────────────────────────────────────────────┐
//!   │ [ICON] Partly Cloudy │   18.5 °C       │ [g]  Pressure  1014 hPa │
//!   │        7 hours ago   │ max 19 / min 14 │ [d]  Humidity      66 % │
//!   │                      │                 │ [w]  Wind 13 km/h (WNW) │
//!   ├──────────────────────────────────────────────────────────────────┤
//!   │ Hourly forecast                                                  │
//!   │ Sun                Mon                                           │
//!   │ 21:00 22:00 23:00 0:00 1:00 2:00 3:00 4:00                       │
//!   │ [ic]  [ic]  [ic]  [ic] [ic] [ic] [ic] [ic]                       │
//!   │ 17.6° 16.8° 16.2° 15.9° 15.5° 15.1° 14.7° 14.3°                  │
//!   ├──────────────────────────────────────────────────────────────────┤
//!   │ Weekly forecast                                                  │
//!   │ Today  Tue  Wed  Thu  Fri  Sat  Sun                              │
//!   │ [ic]   [ic] [ic] [ic] [ic] [ic] [ic]                             │
//!   │ 19°    21°  20°  18°  17°  16°  15°                              │
//!   │ 12°    13°  14°  11°  10°   9°   8°                              │
//!   └──────────────────────────────────────────────────────────────────┘
//! ```
//!
//! All data comes from a single [`weather_src::fetch`] call. The hero
//! row's stats column tolerates missing attributes by rendering an
//! em-dash so partial data still produces a sensible layout.

use ab_glyph::FontRef;
use chrono::{DateTime, Datelike, NaiveDate, Timelike, Utc};
use chrono_tz::Tz;
use image::{GrayImage, Luma};
use imageproc::drawing::draw_line_segment_mut;
use tracing::info;

use crate::config::Settings;
use crate::render::badge::draw_version_badge;
use crate::render::blank_canvas;
use crate::render::fonts::{draw_crisp_text, draw_text, font_for, text_height, text_width, Weight};
use crate::render::icons::{draw_icon, icon_for_weather_state};
use crate::render::pages::RenderFuture;
use crate::sources::local_sensors::LocalSensors;
use crate::sources::weather::{self as weather_src, bearing_to_cardinal, ForecastEntry, Weather};

// === Layout tunables (pixels) ============================================
const SIDE_INSET: i32 = 24;
const TOP_INSET: i32 = 14;
const HERO_HEIGHT: i32 = 140; // row 1: hero + stats merged
const HOURLY_HEIGHT: i32 = 160; // row 2: hourly forecast strip
                                // Row 3 (weekly) takes whatever vertical space remains.
const SECTION_GAP: i32 = 10; // gap above/below the separator rule between rows
const FORECAST_LABEL_HEIGHT: i32 = 22; // heading + breathing room
const HOURLY_COL_MIN: i32 = 86; // min width per hourly column
const WEEKLY_COL_MIN: i32 = 88; // min width per weekly column
const STATS_ROW_GAP: i32 = 4;

// Hero row column splits (fractions of the available width).
const HERO_LEFT_FRAC: f32 = 0.34;
const HERO_MID_FRAC: f32 = 0.30;
// Right column gets the remainder (≈ 0.36).

/// Resolve `settings.timezone` to a chrono-tz zone, defaulting to UTC.
fn timezone(settings: &Settings) -> Tz {
    settings.timezone.parse::<Tz>().unwrap_or(chrono_tz::UTC)
}

/// Format a temperature with one decimal place, em-dash for `None`.
/// Pass `unit` like `" °C"` or `"°"` — it's appended verbatim.
fn fmt_temp(t: Option<f64>, unit: &str) -> String {
    match t {
        None => "\u{2014}".to_string(),
        Some(v) => format!("{v:.1}{unit}"),
    }
}

/// Format a temperature with no decimal places (rounded). Em-dash for `None`.
fn fmt_temp_int(t: Option<f64>, unit: &str) -> String {
    match t {
        None => "\u{2014}".to_string(),
        Some(v) => format!("{}{unit}", v.round() as i64),
    }
}

/// Render a "N minutes/hours/days ago" string.
///
/// Returns `None` when no timestamp is available. Negative deltas
/// (clock skew) collapse to "just now" so we never show
/// "in 3 minutes", which would be confusing to a user.
fn humanise_age(then: Option<DateTime<Utc>>, now: DateTime<Utc>) -> Option<String> {
    let then = then?;
    let seconds = (now - then).num_seconds();
    if seconds < 60 {
        return Some("just now".to_string());
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        let s = if minutes == 1 { "" } else { "s" };
        return Some(format!("{minutes} minute{s} ago"));
    }
    let hours = minutes / 60;
    if hours < 24 {
        let s = if hours == 1 { "" } else { "s" };
        return Some(format!("{hours} hour{s} ago"));
    }
    let days = hours / 24;
    let s = if days == 1 { "" } else { "s" };
    Some(format!("{days} day{s} ago"))
}

/// Title-case a condition string with `_` / `-` collapsed to spaces.
/// `"partly_cloudy"` → `"Partly Cloudy"`. Mirrors the same helper in
/// the small dashboard widget.
fn pretty_condition(condition: &str) -> String {
    let cleaned: String = condition
        .chars()
        .map(|c| if c == '_' || c == '-' { ' ' } else { c })
        .collect();
    let mut out = String::with_capacity(cleaned.len());
    let mut new_word = true;
    for ch in cleaned.chars() {
        if ch.is_whitespace() {
            new_word = true;
            out.push(ch);
        } else if new_word {
            for u in ch.to_uppercase() {
                out.push(u);
            }
            new_word = false;
        } else {
            for u in ch.to_lowercase() {
                out.push(u);
            }
        }
    }
    out
}

/// Truncate `text` to fit within `max_w` pixels at the given font + size,
/// appending an ellipsis when truncation happens. UTF-8 safe.
fn truncate_to_width(font: &FontRef<'static>, size: f32, text: &str, max_w: i32) -> String {
    if max_w <= 0 {
        return String::new();
    }
    if text_width(font, size, text).ceil() as i32 <= max_w {
        return text.to_string();
    }
    let mut chars: Vec<char> = text.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let candidate: String = chars.iter().collect::<String>() + "\u{2026}";
        if text_width(font, size, &candidate).ceil() as i32 <= max_w {
            return candidate;
        }
    }
    "\u{2026}".to_string()
}

/// Left column of the hero row: weather icon + condition label + age.
fn draw_hero_left(
    canvas: &mut GrayImage,
    weather: &Weather,
    settings: &Settings,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
) {
    let icon_size = (h as f32 * 0.62) as i32;
    let icon_x = x;
    let icon_y = y + (h - icon_size) / 2;
    let icon_name = weather
        .icon
        .as_deref()
        .unwrap_or_else(|| icon_for_weather_state(&weather.condition));
    draw_icon(canvas, icon_name, icon_x, icon_y, icon_size as f32, 0);

    let cond_label_full = pretty_condition(&weather.condition);
    let cond_f = font_for(Weight::Bold);
    let cond_size: f32 = 26.0;
    let cond_h = text_height(cond_f, cond_size).ceil() as i32;

    let now = Utc::now();
    let age_text = humanise_age(weather.last_updated, now);
    let age_f = font_for(Weight::Regular);
    let age_size: f32 = 14.0;
    let age_h = text_height(age_f, age_size).ceil() as i32;

    let block_h = if age_text.is_some() {
        cond_h + 6 + age_h
    } else {
        cond_h
    };
    let block_top = y + (h - block_h) / 2;
    let text_x = icon_x + icon_size + 12;

    // Truncate condition so a long label can't bleed into the centre column.
    let avail = (x + w - text_x).max(0);
    let cond_label = truncate_to_width(cond_f, cond_size, &cond_label_full, avail);
    draw_text(canvas, text_x, block_top, &cond_label, cond_f, cond_size, 0);

    if let Some(age) = age_text {
        draw_crisp_text(
            canvas,
            text_x,
            block_top + cond_h + 6,
            &age,
            age_f,
            age_size,
            0,
        );
    }

    // `settings` is borrowed only to keep the function signature
    // stable if we later need locale-aware formatting (matches the
    // Python helper's signature).
    let _ = settings;
}

/// Middle column of the hero row: big current temperature + hi/lo line.
fn draw_hero_middle(canvas: &mut GrayImage, weather: &Weather, x: i32, y: i32, w: i32, h: i32) {
    let temp_text = fmt_temp(
        weather.temperature,
        &format!(" {}", weather.temperature_unit),
    );
    let temp_f = font_for(Weight::Regular);
    let temp_size: f32 = 48.0;
    let temp_w = text_width(temp_f, temp_size, &temp_text).ceil() as i32;
    let temp_h = text_height(temp_f, temp_size).ceil() as i32;

    // Today's hi/lo comes from the first daily forecast entry, if present.
    let today_fc: Option<&ForecastEntry> = weather.forecast.first();
    let hilo_text = match today_fc {
        Some(fc) if fc.temp_high.is_some() && fc.temp_low.is_some() => {
            let unit = &weather.temperature_unit;
            // Two-space dividers around `/` to add visual breathing room.
            format!(
                "max {:.1} {unit}  /  min {:.1} {unit}",
                fc.temp_high.unwrap(),
                fc.temp_low.unwrap(),
            )
        }
        _ => String::new(),
    };

    let hilo_f = font_for(Weight::Regular);
    let hilo_size: f32 = 16.0;
    let (hilo_w, hilo_h) = if hilo_text.is_empty() {
        (0, 0)
    } else {
        (
            text_width(hilo_f, hilo_size, &hilo_text).ceil() as i32,
            text_height(hilo_f, hilo_size).ceil() as i32,
        )
    };

    let temp_hilo_gap = 18;
    let block_h = if hilo_text.is_empty() {
        temp_h
    } else {
        temp_h + temp_hilo_gap + hilo_h
    };
    let block_top = y + (h - block_h) / 2;

    // Centre-align both lines within the middle column for visual balance.
    let temp_x = x + (w - temp_w) / 2;
    draw_text(canvas, temp_x, block_top, &temp_text, temp_f, temp_size, 0);
    if !hilo_text.is_empty() {
        let hilo_x = x + (w - hilo_w) / 2;
        draw_crisp_text(
            canvas,
            hilo_x,
            block_top + temp_h + temp_hilo_gap,
            &hilo_text,
            hilo_f,
            hilo_size,
            0,
        );
    }
}

/// Right column of the hero row: three stacked stat rows.
///
/// Each row is `[icon] label .................. value`. The value is
/// right-aligned to the column edge so the eye can scan downward
/// without the numbers jumping around horizontally.
fn draw_hero_stats(canvas: &mut GrayImage, weather: &Weather, x: i32, y: i32, w: i32, h: i32) {
    let cardinal = bearing_to_cardinal(weather.wind_bearing);
    let wind_value = match (weather.wind_speed, cardinal) {
        (None, _) => "\u{2014}".to_string(),
        (Some(v), Some(c)) => format!("{} {} ({c})", v.round() as i64, weather.wind_unit),
        (Some(v), None) => format!("{} {}", v.round() as i64, weather.wind_unit),
    };
    let pressure_value = match weather.pressure {
        Some(p) => format!("{:.0} {}", p, weather.pressure_unit),
        None => "\u{2014}".to_string(),
    };
    let humidity_value = match weather.humidity {
        Some(p) => format!("{} %", p.round() as i64),
        None => "\u{2014}".to_string(),
    };

    let rows: [(&str, &str, &str); 3] = [
        ("gauge", "Pressure", &pressure_value),
        ("water-percent", "Humidity", &humidity_value),
        ("weather-windy", "Wind", &wind_value),
    ];

    let n = rows.len() as i32;
    let row_h = (h - STATS_ROW_GAP * (n - 1)) / n;
    let icon_size = (row_h - 6).min(26);
    let label_f = font_for(Weight::Regular);
    let label_size: f32 = 14.0;
    let value_f = font_for(Weight::Bold);
    let value_size: f32 = 15.0;

    for (i, (icon_name, label, value)) in rows.iter().enumerate() {
        let row_y = y + i as i32 * (row_h + STATS_ROW_GAP);
        let icon_y = row_y + (row_h - icon_size) / 2;
        draw_icon(canvas, icon_name, x, icon_y, icon_size as f32, 0);

        let label_h = text_height(label_f, label_size).ceil() as i32;
        let label_x = x + icon_size + 8;
        let label_y = row_y + (row_h - label_h) / 2 - 1;
        draw_crisp_text(canvas, label_x, label_y, label, label_f, label_size, 0);

        let value_w = text_width(value_f, value_size, value).ceil() as i32;
        let value_h = text_height(value_f, value_size).ceil() as i32;
        let value_x = x + w - value_w;
        let value_y = row_y + (row_h - value_h) / 2 - 1;
        draw_text(canvas, value_x, value_y, value, value_f, value_size, 0);
    }
}

/// Compose the hero row from the three column helpers and draw thin
/// vertical rules between them. Vertical rules are inset from the
/// row's top/bottom so they read as separators, not borders.
fn draw_hero(
    canvas: &mut GrayImage,
    weather: &Weather,
    settings: &Settings,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
) {
    let left_w = (w as f32 * HERO_LEFT_FRAC) as i32;
    let mid_w = (w as f32 * HERO_MID_FRAC) as i32;
    let right_w = w - left_w - mid_w;

    let rule_pad = 8;
    let rule_x1 = (x + left_w) as f32;
    let rule_x2 = (x + left_w + mid_w) as f32;
    draw_line_segment_mut(
        canvas,
        (rule_x1, (y + rule_pad) as f32),
        (rule_x1, (y + h - rule_pad) as f32),
        Luma([0]),
    );
    draw_line_segment_mut(
        canvas,
        (rule_x2, (y + rule_pad) as f32),
        (rule_x2, (y + h - rule_pad) as f32),
        Luma([0]),
    );

    draw_hero_left(canvas, weather, settings, x, y, left_w - 6, h);
    draw_hero_middle(canvas, weather, x + left_w + 6, y, mid_w - 12, h);
    draw_hero_stats(canvas, weather, x + left_w + mid_w + 8, y, right_w - 8, h);
}

/// Short weekday name from a chrono `Weekday`. Locale-independent so
/// the firmware-side rendering matches no matter what `LANG` happens
/// to be set in the user's HA install.
fn weekday_short(day: chrono::Weekday) -> &'static str {
    match day {
        chrono::Weekday::Mon => "Mon",
        chrono::Weekday::Tue => "Tue",
        chrono::Weekday::Wed => "Wed",
        chrono::Weekday::Thu => "Thu",
        chrono::Weekday::Fri => "Fri",
        chrono::Weekday::Sat => "Sat",
        chrono::Weekday::Sun => "Sun",
    }
}

/// Hourly forecast strip: heading + columns of (day-label, hour, icon, temp).
///
/// Day labels only render on column 0 and at every day rollover, to
/// avoid stamping "Mon Mon Mon" on every column when the strip
/// straddles a day boundary.
fn draw_hourly(
    canvas: &mut GrayImage,
    weather: &Weather,
    settings: &Settings,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
) {
    let head_f = font_for(Weight::Bold);
    let head_size: f32 = 18.0;
    draw_crisp_text(canvas, x, y, "Hourly forecast", head_f, head_size, 0);
    let grid_top = y + FORECAST_LABEL_HEIGHT;

    let tz = timezone(settings);
    let now_local = Utc::now().with_timezone(&tz);
    let hour_floor = now_local
        .with_minute(0)
        .and_then(|d| d.with_second(0))
        .and_then(|d| d.with_nanosecond(0))
        .unwrap_or(now_local);

    let upcoming: Vec<(&ForecastEntry, chrono::DateTime<Tz>)> = weather
        .forecast_hourly
        .iter()
        .map(|f| (f, f.when.with_timezone(&tz)))
        .filter(|(_, when_local)| *when_local >= hour_floor)
        .collect();

    if upcoming.is_empty() {
        let mf = font_for(Weight::Regular);
        let m_size: f32 = 16.0;
        let msg = "No hourly forecast available.";
        let mw = text_width(mf, m_size, msg).ceil() as i32;
        let mh = text_height(mf, m_size).ceil() as i32;
        draw_crisp_text(
            canvas,
            x + (w - mw) / 2,
            grid_top + (h - FORECAST_LABEL_HEIGHT - mh) / 2,
            msg,
            mf,
            m_size,
            0,
        );
        return;
    }

    let max_cols = ((upcoming.len() as i32).min(w / HOURLY_COL_MIN)).max(1);
    let col_w = w / max_cols;

    let day_f = font_for(Weight::Bold);
    let day_size: f32 = 16.0;
    let hour_f = font_for(Weight::Regular);
    let hour_size: f32 = 16.0;
    let temp_f = font_for(Weight::Bold);
    let temp_size: f32 = 18.0;
    let icon_sz: i32 = 32;

    let inner_top = grid_top;
    let day_h = day_size as i32 + 2;
    let hour_h = hour_size as i32 + 2;
    let mut prev_day: Option<NaiveDate> = None;
    for (i, (f, when_local)) in upcoming.iter().take(max_cols as usize).enumerate() {
        let cx = x + i as i32 * col_w;

        let this_day = when_local.date_naive();
        if i == 0 || prev_day != Some(this_day) {
            let day_label = weekday_short(when_local.weekday());
            let dw = text_width(day_f, day_size, day_label).ceil() as i32;
            draw_crisp_text(
                canvas,
                cx + (col_w - dw) / 2,
                inner_top,
                day_label,
                day_f,
                day_size,
                0,
            );
        }
        prev_day = Some(this_day);

        let hour_label = format!("{}:{:02}", when_local.hour(), when_local.minute());
        let hw = text_width(hour_f, hour_size, &hour_label).ceil() as i32;
        let hour_y = inner_top + day_h;
        draw_crisp_text(
            canvas,
            cx + (col_w - hw) / 2,
            hour_y,
            &hour_label,
            hour_f,
            hour_size,
            0,
        );

        let icon_y = hour_y + hour_h + 2;
        let icon_x = cx + (col_w - icon_sz) / 2;
        draw_icon(
            canvas,
            icon_for_weather_state(&f.condition),
            icon_x,
            icon_y,
            icon_sz as f32,
            0,
        );

        let t_text = match f.temp_high {
            Some(_) => fmt_temp(f.temp_high, "°"),
            None => "\u{2014}".to_string(),
        };
        let tw = text_width(temp_f, temp_size, &t_text).ceil() as i32;
        let t_y = icon_y + icon_sz + 4;
        draw_crisp_text(
            canvas,
            cx + (col_w - tw) / 2,
            t_y,
            &t_text,
            temp_f,
            temp_size,
            0,
        );
    }
}

/// Weekly forecast strip: heading + 5–7 daily columns showing weekday,
/// icon, hi (rounded), lo (rounded). The first column showing today
/// is labelled "Today" instead of the weekday name to make it
/// immediately obvious which one is the current day.
fn draw_weekly(
    canvas: &mut GrayImage,
    weather: &Weather,
    settings: &Settings,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
) {
    let head_f = font_for(Weight::Bold);
    let head_size: f32 = 18.0;
    draw_crisp_text(canvas, x, y, "Weekly forecast", head_f, head_size, 0);
    let grid_top = y + FORECAST_LABEL_HEIGHT;

    let tz = timezone(settings);
    let today = Utc::now().with_timezone(&tz).date_naive();

    // Drop entries whose date is before today — some providers
    // include a "today" entry whose hi/lo reflects the already-
    // elapsed portion of the day, which is misleading after noon.
    let daily: Vec<(&ForecastEntry, chrono::DateTime<Tz>)> = weather
        .forecast
        .iter()
        .filter_map(|f| {
            let when_local = f.when.with_timezone(&tz);
            if when_local.date_naive() < today {
                None
            } else {
                Some((f, when_local))
            }
        })
        .collect();

    if daily.is_empty() {
        let mf = font_for(Weight::Regular);
        let m_size: f32 = 16.0;
        let msg = "No weekly forecast available.";
        let mw = text_width(mf, m_size, msg).ceil() as i32;
        let mh = text_height(mf, m_size).ceil() as i32;
        draw_crisp_text(
            canvas,
            x + (w - mw) / 2,
            grid_top + (h - FORECAST_LABEL_HEIGHT - mh) / 2,
            msg,
            mf,
            m_size,
            0,
        );
        return;
    }

    let max_cols = ((daily.len() as i32).min(w / WEEKLY_COL_MIN)).max(1);
    let col_w = w / max_cols;

    let day_f = font_for(Weight::Bold);
    let day_size: f32 = 16.0;
    let hi_f = font_for(Weight::Bold);
    let hi_size: f32 = 17.0;
    let lo_f = font_for(Weight::Regular);
    let lo_size: f32 = 16.0;
    let icon_sz: i32 = 28;

    let day_h = day_size as i32 + 2;
    let hi_h = hi_size as i32 + 2;
    let inner_top = grid_top;

    for (i, (f, when_local)) in daily.iter().take(max_cols as usize).enumerate() {
        let cx = x + i as i32 * col_w;

        let day_label_buf;
        let day_label: &str = if when_local.date_naive() == today {
            "Today"
        } else {
            day_label_buf = weekday_short(when_local.weekday());
            day_label_buf
        };
        let dw = text_width(day_f, day_size, day_label).ceil() as i32;
        draw_crisp_text(
            canvas,
            cx + (col_w - dw) / 2,
            inner_top,
            day_label,
            day_f,
            day_size,
            0,
        );

        let icon_y = inner_top + day_h + 2;
        let icon_x = cx + (col_w - icon_sz) / 2;
        draw_icon(
            canvas,
            icon_for_weather_state(&f.condition),
            icon_x,
            icon_y,
            icon_sz as f32,
            0,
        );

        let hi_text = fmt_temp_int(f.temp_high, "°");
        let lo_text = fmt_temp_int(f.temp_low, "°");
        let hi_w = text_width(hi_f, hi_size, &hi_text).ceil() as i32;
        let lo_w = text_width(lo_f, lo_size, &lo_text).ceil() as i32;
        let hi_y = icon_y + icon_sz + 4;
        let lo_y = hi_y + hi_h;
        draw_crisp_text(
            canvas,
            cx + (col_w - hi_w) / 2,
            hi_y,
            &hi_text,
            hi_f,
            hi_size,
            0,
        );
        draw_crisp_text(
            canvas,
            cx + (col_w - lo_w) / 2,
            lo_y,
            &lo_text,
            lo_f,
            lo_size,
            0,
        );
    }
}

/// Render the full-screen weather page.
pub async fn render(
    settings: Settings,
    _sensors: LocalSensors,
    fw_version: Option<String>,
) -> GrayImage {
    let w = settings.width as i32;
    let h = settings.height as i32;
    let mut img = blank_canvas(settings.width, settings.height);

    let weather = weather_src::fetch(&settings).await;
    info!(
        condition = %weather.condition,
        daily = weather.forecast.len(),
        hourly = weather.forecast_hourly.len(),
        "weather page: rendering"
    );

    let x = SIDE_INSET;
    let avail_w = w - 2 * SIDE_INSET;
    let mut cursor_y = TOP_INSET;

    // Row 1: hero (icon + condition / temp + hi-lo / stats).
    draw_hero(
        &mut img,
        &weather,
        &settings,
        x,
        cursor_y,
        avail_w,
        HERO_HEIGHT,
    );
    cursor_y += HERO_HEIGHT + SECTION_GAP;
    draw_line_segment_mut(
        &mut img,
        (x as f32, cursor_y as f32),
        ((x + avail_w) as f32, cursor_y as f32),
        Luma([0]),
    );
    cursor_y += SECTION_GAP;

    // Row 2: hourly forecast strip.
    draw_hourly(
        &mut img,
        &weather,
        &settings,
        x,
        cursor_y,
        avail_w,
        HOURLY_HEIGHT,
    );
    cursor_y += HOURLY_HEIGHT + SECTION_GAP;
    draw_line_segment_mut(
        &mut img,
        (x as f32, cursor_y as f32),
        ((x + avail_w) as f32, cursor_y as f32),
        Luma([0]),
    );
    cursor_y += SECTION_GAP;

    // Row 3: weekly forecast — flexes to absorb whatever vertical
    // space remains so the layout adapts to canvas-height changes.
    let weekly_h = h - cursor_y - TOP_INSET;
    draw_weekly(
        &mut img, &weather, &settings, x, cursor_y, avail_w, weekly_h,
    );

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
    use chrono::Duration;

    #[test]
    fn fmt_temp_handles_some_and_none() {
        assert_eq!(fmt_temp(Some(18.5), " °C"), "18.5 °C");
        assert_eq!(fmt_temp(None, " °C"), "\u{2014}");
    }

    #[test]
    fn fmt_temp_int_rounds_correctly() {
        assert_eq!(fmt_temp_int(Some(18.4), "°"), "18°");
        assert_eq!(fmt_temp_int(Some(18.6), "°"), "19°");
        assert_eq!(fmt_temp_int(None, "°"), "\u{2014}");
    }

    #[test]
    fn humanise_age_returns_none_for_missing_timestamp() {
        let now = Utc::now();
        assert!(humanise_age(None, now).is_none());
    }

    #[test]
    fn humanise_age_just_now_for_recent() {
        let now = Utc::now();
        assert_eq!(humanise_age(Some(now), now), Some("just now".to_string()));
    }

    #[test]
    fn humanise_age_negative_delta_collapses_to_just_now() {
        // Clock skew can put `then` slightly in the future.
        let now = Utc::now();
        let then = now + Duration::seconds(30);
        assert_eq!(humanise_age(Some(then), now), Some("just now".to_string()));
    }

    #[test]
    fn humanise_age_minutes_uses_singular_for_one() {
        let now = Utc::now();
        let then = now - Duration::seconds(60);
        assert_eq!(
            humanise_age(Some(then), now),
            Some("1 minute ago".to_string())
        );
    }

    #[test]
    fn humanise_age_hours_pluralizes() {
        let now = Utc::now();
        let then = now - Duration::hours(7);
        assert_eq!(
            humanise_age(Some(then), now),
            Some("7 hours ago".to_string())
        );
    }

    #[test]
    fn humanise_age_days_pluralizes() {
        let now = Utc::now();
        let then = now - Duration::days(2);
        assert_eq!(
            humanise_age(Some(then), now),
            Some("2 days ago".to_string())
        );
    }

    #[test]
    fn pretty_condition_titlecases_and_collapses_separators() {
        assert_eq!(pretty_condition("partly_cloudy"), "Partly Cloudy");
        assert_eq!(pretty_condition("clear-night"), "Clear Night");
        assert_eq!(pretty_condition("sunny"), "Sunny");
    }

    #[test]
    fn truncate_to_width_returns_empty_for_nonpositive_max() {
        let f = font_for(Weight::Regular);
        assert_eq!(truncate_to_width(f, 12.0, "Hello", 0), "");
        assert_eq!(truncate_to_width(f, 12.0, "Hello", -10), "");
    }

    #[test]
    fn truncate_to_width_appends_ellipsis_when_overflow() {
        let f = font_for(Weight::Regular);
        let truncated = truncate_to_width(f, 16.0, "An extremely long condition label", 40);
        assert!(
            truncated.ends_with('\u{2026}'),
            "expected ellipsis suffix, got {truncated:?}"
        );
    }

    #[test]
    fn truncate_to_width_returns_input_when_already_fits() {
        let f = font_for(Weight::Regular);
        assert_eq!(truncate_to_width(f, 12.0, "Hi", 200), "Hi");
    }

    #[test]
    fn weekday_short_covers_all_seven() {
        assert_eq!(weekday_short(chrono::Weekday::Mon), "Mon");
        assert_eq!(weekday_short(chrono::Weekday::Sun), "Sun");
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
        assert!(dark > 0, "weather render produced no dark pixels");
    }
}
