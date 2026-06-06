//! Calendar list widget — two-line entries with date / optional tag /
//! title / location.
//!
//! Per-entry layout:
//!
//! ```text
//!     +-------+------+----------------------------+
//!     | date  | TAG  | title                      |
//!     |       |      | location (smaller font)    |
//!     +-------+------+----------------------------+
//! ```
//!
//! * date         "16 May", or "Today" / "Tomorrow" for the next two days.
//! * TAG          if the summary contains a colon, the part before the
//!   colon is displayed as an inverted rounded-corner pill, and stripped
//!   from the rendered title.
//! * location     optional, rendered on the second line in a smaller font.
//!
//! Mirrors `addon/app/render/widgets/calendar_list.py`.

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use image::{GrayImage, Luma};
use imageproc::drawing::{draw_filled_circle_mut, draw_filled_rect_mut, draw_line_segment_mut};

use crate::config::Settings;
use crate::render::fonts::{draw_crisp_text, draw_text, font_for, text_width, Weight, TITLE_SIZE};
use crate::render::widgets::Rect;
use crate::sources::calendar::Event;

const DATE_COL_W: i32 = 92;
const GAP_AFTER_DATE: i32 = 6;
const GAP_AFTER_TAG: i32 = 8;

/// Local-date-aware human-readable label for a UTC instant.
///
/// "Today" if the event's local date matches today, "Tomorrow" if it's
/// the next local day, otherwise `"DD MMM"` with the leading zero of
/// the day-of-month stripped (matches the Python addon's output).
fn format_date(start: DateTime<Utc>, tz: Tz, now: DateTime<Utc>) -> String {
    let start_local = start.with_timezone(&tz).date_naive();
    let now_local = now.with_timezone(&tz).date_naive();
    let delta_days = (start_local - now_local).num_days();
    match delta_days {
        0 => "Today".to_string(),
        1 => "Tomorrow".to_string(),
        _ => {
            let formatted = start.with_timezone(&tz).format("%d %b").to_string();
            // Strip a single leading zero from the day-of-month — Python's
            // ``.lstrip("0")`` matches at most a leading-zero day, never a
            // legitimately-zero string ("0 Jan" can't occur in practice).
            formatted.trim_start_matches('0').to_string()
        }
    }
}

/// Time label for the second line of the date column.
fn format_time(ev: &Event, tz: Tz) -> String {
    if ev.all_day {
        "all day".to_string()
    } else {
        ev.start.with_timezone(&tz).format("%H:%M").to_string()
    }
}

/// If the summary contains a colon, return (`Some(tag)`, remainder).
/// Only treats the prefix as a tag when it looks like a short label —
/// 2..14 chars, no whitespace — so things like "1:1 with manager" or
/// timestamps are left alone.
pub(crate) fn split_tag(summary: &str) -> (Option<String>, String) {
    let Some(idx) = summary.find(':') else {
        return (None, summary.to_string());
    };
    let tag = summary[..idx].trim();
    let rest = summary[idx + 1..].trim();
    if !tag.is_empty()
        && !rest.is_empty()
        && (2..=14).contains(&tag.chars().count())
        && !tag.chars().any(char::is_whitespace)
    {
        (Some(tag.to_string()), rest.to_string())
    } else {
        (None, summary.to_string())
    }
}

