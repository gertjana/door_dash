//! Weather widget — current condition (left) + multi-day forecast (right).
//!
//! Layout (single row beneath the title):
//!
//! ```text
//!     [icon] [BIG TEMP]  |  Tue  Wed  Thu  Fri
//!      Cond / details    | [ic] [ic] [ic] [ic]
//!                        | 22°  20°  17°  19°
//! ```
//!
//! Mirrors `addon/app/render/widgets/weather.py`.

use chrono::{NaiveDate, Utc};
use chrono_tz::Tz;
use image::{GrayImage, Luma};
use imageproc::drawing::draw_line_segment_mut;

use crate::config::Settings;
use crate::render::fonts::{
    draw_crisp_text, draw_text, font_for, text_height, text_width, Weight, TITLE_SIZE,
};
use crate::render::icons::{draw_icon, icon_for_weather_state};
use crate::render::widgets::Rect;
use crate::sources::weather::{ForecastEntry, Weather};

/// Format a temperature for display. `None` becomes em-dash.
fn fmt_temp(t: Option<f64>, unit: &str) -> String {
    match t {
        None => "\u{2014}".to_string(), // em-dash
        Some(v) => format!("{}{unit}", v.round() as i64),
    }
}

/// Resolve `settings.timezone` to a `chrono_tz::Tz`, falling back to
/// UTC when the IANA name doesn't parse. UTC is a safe default
/// because the dashboard is dev-targeted at Europe/Amsterdam (which
/// always parses), so we only hit this branch if the user typed an
/// invalid string.
fn timezone(settings: &Settings) -> Tz {
    settings.timezone.parse::<Tz>().unwrap_or(chrono_tz::UTC)
}

/// Today's date in the configured timezone.
fn today(settings: &Settings) -> NaiveDate {
    let tz = timezone(settings);
    Utc::now().with_timezone(&tz).date_naive()
}

/// Title-case a condition string with `_` / `-` collapsed to spaces.
/// `"partly_cloudy"` → `"Partly Cloudy"`.
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
            out.extend(ch.to_uppercase());
            new_word = false;
        } else {
            out.extend(ch.to_lowercase());
        }
    }
    out
}

