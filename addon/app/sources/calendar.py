"""Calendar source: reads upcoming events from HA calendar entities.

Returns up to `settings.max_events` chronologically sorted events, merged
across all configured calendars.
"""

from __future__ import annotations

from dataclasses import dataclass
from datetime import UTC, datetime, timedelta

from dateutil import parser as dtparser

from ..config import Settings
from ..ha_client import HAClient


@dataclass
class Event:
    start: datetime
    end: datetime | None
    summary: str
    all_day: bool
    location: str | None = None


def _parse(value: str | None) -> datetime | None:
    if not value:
        return None
    try:
        dt = dtparser.parse(value)
        if dt.tzinfo is None:
            dt = dt.replace(tzinfo=UTC)
        return dt
    except (ValueError, TypeError):
        return None


def _clean(text: str | None) -> str | None:
    """Collapse runs of whitespace (incl. newlines) to single spaces.

    Calendar fields routinely contain multi-line text — addresses with
    embedded newlines, descriptions with carriage returns. Pillow's
    `textlength` can't measure multi-line strings, so we always normalise.
    """
    if not text:
        return None
    cleaned = " ".join(text.split())
    return cleaned or None


def _to_event(raw: dict) -> Event | None:
    summary = _clean(raw.get("summary")) or "(no title)"
    location = _clean(raw.get("location"))
    start_raw = raw.get("start", {})
    end_raw = raw.get("end", {})

    # HA returns either {"dateTime": "..."} or {"date": "YYYY-MM-DD"} for all-day
    all_day = "date" in start_raw and "dateTime" not in start_raw
    start = _parse(start_raw.get("dateTime") or start_raw.get("date"))
    end = _parse(end_raw.get("dateTime") or end_raw.get("date"))
    if not start:
        return None
    return Event(start=start, end=end, summary=summary, all_day=all_day, location=location)


_FALLBACK_EVENTS = None


def _fallback(settings: Settings) -> list[Event]:
    now = datetime.now(UTC)
    # Match HA's calendar API conventions so the fallback exercises the
    # same code paths real data does:
    #   * all-day events use an EXCLUSIVE end at midnight of the day after
    #     the last all-day day (a one-day "Holiday" on May 3 has
    #     start=2026-05-03 00:00, end=2026-05-04 00:00).
    #   * timed events have an explicit end ~1h after start.
    today_midnight = datetime.combine(now.date(), datetime.min.time(), tzinfo=UTC)
    samples = [
        (
            now + timedelta(hours=3),
            now + timedelta(hours=4),
            "Dentist appointment",
            False,
            "Tandartspraktijk Centrum",
        ),
        (
            now + timedelta(days=1, hours=2),
            now + timedelta(days=1, hours=3),
            "Work: Team standup",
            False,
            "Online",
        ),
        (
            now + timedelta(days=1, hours=11),
            now + timedelta(days=1, hours=13),
            "Dinner with Anna",
            False,
            "Restaurant De Kas",
        ),
        (
            now + timedelta(days=2, hours=1),
            now + timedelta(days=2, hours=2),
            "Family: School run",
            False,
            None,
        ),
        (
            now + timedelta(days=2, hours=7),
            now + timedelta(days=2, hours=8),
            "Work: 1:1 with manager",
            False,
            "Office, Room 3.14",
        ),
        # All-day "Holiday" on (today+3). End is EXCLUSIVE — midnight of (today+4).
        (
            today_midnight + timedelta(days=3),
            today_midnight + timedelta(days=4),
            "Holiday",
            True,
            None,
        ),
        (
            now + timedelta(days=4, hours=3),
            now + timedelta(days=4, hours=4),
            "Health: Doctor",
            False,
            "Huisartsenpraktijk",
        ),
        (
            now + timedelta(days=5, hours=5),
            now + timedelta(days=5, hours=6),
            "Lunch in town",
            False,
            "Café Brecht",
        ),
    ]
    return [
        Event(start=s, end=e, summary=t, all_day=ad, location=loc) for (s, e, t, ad, loc) in samples
    ][: settings.max_events]


def fetch(settings: Settings) -> list[Event]:
    """Fetch upcoming events for the dashboard's "Upcoming" list.

    Returns an empty list when HA is unreachable or returns no events —
    the calendar_list widget renders a "No upcoming events." message in
    that case. The previous behaviour returned a hand-rolled sample set
    (``_fallback``), which made it impossible to tell at a glance whether
    the calendar integration was broken or genuinely empty.

    ``fetch_range()`` (used by the month grid page) still falls back to
    sample data when HA is unavailable so dev runs without a HA backend
    are visually meaningful — the month grid would look entirely broken
    if it had no data at all.
    """
    ha = HAClient(settings)
    if not ha.available:
        return []

    now = datetime.now(UTC)
    end_window = now + timedelta(days=30)
    start_iso = now.isoformat()
    end_iso = end_window.isoformat()

    events: list[Event] = []
    for entity in settings.calendar_entities:
        raw_events = ha.get_calendar(entity, start_iso, end_iso)
        for raw in raw_events:
            ev = _to_event(raw)
            if ev:
                events.append(ev)

    events.sort(key=lambda e: e.start)
    return events[: settings.max_events]


def fetch_range(settings: Settings, start: datetime, end: datetime) -> list[Event]:
    """Fetch all events from all configured calendars within [start, end].

    Unlike ``fetch()`` this does not cap at ``settings.max_events`` — the
    caller (e.g. a full-month grid renderer) usually wants every event in
    the window. Returns a fallback set only if there is no HA connection;
    an empty real result from HA is returned as-is (an empty list).
    """
    ha = HAClient(settings)
    if not ha.available:
        # For a month view the existing _fallback (next-week-ish events)
        # is fine for dev; clip to the requested window so we don't
        # render events outside the displayed range.
        fb = _fallback(settings)
        return [e for e in fb if start <= e.start <= end]

    start_iso = start.isoformat()
    end_iso = end.isoformat()
    events: list[Event] = []
    for entity in settings.calendar_entities:
        raw_events = ha.get_calendar(entity, start_iso, end_iso)
        for raw in raw_events:
            ev = _to_event(raw)
            if ev:
                events.append(ev)
    events.sort(key=lambda e: e.start)
    return events
