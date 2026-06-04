//! Calendar source — upcoming events from HA calendar entities.
//!
//! Returns up to `settings.max_events` chronologically sorted events
//! merged across all configured calendars. `fetch_range()` widens the
//! window for a month-grid view and skips the per-event cap.
//!
//! Mirrors `addon/app/sources/calendar.py`. The Python addon uses
//! `dateutil.parser.parse` which is more permissive than chrono's
//! built-in parsers; for HA calendar payloads we only need RFC 3339
//! `dateTime` and `YYYY-MM-DD` `date` formats, which we handle
//! explicitly here.

use chrono::{DateTime, Duration, NaiveDate, NaiveTime, Utc};
use serde_json::Value;
use tracing::debug;

use crate::config::Settings;
use crate::ha_client::HAClient;

/// One calendar event. `end` may be `None` for events without a
/// declared end time (HA generally always provides one, but stay
/// defensive). `all_day` is true when the event spans whole days
/// rather than a specific time-of-day window — HA signals this by
/// using `date` (YYYY-MM-DD) rather than `dateTime` in the payload.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub start: DateTime<Utc>,
    pub end: Option<DateTime<Utc>>,
    pub summary: String,
    pub all_day: bool,
    pub location: Option<String>,
}

/// Try parsing a `dateTime` (RFC 3339) or `date` (YYYY-MM-DD) string.
/// Date-only values are treated as midnight UTC, matching the Python
/// addon's `dateutil.parser` + `replace(tzinfo=UTC)` behaviour.
fn parse_dt(s: Option<&str>) -> Option<DateTime<Utc>> {
    let s = s?;
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }
    if let Ok(date) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        let dt = date.and_time(NaiveTime::from_hms_opt(0, 0, 0)?);
        return Some(DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc));
    }
    None
}

