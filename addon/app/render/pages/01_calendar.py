"""Page 1 — full-screen month calendar.

Renders a 7-column × 6-row month grid for the *current* month, with
events from all configured HA calendars (``settings.calendar_entities``)
drawn as Outlook-style stacked bars inside each day cell. Week starts
Monday; today's day-number is bold.

Design notes
------------
* Fully self-contained: fetches its own events via ``sources.calendar``
  (no per-page data plumbing needed — keeps the page registry's signature
  uniform across pages).
* All sizing is derived from ``settings.width`` / ``settings.height`` so
  the page works on any panel size, not just the 800×480 of the E1001.
* On a 1-bit panel we have only black/white. We use two visual styles
  for event bars to keep things readable when stacked:
    - all-day  → filled black rectangle, white text
    - timed    → outlined rectangle, black text
  Days outside the displayed month are drawn with a smaller, regular-
  weight day-number to de-emphasize them without relying on greys.
"""  # noqa: N999

from __future__ import annotations

import calendar as pycal
import logging
from datetime import UTC, datetime, time, timedelta
from typing import TYPE_CHECKING
from zoneinfo import ZoneInfo

from PIL import Image, ImageDraw

from ... import __version__ as ADDON_VERSION  # noqa: N812
from ...sources.calendar import Event, fetch_range
from ..fonts import draw_crisp_text, font

if TYPE_CHECKING:
    from ...config import Settings
    from ...sources.local_sensors import LocalSensors

log = logging.getLogger(__name__)

TITLE = "Calendar — Month"

# Layout tunables (pixels). Derived sizes are computed in `render()`.
HEADER_HEIGHT = 56  # month/year title strip at the top
WEEKDAY_HEIGHT = 22  # Mon/Tue/... header row
CELL_PADDING = 3  # inner padding inside every day cell
EVENT_BAR_HEIGHT = 13  # height of a single event bar
EVENT_BAR_GAP = 2  # vertical gap between stacked event bars
GRID_LINE = 1  # 1px grid lines


def _month_grid_days(year: int, month: int) -> list[list[datetime]]:
    """Return a 6-row × 7-col grid of `datetime` (date-only, midnight UTC).

    Always 6 rows so the layout is stable regardless of which weekday the
    month starts on. Days outside the requested month are filled in from
    the surrounding months — the caller draws them in a muted style.
    Week starts Monday.
    """
    cal = pycal.Calendar(firstweekday=pycal.MONDAY)
    weeks: list[list[datetime]] = []
    for week in cal.monthdatescalendar(year, month):
        row = [datetime.combine(d, time.min, tzinfo=UTC) for d in week]
        weeks.append(row)
    # `monthdatescalendar` returns 4-6 weeks depending on the month;
    # pad to exactly 6 so the grid geometry is constant.
    while len(weeks) < 6:
        last_date = weeks[-1][-1].date()
        weeks.append(
            [
                datetime.combine(last_date + timedelta(days=i + 1), time.min, tzinfo=UTC)
                for i in range(7)
            ]
        )
    return weeks[:6]


def _events_for_day(events: list[Event], day: datetime, tz: ZoneInfo) -> list[Event]:
    """Filter events whose local-time interval intersects `day` (00:00-24:00 local)."""
    day_start_local = datetime.combine(day.date(), time.min, tzinfo=tz)
    day_end_local = day_start_local + timedelta(days=1)
    out: list[Event] = []
    for ev in events:
        ev_start = ev.start.astimezone(tz)
        # All-day events from HA come with end = next day midnight; treat
        # missing end as a point event.
        ev_end = (ev.end or ev.start).astimezone(tz)
        if ev_end > day_start_local and ev_start < day_end_local:
            out.append(ev)
    # Within a day, sort all-day first then by start time.
    out.sort(key=lambda e: (not e.all_day, e.start))
    return out


