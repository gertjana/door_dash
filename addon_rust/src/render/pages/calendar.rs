//! Page 1 — full-screen month calendar.
//!
//! Renders a 7-column × 6-row month grid for the current month, with
//! events drawn as Outlook-style stacked bars inside each day cell.
//! Week starts Monday; today's day-number is bold.
//!
//! Mirrors `addon/app/render/pages/01_calendar.py`.
//!
//! # Drawing strategy on a 1-bit panel
//!
//! Two visual styles for event bars keep things readable when stacked:
//!
//! * **all-day** — filled black rectangle, white text
//! * **timed** — plain text on the cell background (no outline)
//!
//! Days outside the displayed month are drawn with a smaller,
//! regular-weight day-number to de-emphasize them without relying on
//! greys (which would just collapse to black or white at 1-bit
//! threshold time anyway).

use ab_glyph::FontRef;
use chrono::{DateTime, Datelike, Duration, LocalResult, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use image::{GrayImage, Luma};
use imageproc::drawing::{draw_filled_rect_mut, draw_line_segment_mut};
use tracing::info;

use crate::config::Settings;
use crate::render::badge::draw_version_badge;
use crate::render::blank_canvas;
use crate::render::fonts::{draw_crisp_text, draw_text, font_for, text_height, text_width, Weight};
use crate::render::pages::RenderFuture;
use crate::sources::calendar::{fetch_range, Event};
use crate::sources::local_sensors::LocalSensors;

// === Layout tunables (pixels) ============================================
const HEADER_HEIGHT: i32 = 56; // Month/year title strip at the top
const WEEKDAY_HEIGHT: i32 = 22; // Mon/Tue/... header row
const CELL_PADDING: i32 = 3; // Inner padding inside every day cell
const EVENT_BAR_HEIGHT: i32 = 13; // Height of a single event bar
const EVENT_BAR_GAP: i32 = 2; // Vertical gap between stacked bars
const GRID_LINE: i32 = 1; // 1px grid lines

/// Resolve `settings.timezone` to a chrono-tz zone, falling back to
/// UTC for unparseable names.
fn timezone(settings: &Settings) -> Tz {
    settings.timezone.parse::<Tz>().unwrap_or(chrono_tz::UTC)
}

/// Convert a calendar date (date-only) at local-midnight to the
/// equivalent absolute instant.
///
/// DST quirks:
///
/// * `Single` — normal case, return as-is.
/// * `Ambiguous` — only happens in the 02:00–03:00 fall-back window,
///   which can't include 00:00. Defensively return the earlier
///   variant if we ever see it for a midnight.
/// * `None` — only happens in the 02:00–03:00 spring-forward gap,
///   again not at 00:00. If we ever see it, fall back to 03:00 of
///   that day, then to UTC, so the page never panics.
fn local_midnight(date: NaiveDate, tz: Tz) -> DateTime<Tz> {
    let naive = date
        .and_hms_opt(0, 0, 0)
        .expect("midnight is always a valid time-of-day");
    match tz.from_local_datetime(&naive) {
        LocalResult::Single(dt) => dt,
        LocalResult::Ambiguous(a, _) => a,
        LocalResult::None => {
            let after_dst = date
                .and_hms_opt(3, 0, 0)
                .expect("03:00 is always a valid time-of-day");
            tz.from_local_datetime(&after_dst)
                .earliest()
                .unwrap_or_else(|| Utc.from_utc_datetime(&naive).with_timezone(&tz))
        }
    }
}

/// Build a 6-row × 7-col grid of `NaiveDate` covering the displayed
/// month. Always 6 rows so the grid geometry is constant; weeks start
/// Monday. Days outside the requested month are filled in from
/// surrounding months — the caller draws them in a muted style.
fn month_grid_days(year: i32, month: u32) -> [[NaiveDate; 7]; 6] {
    let first = NaiveDate::from_ymd_opt(year, month, 1).expect("calendar page: invalid year/month");
    // chrono's `weekday().num_days_from_monday()` returns 0..=6 with
    // Monday=0, which matches our Monday-starting grid convention.
    let offset = i64::from(first.weekday().num_days_from_monday());
    let grid_start = first - Duration::days(offset);

    // Initialize with a placeholder; every cell gets overwritten.
    let placeholder = NaiveDate::from_ymd_opt(2000, 1, 1).expect("placeholder date is valid");
    let mut grid = [[placeholder; 7]; 6];
    // 2D-grid index loop reads more clearly than `iter_mut().enumerate()`
    // because we need both row and column indices to compute the offset.
    #[allow(clippy::needless_range_loop)]
    for r in 0..6 {
        for c in 0..7 {
            grid[r][c] = grid_start + Duration::days((r * 7 + c) as i64);
        }
    }
    grid
}

/// Truncate `text` so it fits within `max_w` pixels at the given font
/// + size, appending an ellipsis when truncation happens. UTF-8 safe.
fn truncate_to_width(font: &FontRef<'static>, size: f32, text: &str, max_w: i32) -> String {
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

/// Filter `events` to the ones whose interval intersects `day` in
/// the local timezone.
///
/// All-day events compare on **date** only — HA gives `start` =
/// first day, `end` = exclusive day-after-last (e.g. for a one-day
/// "Holiday" on June 3, end = June 4 00:00). Comparing on date avoids
/// the spurious "spills onto June 4" bug that would happen if we
/// timezone-shifted the UTC-stamped midnight.
///
/// Timed events use the full timezone-aware interval.
fn events_for_day(events: &[Event], day: NaiveDate, tz: Tz) -> Vec<Event> {
    let day_start_local = local_midnight(day, tz);
    let day_end_local = day_start_local + Duration::days(1);

    let mut out: Vec<Event> = Vec::new();
    for ev in events {
        if ev.all_day {
            // HA: `end` is exclusive day-after-last; if absent, point.
            let start_date = ev.start.date_naive();
            let last_date = ev
                .end
                .map(|e| (e - Duration::days(1)).date_naive())
                .unwrap_or(start_date);
            if start_date <= day && day <= last_date {
                out.push(ev.clone());
            }
        } else {
            let start_local = ev.start.with_timezone(&tz);
            let end_local = ev.end.unwrap_or(ev.start).with_timezone(&tz);
            if end_local >= day_start_local && start_local < day_end_local {
                out.push(ev.clone());
            }
        }
    }
    // All-day events first, then timed events sorted by start time.
    out.sort_by_key(|e| (!e.all_day, e.start));
    out
}

/// Title-case the C-locale month name. We avoid pulling in a
/// localisation crate because the Python addon also uses C-locale
/// month names (`calendar.month_name[m]`).
fn month_name(month: u32) -> &'static str {
    match month {
        1 => "January",
        2 => "February",
        3 => "March",
        4 => "April",
        5 => "May",
        6 => "June",
        7 => "July",
        8 => "August",
        9 => "September",
        10 => "October",
        11 => "November",
        12 => "December",
        _ => "",
    }
}

/// Render the full-screen month calendar page.
pub async fn render(
    settings: Settings,
    _sensors: LocalSensors,
    fw_version: Option<String>,
) -> GrayImage {
    let w = settings.width as i32;
    let h = settings.height as i32;
    let mut img = blank_canvas(settings.width, settings.height);

    // Resolve "today" in the configured timezone so the grid centres
    // on the right month and "today" is highlighted correctly across
    // the midnight rollover regardless of UTC vs. local.
    let tz = timezone(&settings);
    let today_local = Utc::now().with_timezone(&tz).date_naive();
    let year = today_local.year();
    let month = today_local.month();

    // === Header strip: month + year, centred ===
    let title_text = format!("{} {year}", month_name(month));
    let title_f = font_for(Weight::Bold);
    let title_size: f32 = 36.0;
    let tw = text_width(title_f, title_size, &title_text).ceil() as i32;
    let th = text_height(title_f, title_size).ceil() as i32;
    draw_text(
        &mut img,
        (w - tw) / 2,
        (HEADER_HEIGHT - th) / 2 - 4,
        &title_text,
        title_f,
        title_size,
        0,
    );

    // === Weekday header row (Mon..Sun) ===
    let weekday_f = font_for(Weight::Bold);
    let weekday_size: f32 = 13.0;
    let col_w = w as f32 / 7.0; // float — quantise per-cell when drawing
    let wd_y = HEADER_HEIGHT;
    let weekdays = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    for (i, name) in weekdays.iter().enumerate() {
        let x_centre = (col_w * i as f32 + col_w / 2.0) as i32;
        let nw = text_width(weekday_f, weekday_size, name).ceil() as i32;
        let nh = text_height(weekday_f, weekday_size).ceil() as i32;
        draw_crisp_text(
            &mut img,
            x_centre - nw / 2,
            wd_y + (WEEKDAY_HEIGHT - nh) / 2 - 2,
            name,
            weekday_f,
            weekday_size,
            0,
        );
    }
    // Separator under the weekday row.
    let sep_y = HEADER_HEIGHT + WEEKDAY_HEIGHT;
    draw_line_segment_mut(
        &mut img,
        (0.0, sep_y as f32),
        (w as f32, sep_y as f32),
        Luma([0]),
    );

    // === Grid geometry ===
    let grid_top = sep_y + GRID_LINE;
    let grid_h = h - grid_top;
    let row_h = grid_h as f32 / 6.0;

    // === Fetch events for the displayed window ===
    let grid = month_grid_days(year, month);
    let win_start_local = local_midnight(grid[0][0], tz);
    let win_end_local = local_midnight(grid[5][6] + Duration::days(1), tz);
    let events = fetch_range(
        &settings,
        win_start_local.with_timezone(&Utc),
        win_end_local.with_timezone(&Utc),
    )
    .await;
    info!(
        events = events.len(),
        window_start = %grid[0][0],
        window_end = %grid[5][6],
        "calendar page: fetched events"
    );

    // === Day fonts (chosen per-cell based on in_month / today flags) ===
    let day_num_f = font_for(Weight::Regular);
    let day_num_bold_f = font_for(Weight::Bold);
    let day_num_muted_f = font_for(Weight::Regular);
    let event_f = font_for(Weight::Regular);
    let event_size: f32 = 10.0;

    // 2D loop again — see the same lint waiver in `month_grid_days`.
    #[allow(clippy::needless_range_loop)]
    for r in 0..6 {
        for c in 0..7 {
            let cell_x = (col_w * c as f32) as i32;
            let cell_y = (grid_top as f32 + row_h * r as f32) as i32;
            let cell_x2 = (col_w * (c + 1) as f32) as i32;
            let cell_y2 = (grid_top as f32 + row_h * (r + 1) as f32) as i32;

            // Cell border lines (right + bottom; left/top covered by
            // adjacent cells or the canvas edge).
            draw_line_segment_mut(
                &mut img,
                ((cell_x2 - GRID_LINE) as f32, cell_y as f32),
                ((cell_x2 - GRID_LINE) as f32, cell_y2 as f32),
                Luma([0]),
            );
            draw_line_segment_mut(
                &mut img,
                (cell_x as f32, (cell_y2 - GRID_LINE) as f32),
                (cell_x2 as f32, (cell_y2 - GRID_LINE) as f32),
                Luma([0]),
            );

            let day = grid[r][c];
            let in_month = day.month() == month;
            let is_today = day == today_local;

            // Day number — top-left of the cell.
            let day_str = day.day().to_string();
            let (day_font, day_size): (&FontRef<'static>, f32) = if !in_month {
                (day_num_muted_f, 13.0)
            } else if is_today {
                (day_num_bold_f, 15.0)
            } else {
                (day_num_f, 15.0)
            };
            draw_crisp_text(
                &mut img,
                cell_x + CELL_PADDING + 1,
                cell_y + CELL_PADDING - 1,
                &day_str,
                day_font,
                day_size,
                0,
            );

            // Event bars below the day number.
            let day_events = events_for_day(&events, day, tz);
            if day_events.is_empty() {
                continue;
            }

            let bar_top = cell_y + CELL_PADDING + 16; // 16 ≈ day-num line height
            let bar_bottom = cell_y2 - CELL_PADDING - GRID_LINE;
            let bar_left = cell_x + CELL_PADDING;
            let bar_right = cell_x2 - CELL_PADDING - GRID_LINE;
            let bar_w = bar_right - bar_left;
            let avail_h = bar_bottom - bar_top;
            if avail_h < EVENT_BAR_HEIGHT {
                continue;
            }
            let max_bars =
                (((avail_h + EVENT_BAR_GAP) / (EVENT_BAR_HEIGHT + EVENT_BAR_GAP)).max(1)) as usize;

            // Reserve the last bar slot for "+N more" if we'd overflow.
            let total = day_events.len();
            let (shown, overflow): (Vec<&Event>, usize) = if total > max_bars {
                let cap = max_bars.saturating_sub(1);
                (day_events.iter().take(cap).collect(), total - cap)
            } else {
                (day_events.iter().collect(), 0)
            };

            let mut by = bar_top;
            for ev in shown {
                let bx2 = bar_left + bar_w;
                let by2 = by + EVENT_BAR_HEIGHT;
                let raw_title: &str = if ev.summary.is_empty() {
                    "(no title)"
                } else {
                    ev.summary.as_str()
                };

                let (text_fill, text_x, mut display_title) = if ev.all_day {
                    // Filled black bar, white text — keeps all-day
                    // events visually distinct from timed ones without
                    // using a border (matches Python's no-border policy).
                    let rect_w = (bx2 - bar_left).max(0) as u32;
                    let rect_h = (by2 - by).max(0) as u32;
                    if rect_w > 0 && rect_h > 0 {
                        draw_filled_rect_mut(
                            &mut img,
                            imageproc::rect::Rect::at(bar_left, by).of_size(rect_w, rect_h),
                            Luma([0]),
                        );
                    }
                    (255u8, bar_left + 3, raw_title.to_string())
                } else {
                    // Plain text on the cell background — no outline.
                    let local_start = ev.start.with_timezone(&tz);
                    let title = format!("{} {}", local_start.format("%H:%M"), raw_title);
                    (0u8, bar_left + 3, title)
                };

                let inner_w = bx2 - text_x - 2;
                display_title = truncate_to_width(event_f, event_size, &display_title, inner_w);
                draw_crisp_text(
                    &mut img,
                    text_x,
                    by - 1,
                    &display_title,
                    event_f,
                    event_size,
                    text_fill,
                );
                by = by2 + EVENT_BAR_GAP;
            }

            if overflow > 0 {
                let more_text = format!("+{overflow} more");
                let trimmed = truncate_to_width(event_f, event_size, &more_text, bar_w - 4);
                draw_crisp_text(
                    &mut img,
                    bar_left + 2,
                    by - 1,
                    &trimmed,
                    event_f,
                    event_size,
                    0,
                );
            }
        }
    }

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
    use chrono::Timelike;
    use chrono_tz::Europe::Amsterdam;

    #[test]
    fn month_grid_has_six_rows_seven_cols() {
        let g = month_grid_days(2026, 6);
        // June 2026 starts on Monday, so the first cell is exactly
        // 2026-06-01 with no padding from May.
        assert_eq!(g[0][0], NaiveDate::from_ymd_opt(2026, 6, 1).unwrap());
        // Last cell is 6*7=42 days after the first.
        assert_eq!(
            g[5][6],
            NaiveDate::from_ymd_opt(2026, 6, 1).unwrap() + Duration::days(41)
        );
    }

    #[test]
    fn month_grid_pads_with_previous_month_for_offset_first() {
        // May 2026 starts on a Friday. The grid should pad with the
        // four preceding April days so the first column is Monday.
        let g = month_grid_days(2026, 5);
        assert_eq!(g[0][0], NaiveDate::from_ymd_opt(2026, 4, 27).unwrap());
        assert_eq!(g[0][4], NaiveDate::from_ymd_opt(2026, 5, 1).unwrap());
    }

    #[test]
    fn local_midnight_returns_midnight_in_zone() {
        let d = NaiveDate::from_ymd_opt(2026, 6, 15).unwrap();
        let dt = local_midnight(d, Amsterdam);
        assert_eq!(dt.naive_local().date(), d);
        assert_eq!(dt.naive_local().time().hour(), 0);
    }

    #[test]
    fn truncate_to_width_appends_ellipsis_when_overflow() {
        let f = font_for(Weight::Regular);
        let truncated = truncate_to_width(f, 10.0, "Some really long event title", 30);
        assert!(
            truncated.ends_with('\u{2026}'),
            "expected ellipsis suffix, got {truncated:?}"
        );
    }

    #[test]
    fn truncate_to_width_returns_input_when_already_fits() {
        let f = font_for(Weight::Regular);
        let s = "Hi";
        assert_eq!(truncate_to_width(f, 10.0, s, 200), s);
    }

    #[test]
    fn month_name_handles_known_and_unknown() {
        assert_eq!(month_name(1), "January");
        assert_eq!(month_name(12), "December");
        // Out-of-range months collapse to empty rather than panic.
        assert_eq!(month_name(0), "");
        assert_eq!(month_name(13), "");
    }

    #[test]
    fn events_for_day_includes_overlapping_all_day_event() {
        let target = NaiveDate::from_ymd_opt(2026, 6, 15).unwrap();
        let ev = Event {
            start: Utc.with_ymd_and_hms(2026, 6, 14, 0, 0, 0).unwrap(),
            // Three-day event ending exclusively on the 17th — covers
            // 14, 15, 16. So 15 is included.
            end: Some(Utc.with_ymd_and_hms(2026, 6, 17, 0, 0, 0).unwrap()),
            summary: "Conference".to_string(),
            all_day: true,
            location: None,
        };
        let result = events_for_day(&[ev], target, Amsterdam);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn events_for_day_excludes_non_overlapping_event() {
        let target = NaiveDate::from_ymd_opt(2026, 6, 15).unwrap();
        let ev = Event {
            // Single-day timed event on a different day.
            start: Utc.with_ymd_and_hms(2026, 6, 20, 9, 0, 0).unwrap(),
            end: Some(Utc.with_ymd_and_hms(2026, 6, 20, 10, 0, 0).unwrap()),
            summary: "Standup".to_string(),
            all_day: false,
            location: None,
        };
        assert!(events_for_day(&[ev], target, Amsterdam).is_empty());
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
        assert!(dark > 0, "calendar render produced no dark pixels");
    }
}