/// Collapse runs of whitespace (incl. newlines) to single spaces.
///
/// Calendar fields routinely contain multi-line text — addresses with
/// embedded newlines, descriptions with carriage returns. Single-line
/// rendering is the only sensible output, and pre-cleaning here means
/// downstream widgets don't have to know.
fn clean(text: Option<&str>) -> Option<String> {
    let t = text?;
    let cleaned: String = t.split_whitespace().collect::<Vec<&str>>().join(" ");
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

/// Convert one HA calendar API event into our typed `Event`. Returns
/// `None` if the event has no parseable start time.
fn to_event(raw: &Value) -> Option<Event> {
    let summary = clean(raw.get("summary").and_then(Value::as_str))
        .unwrap_or_else(|| "(no title)".to_owned());
    let location = clean(raw.get("location").and_then(Value::as_str));

    let start_obj = raw.get("start")?;
    let end_obj = raw.get("end");

    // `date` (no `dateTime`) signals an all-day event.
    let all_day = start_obj.get("date").is_some() && start_obj.get("dateTime").is_none();

    let start_str = start_obj
        .get("dateTime")
        .or_else(|| start_obj.get("date"))
        .and_then(Value::as_str);
    let end_str = end_obj
        .and_then(|o| o.get("dateTime").or_else(|| o.get("date")))
        .and_then(Value::as_str);

    let start = parse_dt(start_str)?;
    let end = parse_dt(end_str);

    Some(Event {
        start,
        end,
        summary,
        all_day,
        location,
    })
}

/// Synthesised events for offline dev. Anchored relative to "now" so
/// the dashboard never looks frozen during local renders.
fn fallback(settings: &Settings) -> Vec<Event> {
    let now = Utc::now();
    let today_midnight = DateTime::<Utc>::from_naive_utc_and_offset(
        now.date_naive().and_hms_opt(0, 0, 0).unwrap(),
        Utc,
    );
    let samples: Vec<Event> = vec![
        Event {
            start: now + Duration::hours(3),
            end: Some(now + Duration::hours(4)),
            summary: "Dentist appointment".to_owned(),
            all_day: false,
            location: Some("Tandartspraktijk Centrum".to_owned()),
        },
        Event {
            start: now + Duration::days(1) + Duration::hours(2),
            end: Some(now + Duration::days(1) + Duration::hours(3)),
            summary: "Work: Team standup".to_owned(),
            all_day: false,
            location: Some("Online".to_owned()),
        },
        Event {
            start: now + Duration::days(1) + Duration::hours(11),
            end: Some(now + Duration::days(1) + Duration::hours(13)),
            summary: "Dinner with Anna".to_owned(),
            all_day: false,
            location: Some("Restaurant De Kas".to_owned()),
        },
        Event {
            start: now + Duration::days(2) + Duration::hours(1),
            end: Some(now + Duration::days(2) + Duration::hours(2)),
            summary: "Family: School run".to_owned(),
            all_day: false,
            location: None,
        },
        Event {
            start: now + Duration::days(2) + Duration::hours(7),
            end: Some(now + Duration::days(2) + Duration::hours(8)),
            summary: "Work: 1:1 with manager".to_owned(),
            all_day: false,
            location: Some("Office, Room 3.14".to_owned()),
        },
        // All-day "Holiday" on (today + 3). End is exclusive — midnight of (today + 4).
        Event {
            start: today_midnight + Duration::days(3),
            end: Some(today_midnight + Duration::days(4)),
            summary: "Holiday".to_owned(),
            all_day: true,
            location: None,
        },
        Event {
            start: now + Duration::days(4) + Duration::hours(3),
            end: Some(now + Duration::days(4) + Duration::hours(4)),
            summary: "Health: Doctor".to_owned(),
            all_day: false,
            location: Some("Huisartsenpraktijk".to_owned()),
        },
        Event {
            start: now + Duration::days(5) + Duration::hours(5),
            end: Some(now + Duration::days(5) + Duration::hours(6)),
            summary: "Lunch in town".to_owned(),
            all_day: false,
            location: Some("Café Brecht".to_owned()),
        },
    ];
    samples.into_iter().take(settings.max_events).collect()
}

/// Fetch upcoming events for the dashboard's "Upcoming" list.
///
/// Returns an empty list when HA is unreachable or returns no
/// events. The `calendar_list` widget renders a "No upcoming events."
/// message in that case — the previous behaviour returned demo
/// fallback data, which made it impossible to tell at a glance
/// whether the calendar integration was broken or genuinely empty.
///
/// `fetch_range` (used by the month grid page) still falls back to
/// sample data when HA is unavailable so dev runs without HA stay
/// visually meaningful.
pub async fn fetch(settings: &Settings) -> Vec<Event> {
    let ha = HAClient::new(settings);
    if !ha.available() {
        return Vec::new();
    }
    let now = Utc::now();
    let end_window = now + Duration::days(30);
    let start_iso = now.to_rfc3339();
    let end_iso = end_window.to_rfc3339();

    let mut events: Vec<Event> = Vec::new();
    for entity in &settings.calendar_entities {
        let raw = ha.get_calendar(entity, &start_iso, &end_iso).await;
        for ev_json in &raw {
            if let Some(ev) = to_event(ev_json) {
                events.push(ev);
            }
        }
    }
    events.sort_by_key(|e| e.start);
    events.truncate(settings.max_events);
    debug!(count = events.len(), "calendar: fetched");
    events
}

/// Fetch all events from all configured calendars within `[start, end]`.
///
/// Unlike `fetch()` this does not cap at `settings.max_events` — the
/// caller (e.g. a full-month grid renderer) usually wants every event
/// in the window. Returns a fallback set only if HA is unreachable;
/// an empty real result from HA is returned as-is.
pub async fn fetch_range(
    settings: &Settings,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Vec<Event> {
    let ha = HAClient::new(settings);
    if !ha.available() {
        return fallback(settings)
            .into_iter()
            .filter(|e| e.start >= start && e.start <= end)
            .collect();
    }
    let start_iso = start.to_rfc3339();
    let end_iso = end.to_rfc3339();
    let mut events: Vec<Event> = Vec::new();
    for entity in &settings.calendar_entities {
        let raw = ha.get_calendar(entity, &start_iso, &end_iso).await;
        for ev_json in &raw {
            if let Some(ev) = to_event(ev_json) {
                events.push(ev);
            }
        }
    }
    events.sort_by_key(|e| e.start);
    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_dt_handles_rfc3339_with_offset() {
        let dt = parse_dt(Some("2026-05-26T15:00:00+02:00")).unwrap();
        assert_eq!(dt.to_rfc3339(), "2026-05-26T13:00:00+00:00");
    }

    #[test]
    fn parse_dt_handles_z_suffix() {
        let dt = parse_dt(Some("2026-05-26T13:00:00Z")).unwrap();
        assert_eq!(dt.to_rfc3339(), "2026-05-26T13:00:00+00:00");
    }

    #[test]
    fn parse_dt_handles_date_only_as_midnight_utc() {
        let dt = parse_dt(Some("2026-05-26")).unwrap();
        assert_eq!(dt.to_rfc3339(), "2026-05-26T00:00:00+00:00");
    }

    #[test]
    fn parse_dt_returns_none_for_garbage() {
        assert!(parse_dt(Some("not-a-date")).is_none());
        assert!(parse_dt(None).is_none());
    }

    #[test]
    fn clean_collapses_whitespace() {
        assert_eq!(
            clean(Some("  hello\nworld  ")),
            Some("hello world".to_owned())
        );
        assert_eq!(clean(Some("\t\n   ")), None);
        assert_eq!(clean(None), None);
    }

    #[test]
    fn to_event_handles_timed_event() {
        let raw = json!({
            "summary": "Meeting",
            "start": {"dateTime": "2026-05-26T15:00:00+00:00"},
            "end": {"dateTime": "2026-05-26T16:00:00+00:00"},
            "location": "Office"
        });
        let ev = to_event(&raw).unwrap();
        assert_eq!(ev.summary, "Meeting");
        assert!(!ev.all_day);
        assert_eq!(ev.location, Some("Office".to_owned()));
        assert!(ev.end.is_some());
    }

    #[test]
    fn to_event_handles_all_day_event() {
        let raw = json!({
            "summary": "Holiday",
            "start": {"date": "2026-05-26"},
            "end": {"date": "2026-05-27"}
        });
        let ev = to_event(&raw).unwrap();
        assert!(ev.all_day);
        assert_eq!(ev.start.to_rfc3339(), "2026-05-26T00:00:00+00:00");
    }

    #[test]
    fn to_event_returns_none_when_start_unparseable() {
        let raw = json!({"summary": "Bad", "start": {"dateTime": "garbage"}});
        assert!(to_event(&raw).is_none());
    }

    #[test]
    fn to_event_substitutes_no_title_when_summary_missing() {
        let raw = json!({"start": {"dateTime": "2026-05-26T15:00:00+00:00"}});
        let ev = to_event(&raw).unwrap();
        assert_eq!(ev.summary, "(no title)");
    }

    #[test]
    fn to_event_cleans_multiline_summary_and_location() {
        let raw = json!({
            "summary": "Line one\nLine two",
            "start": {"dateTime": "2026-05-26T15:00:00+00:00"},
            "location": "  Tandarts  \n  Centrum  "
        });
        let ev = to_event(&raw).unwrap();
        assert_eq!(ev.summary, "Line one Line two");
        assert_eq!(ev.location, Some("Tandarts Centrum".to_owned()));
    }

    #[tokio::test]
    async fn fetch_returns_empty_when_ha_unavailable() {
        let settings = Settings {
            supervisor_token: None,
            ..Settings::default()
        };
        let events = fetch(&settings).await;
        assert!(events.is_empty(), "no token ⇒ no events; got {events:?}");
    }

    #[tokio::test]
    async fn fetch_range_falls_back_when_ha_unavailable() {
        let settings = Settings {
            supervisor_token: None,
            max_events: 10,
            ..Settings::default()
        };
        let now = Utc::now();
        let events = fetch_range(&settings, now, now + Duration::days(30)).await;
        assert!(
            !events.is_empty(),
            "fetch_range should yield fallback events for dev"
        );
    }
}