def _truncate_to_width(
    draw: ImageDraw.ImageDraw,
    text: str,
    font_obj,
    max_w: int,
) -> str:
    """Truncate `text` with an ellipsis so its rendered width fits `max_w`."""
    if draw.textlength(text, font=font_obj) <= max_w:
        return text
    ell = "…"
    # Binary-ish shrink — fine for short event titles.
    for n in range(len(text) - 1, 0, -1):
        candidate = text[:n].rstrip() + ell
        if draw.textlength(candidate, font=font_obj) <= max_w:
            return candidate
    return ell


def render(
    settings: Settings,
    sensors: LocalSensors,
    fw_version: str | None = None,
) -> Image.Image:
    w, h = settings.width, settings.height
    img = Image.new("L", (w, h), color=255)
    draw = ImageDraw.Draw(img)

    # Resolve "today" in the configured timezone so the grid centres on
    # the right month and "today" is highlighted correctly across the
    # midnight rollover regardless of UTC vs. local.
    try:
        tz = ZoneInfo(settings.timezone)
    except Exception:
        tz = UTC  # type: ignore[assignment]
    today_local = datetime.now(tz).date()
    year, month = today_local.year, today_local.month

    # === Header strip: month + year, centred ===
    title_text = f"{pycal.month_name[month]} {year}"
    title_f = font(36, bold=True)
    bbox = draw.textbbox((0, 0), title_text, font=title_f)
    tw = bbox[2] - bbox[0]
    th = bbox[3] - bbox[1]
    draw.text(((w - tw) // 2, (HEADER_HEIGHT - th) // 2 - 4), title_text, font=title_f, fill=0)

    # === Weekday header row (Mon..Sun) ===
    weekday_f = font(13, bold=True)
    col_w = w / 7  # float for accurate cell placement; floor when drawing
    wd_y = HEADER_HEIGHT
    weekday_names = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
    for i, name in enumerate(weekday_names):
        x_center = int(col_w * i + col_w / 2)
        bb = draw.textbbox((0, 0), name, font=weekday_f)
        nw = bb[2] - bb[0]
        nh = bb[3] - bb[1]
        draw_crisp_text(
            draw,
            (x_center - nw // 2, wd_y + (WEEKDAY_HEIGHT - nh) // 2 - 2),
            name,
            weekday_f,
            fill=0,
        )
    # Separator under the weekday row
    sep_y = HEADER_HEIGHT + WEEKDAY_HEIGHT
    draw.line([(0, sep_y), (w, sep_y)], fill=0, width=GRID_LINE)

    # === Grid geometry ===
    grid_top = sep_y + GRID_LINE
    grid_h = h - grid_top
    row_h = grid_h / 6

    # === Fetch all events for the displayed window ===
    # The displayed window is the 6-week grid (~42 days) starting on the
    # Monday on/before the 1st of the month. Fetch in UTC; we'll convert
    # to local time when assigning events to cells.
    grid = _month_grid_days(year, month)
    window_start_local = datetime.combine(grid[0][0].date(), time.min, tzinfo=tz)
    window_end_local = datetime.combine(
        (grid[-1][-1] + timedelta(days=1)).date(), time.min, tzinfo=tz
    )
    events = fetch_range(
        settings,
        window_start_local.astimezone(UTC),
        window_end_local.astimezone(UTC),
    )
    log.info(
        "Month view: %d events in window %s..%s",
        len(events),
        window_start_local.date(),
        window_end_local.date(),
    )

    # === Draw cells ===
    day_num_f = font(15, bold=False)
    day_num_bold_f = font(15, bold=True)
    day_num_muted_f = font(13, bold=False)
    event_f = font(10, bold=False)

    for r in range(6):
        for c in range(7):
            cell_x = int(col_w * c)
            cell_y = int(grid_top + row_h * r)
            cell_x2 = int(col_w * (c + 1))
            cell_y2 = int(grid_top + row_h * (r + 1))

            # Grid lines (right + bottom border per cell; left/top covered
            # by adjacent cells or the outer canvas).
            draw.line(
                [(cell_x2 - GRID_LINE, cell_y), (cell_x2 - GRID_LINE, cell_y2)],
                fill=0,
                width=GRID_LINE,
            )
            draw.line(
                [(cell_x, cell_y2 - GRID_LINE), (cell_x2, cell_y2 - GRID_LINE)],
                fill=0,
                width=GRID_LINE,
            )

            day = grid[r][c]
            in_month = day.month == month
            is_today = day.date() == today_local

            # Day number — top-left of the cell.
            day_str = str(day.day)
            if not in_month:
                day_font_obj = day_num_muted_f
            elif is_today:
                day_font_obj = day_num_bold_f
            else:
                day_font_obj = day_num_f
            draw_crisp_text(
                draw,
                (cell_x + CELL_PADDING + 1, cell_y + CELL_PADDING - 1),
                day_str,
                day_font_obj,
                fill=0,
            )

            # Event bars below the day number.
            day_events = _events_for_day(events, day, tz)
            if not day_events:
                continue

            # Available area for bars: below the day-number row.
            bar_area_top = cell_y + CELL_PADDING + 16  # 16 ≈ day-num line height
            bar_area_bottom = cell_y2 - CELL_PADDING - GRID_LINE
            bar_area_left = cell_x + CELL_PADDING
            bar_area_right = cell_x2 - CELL_PADDING - GRID_LINE
            bar_w = bar_area_right - bar_area_left
            available_h = bar_area_bottom - bar_area_top
            if available_h < EVENT_BAR_HEIGHT:
                continue
            max_bars = max(1, (available_h + EVENT_BAR_GAP) // (EVENT_BAR_HEIGHT + EVENT_BAR_GAP))

            shown = day_events[:max_bars]
            overflow = len(day_events) - len(shown)
            if overflow > 0:
                # Reserve the last bar slot for the "+N more" label.
                shown = day_events[: max_bars - 1] if max_bars > 1 else []

            by = bar_area_top
            for ev in shown:
                bx2 = bar_area_left + bar_w
                by2 = by + EVENT_BAR_HEIGHT
                title = ev.summary or "(no title)"
                if ev.all_day:
                    # Filled black bar, white text.
                    draw.rectangle([bar_area_left, by, bx2, by2], fill=0)
                    text_fill = 255
                    text_x = bar_area_left + 3
                else:
                    # Outlined bar, black text; show start time prefix.
                    draw.rectangle([bar_area_left, by, bx2, by2], outline=0, width=GRID_LINE)
                    text_fill = 0
                    text_x = bar_area_left + 3
                    local_start = ev.start.astimezone(tz)
                    title = f"{local_start.strftime('%H:%M')} {title}"
                # Truncate title to the bar's inner width.
                inner_w = bx2 - text_x - 2
                title = _truncate_to_width(draw, title, event_f, inner_w)
                # Vertically centre text within the bar (font is small,
                # rough pixel-perfect centring is fine on ePaper).
                draw_crisp_text(draw, (text_x, by - 1), title, event_f, fill=text_fill)
                by = by2 + EVENT_BAR_GAP

            if overflow > 0:
                more_text = f"+{overflow} more"
                more_text = _truncate_to_width(draw, more_text, event_f, bar_w - 4)
                draw_crisp_text(draw, (bar_area_left + 2, by - 1), more_text, event_f, fill=0)

    # Highlight today's cell border (subtle: just leave the day-number
    # bold which we already did — no extra cell decoration per the
    # user's "bold day number only" preference).

    # === Version badge (top-right, matching layout.py) ===
    fw = fw_version or "?"
    badge = f"v{ADDON_VERSION} \u00b7 fw{fw}"
    bf = font(10)
    bbbox = draw.textbbox((0, 0), badge, font=bf)
    bw = bbbox[2] - bbbox[0]
    # Place inside the header strip so it doesn't collide with the title.
    draw_crisp_text(draw, (w - bw - 4, 2), badge, bf, fill=0)

    return img