/// Truncate `text` so it fits within `max_w` pixels at the given font/size,
/// appending `…` when truncation happens.
fn truncate(font: &ab_glyph::FontRef<'static>, size: f32, text: &str, max_w: i32) -> String {
    if text_width(font, size, text).ceil() as i32 <= max_w {
        return text.to_string();
    }
    // Truncate by character (UTF-8 safe) until "<head>…" fits.
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

/// Draw an inverted rounded-corner pill containing `label`. Returns the
/// pill's pixel width so callers can advance the layout cursor.
fn draw_tag_pill(
    canvas: &mut GrayImage,
    x: i32,
    y: i32,
    label: &str,
    font: &ab_glyph::FontRef<'static>,
    size: f32,
) -> i32 {
    let pad_x: i32 = 6;
    let pad_y: i32 = 2;
    let text_w = text_width(font, size, label).ceil() as i32;
    let text_h: i32 = 14; // approx for the small bold font
    let pill_w = text_w + pad_x * 2;
    let pill_h = text_h + pad_y * 2;
    let radius = pill_h / 2;

    // Pill = central filled rect + two end-caps (filled circles).
    let body_x = x + radius;
    let body_w = (pill_w - 2 * radius).max(0) as u32;
    if body_w > 0 {
        draw_filled_rect_mut(
            canvas,
            imageproc::rect::Rect::at(body_x, y).of_size(body_w, pill_h as u32),
            Luma([0]),
        );
    }
    draw_filled_circle_mut(canvas, (x + radius, y + radius), radius, Luma([0]));
    draw_filled_circle_mut(canvas, (x + pill_w - radius, y + radius), radius, Luma([0]));

    // Position text vertically — ascender-aware nudge matches Python.
    draw_crisp_text(canvas, x + pad_x, y + pad_y - 1, label, font, size, 255);
    pill_w
}

/// Render the calendar list widget into `rect`.
pub fn render(canvas: &mut GrayImage, settings: &Settings, events: &[Event], rect: Rect) {
    let title_f = font_for(Weight::Bold);
    draw_text(
        canvas,
        rect.x + 12,
        rect.y + 4,
        "Upcoming",
        title_f,
        TITLE_SIZE,
        0,
    );
    // Underline beneath the title.
    draw_line_segment_mut(
        canvas,
        ((rect.x + 12) as f32, (rect.y + 32) as f32),
        ((rect.x2() - 12) as f32, (rect.y + 32) as f32),
        Luma([0]),
    );

    if events.is_empty() {
        let body_f = font_for(Weight::Regular);
        draw_text(
            canvas,
            rect.x + 12,
            rect.y + 48,
            "No upcoming events.",
            body_f,
            16.0,
            0,
        );
        return;
    }

    let tz: Tz = settings.timezone.parse().unwrap_or(chrono_tz::UTC);
    let now = Utc::now();

    let date_f = font_for(Weight::Bold);
    let date_size: f32 = 15.0;
    let time_f = font_for(Weight::Regular);
    let time_size: f32 = 13.0;
    let title_f_row = font_for(Weight::Bold);
    // Bumped 16→18 for e-paper readability. Title bbox at 18 px is
    // ~20 px tall; location at y+19 still clears it (1 px gap) and the
    // total row content (title + location below) stays within the
    // existing row_h = 36, so no other geometry needs to move.
    let title_size_row: f32 = 18.0;
    let tag_f = font_for(Weight::Bold);
    let tag_size: f32 = 12.0;
    let loc_f = font_for(Weight::Regular);
    let loc_size: f32 = 11.0;

    let row_h: i32 = 36;
    let row_gap: i32 = 4;
    let mut y = rect.y + 42;

    for ev in events.iter().take(settings.max_events) {
        if y + row_h > rect.y2() - 2 {
            break;
        }

        // Column 1: date (top) + time (bottom).
        let date_text = format_date(ev.start, tz, now);
        let time_text = format_time(ev, tz);
        draw_crisp_text(canvas, rect.x + 12, y, &date_text, date_f, date_size, 0);
        draw_crisp_text(
            canvas,
            rect.x + 12,
            y + 17,
            &time_text,
            time_f,
            time_size,
            0,
        );

        // Column 2+3: tag pill + title on line 1, location on line 2.
        let content_x = rect.x + 12 + DATE_COL_W + GAP_AFTER_DATE;
        let content_max_x = rect.x2() - 10;
        let avail_w = content_max_x - content_x;

        let (tag, title_rest) = split_tag(&ev.summary);
        let mut cursor_x = content_x;
        if let Some(tag) = tag {
            let pill_w = draw_tag_pill(canvas, cursor_x, y + 1, &tag, tag_f, tag_size);
            cursor_x += pill_w + GAP_AFTER_TAG;
        }

        // Title (truncated to remaining width on this row).
        let title_max_w = (content_max_x - cursor_x).max(0);
        let title_text = truncate(title_f_row, title_size_row, &title_rest, title_max_w);
        draw_crisp_text(
            canvas,
            cursor_x,
            y,
            &title_text,
            title_f_row,
            title_size_row,
            0,
        );

        // Location line: smaller font, left-aligned under the title column.
        if let Some(loc) = &ev.location {
            // Calendar location strings often contain newlines (multi-line
            // postal addresses). Collapse all whitespace runs to single
            // spaces so width measurement is meaningful.
            let loc_one_line: String = loc.split_whitespace().collect::<Vec<_>>().join(" ");
            let loc_text = truncate(loc_f, loc_size, &loc_one_line, avail_w);
            draw_crisp_text(canvas, content_x, y + 19, &loc_text, loc_f, loc_size, 0);
        }

        y += row_h + row_gap;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn settings_default() -> Settings {
        Settings::default()
    }

    fn ev(start: DateTime<Utc>, summary: &str, all_day: bool, location: Option<&str>) -> Event {
        Event {
            start,
            end: None,
            summary: summary.to_string(),
            all_day,
            location: location.map(|s| s.to_string()),
        }
    }

    #[test]
    fn split_tag_extracts_short_prefix() {
        let (tag, rest) = split_tag("WORK: Quarterly review");
        assert_eq!(tag.as_deref(), Some("WORK"));
        assert_eq!(rest, "Quarterly review");
    }

    #[test]
    fn split_tag_rejects_long_prefix() {
        // Prefix > 14 chars — leave alone.
        let (tag, rest) = split_tag("This is a very long prefix: title");
        assert!(tag.is_none());
        assert_eq!(rest, "This is a very long prefix: title");
    }

    #[test]
    fn split_tag_rejects_short_one_char_prefix() {
        let (tag, _) = split_tag("a: x");
        assert!(tag.is_none());
    }

    #[test]
    fn split_tag_rejects_prefix_with_whitespace() {
        // "1:1 with manager" — prefix contains whitespace before the colon? No.
        // Use a real whitespace case: "tag with space: title".
        let (tag, _) = split_tag("tag space: title");
        assert!(tag.is_none());
    }

    #[test]
    fn split_tag_rejects_empty_remainder() {
        let (tag, _) = split_tag("WORK:   ");
        assert!(tag.is_none());
    }

    #[test]
    fn split_tag_handles_no_colon() {
        let (tag, rest) = split_tag("Just a title");
        assert!(tag.is_none());
        assert_eq!(rest, "Just a title");
    }

    #[test]
    fn format_date_today_and_tomorrow() {
        let tz: Tz = "Europe/Amsterdam".parse().unwrap();
        let now = tz
            .with_ymd_and_hms(2026, 5, 16, 10, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        // Today
        let today = tz
            .with_ymd_and_hms(2026, 5, 16, 14, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(format_date(today, tz, now), "Today");
        // Tomorrow
        let tomorrow = tz
            .with_ymd_and_hms(2026, 5, 17, 9, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(format_date(tomorrow, tz, now), "Tomorrow");
    }

    #[test]
    fn format_date_strips_leading_zero_dom() {
        let tz: Tz = "Europe/Amsterdam".parse().unwrap();
        let now = tz
            .with_ymd_and_hms(2026, 5, 1, 10, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        let later = tz
            .with_ymd_and_hms(2026, 5, 7, 9, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(format_date(later, tz, now), "7 May");
    }

    #[test]
    fn format_time_all_day_returns_all_day_label() {
        let tz: Tz = "Europe/Amsterdam".parse().unwrap();
        let start = tz
            .with_ymd_and_hms(2026, 5, 16, 0, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        let event = ev(start, "Birthday", true, None);
        assert_eq!(format_time(&event, tz), "all day");
    }

    #[test]
    fn format_time_uses_local_clock() {
        let tz: Tz = "Europe/Amsterdam".parse().unwrap();
        // 12:00 UTC on 2026-01-15 = 13:00 in Amsterdam (CET, UTC+1).
        let start = Utc.with_ymd_and_hms(2026, 1, 15, 12, 0, 0).unwrap();
        let event = ev(start, "Meeting", false, None);
        assert_eq!(format_time(&event, tz), "13:00");
    }

    #[test]
    fn truncate_returns_input_when_fits() {
        let f = font_for(Weight::Regular);
        // 10000 px max width — anything fits.
        let out = truncate(f, 14.0, "hello", 10000);
        assert_eq!(out, "hello");
    }

    #[test]
    fn truncate_appends_ellipsis_when_too_wide() {
        let f = font_for(Weight::Regular);
        // 10 px max — no real word fits.
        let out = truncate(f, 14.0, "Hello world", 10);
        assert!(
            out.ends_with('\u{2026}'),
            "expected ellipsis suffix; got {out:?}"
        );
    }

    #[test]
    fn render_empty_events_shows_placeholder() {
        let s = settings_default();
        let mut canvas = GrayImage::from_pixel(800, 200, Luma([255]));
        render(&mut canvas, &s, &[], Rect::new(0, 0, 800, 200));
        // Title + "No upcoming events" line should produce dark pixels.
        let dark = canvas.pixels().filter(|p| p[0] < 200).count();
        assert!(dark > 0);
    }

    #[test]
    fn render_with_events_writes_pixels() {
        let s = settings_default();
        let tz: Tz = s.timezone.parse().unwrap();
        let start = tz
            .with_ymd_and_hms(2099, 1, 1, 9, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        let events = vec![ev(start, "WORK: Standup", false, Some("HQ"))];
        let mut canvas = GrayImage::from_pixel(800, 200, Luma([255]));
        render(&mut canvas, &s, &events, Rect::new(0, 0, 800, 200));
        let dark = canvas.pixels().filter(|p| p[0] < 200).count();
        assert!(dark > 0);
    }

    #[test]
    fn render_caps_at_max_events() {
        let mut s = settings_default();
        s.max_events = 2;
        let tz: Tz = s.timezone.parse().unwrap();
        let mut events = vec![];
        for i in 0..10 {
            let start = tz
                .with_ymd_and_hms(2099, 1, 1, 9 + i, 0, 0)
                .unwrap()
                .with_timezone(&Utc);
            events.push(ev(start, &format!("Event {i}"), false, None));
        }
        let mut canvas = GrayImage::from_pixel(800, 600, Luma([255]));
        render(&mut canvas, &s, &events, Rect::new(0, 0, 800, 600));
        // Just shouldn't panic; visual cap not directly asserted.
    }
}
