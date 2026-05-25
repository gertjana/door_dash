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
    samples = [
        (now + timedelta(hours=3), "Dentist appointment", False, "Tandartspraktijk Centrum"),
        (now + timedelta(days=1, hours=2), "Work: Team standup", False, "Online"),
        (now + timedelta(days=1, hours=11), "Dinner with Anna", False, "Restaurant De Kas"),
        (now + timedelta(days=2, hours=1), "Family: School run", False, None),
        (now + timedelta(days=2, hours=7), "Work: 1:1 with manager", False, "Office, Room 3.14"),
        (now + timedelta(days=3), "Holiday", True, None),
        (now + timedelta(days=4, hours=3), "Health: Doctor", False, "Huisartsenpraktijk"),
        (now + timedelta(days=5, hours=5), "Lunch in town", False, "Café Brecht"),
    ]
    return [
        Event(start=s, end=s + timedelta(hours=1), summary=t, all_day=ad, location=loc)
        for (s, t, ad, loc) in samples
    ][: settings.max_events]


def fetch(settings: Settings) -> list[Event]:
    ha = HAClient(settings)
    if not ha.available:
        return _fallback(settings)

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

    if not events:
        return _fallback(settings)

    events.sort(key=lambda e: e.start)
    return events[: settings.max_events]