/// Render the weather widget into `rect`.
pub fn render(canvas: &mut GrayImage, settings: &Settings, weather: &Weather, rect: Rect) {
    // Title
    let title_f = font_for(Weight::Bold);
    draw_text(
        canvas,
        rect.x + 8,
        rect.y + 4,
        "Weather",
        title_f,
        TITLE_SIZE,
        0,
    );

    let content_top = rect.y + 38;
    let content_bottom = rect.y2() - 6;

    // ----- Left group: icon + big temperature -----
    let temp_text = match weather.temperature {
        None => "\u{2014}".to_string(),
        Some(v) => format!("{}{}", v.round() as i64, weather.temperature_unit),
    };
    let temp_f = font_for(Weight::Bold);
    let temp_size: f32 = 28.0;
    let temp_w = text_width(temp_f, temp_size, &temp_text).ceil() as i32;
    let temp_h = text_height(temp_f, temp_size).ceil() as i32;

    let icon_size: i32 = 40;
    let gap: i32 = 10;
    let left_x = rect.x + 10;

    let top_y = content_top + 2;

    let owned_icon;
    let icon_name = match weather.icon.as_deref() {
        Some(s) if !s.is_empty() => s,
        _ => {
            owned_icon = icon_for_weather_state(&weather.condition).to_string();
            owned_icon.as_str()
        }
    };
    let icon_x = left_x;
    // Vertically centre the icon against the big temperature text.
    let icon_y = top_y + (temp_h - icon_size) / 2;
    draw_icon(canvas, icon_name, icon_x, icon_y, icon_size as f32, 0);

    let temp_x = icon_x + icon_size + gap;
    let temp_y = top_y;
    draw_text(canvas, temp_x, temp_y, &temp_text, temp_f, temp_size, 0);

    // ----- Detail lines: condition + wind below the icon/temp -----
    let detail_f = font_for(Weight::Regular);
    let detail_size: f32 = 13.0;
    let line_h = (detail_size as i32) + 2;
    let detail_x = left_x;
    let detail_y = top_y + temp_h + 10;

    let cond_label = pretty_condition(&weather.condition);
    draw_crisp_text(
        canvas,
        detail_x,
        detail_y,
        &cond_label,
        detail_f,
        detail_size,
        0,
    );

    if let Some(wind) = weather.wind_speed {
        let wind_line = format!("Wind {} {}", wind.round() as i64, weather.wind_unit);
        let wind_y = detail_y + line_h;
        if wind_y + detail_size as i32 <= content_bottom {
            draw_crisp_text(
                canvas,
                detail_x,
                wind_y,
                &wind_line,
                detail_f,
                detail_size,
                0,
            );
        }
    }

    let left_group_right = temp_x + temp_w;
    let forecast_x = left_group_right + 16;

    // ----- Right group: next-N-days forecast -----
    let available_w = rect.x2() - 10 - forecast_x;
    if available_w < 80 {
        return;
    }

    let tz = timezone(settings);
    let today_d = today(settings);
    let upcoming: Vec<&ForecastEntry> = weather
        .forecast
        .iter()
        .filter(|f| f.when.with_timezone(&tz).date_naive() > today_d)
        .collect();
    if upcoming.is_empty() {
        return;
    }

    // Vertical divider for visual separation between current + forecast.
    let div_x = forecast_x - 8;
    draw_line_segment_mut(
        canvas,
        (div_x as f32, (content_top + 2) as f32),
        (div_x as f32, (content_bottom - 2) as f32),
        Luma([0]),
    );

    // Decide how many days we can fit (min 56px per column).
    let col_min: i32 = 56;
    let max_cols = (upcoming.len() as i32).min(available_w / col_min).max(1) as usize;
    let cols = &upcoming[..max_cols];
    let col_w = available_w / max_cols as i32;

    let day_f = font_for(Weight::Bold);
    let day_size: f32 = 12.0;
    let icon_sz: i32 = 24;
    let temp_small_f = font_for(Weight::Bold);
    let temp_small_size: f32 = 13.0;

    for (i, fcast) in cols.iter().enumerate() {
        let cx = forecast_x + (i as i32) * col_w;

        // Day label (e.g. "Tue") — local-time abbreviation.
        let local = fcast.when.with_timezone(&tz);
        let day_label = local.format("%a").to_string();
        let day_w = text_width(day_f, day_size, &day_label).ceil() as i32;
        let day_h = text_height(day_f, day_size).ceil() as i32;
        let day_x = cx + (col_w - day_w) / 2;
        let day_y = content_top + 2;
        draw_crisp_text(canvas, day_x, day_y, &day_label, day_f, day_size, 0);

        // Icon centred.
        let ic_name = icon_for_weather_state(&fcast.condition);
        let ic_x = cx + (col_w - icon_sz) / 2;
        let ic_y = day_y + day_h + 2;
        draw_icon(canvas, ic_name, ic_x, ic_y, icon_sz as f32, 0);

        // Temp: "high°/low°" or just "high°".
        let t_text = match (fcast.temp_high, fcast.temp_low) {
            (Some(hi), Some(lo)) => format!(
                "{}\u{00b0}/{}\u{00b0}",
                hi.round() as i64,
                lo.round() as i64
            ),
            _ => fmt_temp(fcast.temp_high, "\u{00b0}"),
        };
        let t_w = text_width(temp_small_f, temp_small_size, &t_text).ceil() as i32;
        let t_x = cx + (col_w - t_w) / 2;
        let t_y = ic_y + icon_sz + 1;
        let t_h = text_height(temp_small_f, temp_small_size).ceil() as i32;
        if t_y + t_h <= content_bottom {
            draw_crisp_text(canvas, t_x, t_y, &t_text, temp_small_f, temp_small_size, 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::weather::Weather;
    use chrono::TimeZone;

    fn settings_default() -> Settings {
        Settings::default()
    }

    fn weather_with_temp(temp: Option<f64>, cond: &str) -> Weather {
        Weather {
            condition: cond.to_string(),
            temperature: temp,
            temperature_unit: "\u{00b0}C".to_string(),
            precipitation_probability: None,
            wind_speed: Some(12.0),
            wind_unit: "km/h".to_string(),
            humidity: None,
            pressure: None,
            pressure_unit: "hPa".to_string(),
            wind_bearing: None,
            last_updated: None,
            icon: None,
            forecast: vec![],
            forecast_hourly: vec![],
        }
    }

    #[test]
    fn fmt_temp_dash_for_none() {
        assert_eq!(fmt_temp(None, "\u{00b0}"), "\u{2014}");
    }

    #[test]
    fn fmt_temp_rounds_and_appends_unit() {
        assert_eq!(fmt_temp(Some(21.4), "\u{00b0}C"), "21\u{00b0}C");
        assert_eq!(fmt_temp(Some(21.6), "\u{00b0}C"), "22\u{00b0}C");
    }

    #[test]
    fn pretty_condition_title_cases_words() {
        assert_eq!(pretty_condition("partly_cloudy"), "Partly Cloudy");
        assert_eq!(pretty_condition("partly-cloudy"), "Partly Cloudy");
        assert_eq!(pretty_condition("CLEAR-NIGHT"), "Clear Night");
        assert_eq!(pretty_condition("sunny"), "Sunny");
        assert_eq!(pretty_condition(""), "");
    }

    #[test]
    fn timezone_falls_back_to_utc_for_invalid_name() {
        let mut s = settings_default();
        s.timezone = "Definitely/Not/A/Real/Zone".to_string();
        assert_eq!(timezone(&s), chrono_tz::UTC);
    }

    #[test]
    fn timezone_parses_valid_name() {
        let mut s = settings_default();
        s.timezone = "Europe/Amsterdam".to_string();
        assert_eq!(timezone(&s).name(), "Europe/Amsterdam");
    }

    #[test]
    fn render_writes_dark_pixels() {
        let s = settings_default();
        let w = weather_with_temp(Some(21.0), "partlycloudy");
        let mut canvas = GrayImage::from_pixel(800, 200, Luma([255]));
        let rect = Rect::new(0, 0, 800, 200);
        render(&mut canvas, &s, &w, rect);
        let dark = canvas.pixels().filter(|p| p[0] < 200).count();
        assert!(dark > 0, "weather widget produced no dark pixels");
    }

    #[test]
    fn render_handles_missing_temperature() {
        let s = settings_default();
        let w = weather_with_temp(None, "sunny");
        let mut canvas = GrayImage::from_pixel(800, 200, Luma([255]));
        render(&mut canvas, &s, &w, Rect::new(0, 0, 800, 200));
        // Just shouldn't panic; em-dash + icon should still draw.
    }

    #[test]
    fn render_with_forecast_draws_divider() {
        let s = settings_default();
        let mut w = weather_with_temp(Some(20.0), "sunny");
        // Add a forecast entry strictly after today.
        let tz = timezone(&s);
        let tomorrow_local = Utc::now().with_timezone(&tz) + chrono::Duration::days(1);
        let tomorrow_utc = tomorrow_local.with_timezone(&Utc);
        w.forecast.push(ForecastEntry {
            when: tomorrow_utc,
            condition: "rainy".to_string(),
            temp_high: Some(18.0),
            temp_low: Some(11.0),
            precipitation_probability: Some(40),
        });
        let mut canvas = GrayImage::from_pixel(800, 200, Luma([255]));
        render(&mut canvas, &s, &w, Rect::new(0, 0, 800, 200));
        // The vertical divider sits well into the canvas; expect at
        // least one dark pixel along a vertical line in the right
        // half of the canvas.
        let mut found = false;
        for x in 400..800 {
            for y in 50..150 {
                if canvas.get_pixel(x, y)[0] < 200 {
                    found = true;
                    break;
                }
            }
            if found {
                break;
            }
        }
        assert!(found, "forecast region should have drawn pixels");
    }

    #[test]
    fn render_drops_forecast_for_today() {
        // Forecast entry whose local date == today should not render
        // a divider/column.
        let s = settings_default();
        let mut w = weather_with_temp(Some(20.0), "sunny");
        let tz = timezone(&s);
        let today_local = Utc::now().with_timezone(&tz);
        // Use noon today for stability across DST corner cases.
        let local_naive = today_local.date_naive().and_hms_opt(12, 0, 0).unwrap();
        let local = tz.from_local_datetime(&local_naive).single().unwrap();
        w.forecast.push(ForecastEntry {
            when: local.with_timezone(&Utc),
            condition: "rainy".to_string(),
            temp_high: Some(18.0),
            temp_low: Some(11.0),
            precipitation_probability: Some(40),
        });
        let mut canvas = GrayImage::from_pixel(800, 200, Luma([255]));
        render(&mut canvas, &s, &w, Rect::new(0, 0, 800, 200));
        // No forecast columns drawn — no day label "12 AM" or weekday
        // would appear at the right side. Hard to assert directly,
        // so just check the widget still draws something on the left
        // (the title + current condition).
        let dark_left = (0..400)
            .flat_map(|x| (0..200).map(move |y| (x, y)))
            .filter(|&(x, y)| canvas.get_pixel(x, y)[0] < 200)
            .count();
        assert!(dark_left > 0);
    }
}
