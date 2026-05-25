"""Calendar list widget — two-line entries with date / optional tag / title / location.

Per-entry layout:

    +-------+------+----------------------------+
    | date  | TAG  | title                      |
    |       |      | location (smaller font)    |
    +-------+------+----------------------------+

* date         "16 May", or "Today" / "Tomorrow" for the next two days
* TAG          if the summary contains a colon, the part before the colon is
               displayed as an inverted rounded-corner pill, and stripped
               from the rendered title
* location     optional, rendered on the second line in a smaller font
"""

from __future__ import annotations

from datetime import UTC, datetime
from zoneinfo import ZoneInfo

from PIL import Image, ImageDraw

from ...config import Settings
from ...sources.calendar import Event
from ..fonts import draw_crisp_text, font
from .base import Box

# Column widths inside the right pane
DATE_COL_W = 92
GAP_AFTER_DATE = 6
GAP_AFTER_TAG = 8


def _format_date(start: datetime, tz: ZoneInfo, now: datetime) -> str:
    start_local = start.astimezone(tz)
    now_local = now.astimezone(tz)
    delta_days = (start_local.date() - now_local.date()).days
    if delta_days == 0:
        return "Today"
    if delta_days == 1:
        return "Tomorrow"
    # e.g. "16 May" — strip leading zero on day-of-month
    return start_local.strftime("%d %b").lstrip("0")


def _format_time(ev: Event, tz: ZoneInfo) -> str:
    if ev.all_day:
        return "all day"
    return ev.start.astimezone(tz).strftime("%H:%M")


def _split_tag(summary: str) -> tuple[str | None, str]:
    """If the summary contains a colon, return (tag, remainder); else (None, summary).

    Only treats the prefix as a tag when it looks like a short label —
    2..14 chars, no whitespace — so things like "1:1 with manager" or
    timestamps are left alone.
    """
    if ":" in summary:
        tag, _, rest = summary.partition(":")
        tag = tag.strip()
        rest = rest.strip()
        if tag and rest and 2 <= len(tag) <= 14 and not any(ch.isspace() for ch in tag):
            return tag, rest
    return None, summary


def _draw_tag_pill(
    draw: ImageDraw.ImageDraw,
    x: int,
    y: int,
    label: str,
    f,
) -> int:
    """Draw an inverted rounded-corner pill containing `label`. Returns its width."""
    pad_x = 6
    pad_y = 2
    text_w = int(draw.textlength(label, font=f))
    text_h = 14  # approx for the small bold font we use
    pill_w = text_w + pad_x * 2
    pill_h = text_h + pad_y * 2
    radius = pill_h // 2
    # Filled rounded rectangle (black background, white text)
    draw.rounded_rectangle(
        (x, y, x + pill_w, y + pill_h),
        radius=radius,
        fill=0,
    )
    # Position text vertically — ascender-aware nudge
    draw_crisp_text(draw, (x + pad_x, y + pad_y - 1), label, f, fill=255)
    return pill_w


def _truncate(draw: ImageDraw.ImageDraw, text: str, f, max_w: int) -> str:
    if draw.textlength(text, font=f) <= max_w:
        return text
    out = text
    while out and draw.textlength(out + "…", font=f) > max_w:
        out = out[:-1]
    return out + "…"


def render(settings: Settings, events: list[Event], img: Image.Image, box: Box) -> None:
    draw = ImageDraw.Draw(img)

    title_f = font(20, bold=True)
    draw.text((box.x + 12, box.y + 4), "Upcoming", font=title_f, fill=0)
    draw.line((box.x + 12, box.y + 32, box.x2 - 12, box.y + 32), fill=0, width=1)

    if not events:
        draw.text((box.x + 12, box.y + 48), "No upcoming events.", font=font(16), fill=0)
        return

    try:
        tz = ZoneInfo(settings.timezone)
    except Exception:
        tz = ZoneInfo("UTC")
    now = datetime.now(UTC)

    date_f = font(15, bold=True)
    time_f = font(13)
    title_f_row = font(16, bold=True)
    tag_f = font(12, bold=True)
    loc_f = font(11)

    row_h = 36  # two-line entry height
    row_gap = 4
    y = box.y + 42

    for ev in events[: settings.max_events]:
        if y + row_h > box.y2 - 2:
            break

        # --- Column 1: date (top) + time (bottom)
        date_text = _format_date(ev.start, tz, now)
        time_text = _format_time(ev, tz)
        draw_crisp_text(draw, (box.x + 12, y), date_text, date_f)
        draw_crisp_text(draw, (box.x + 12, y + 17), time_text, time_f)

        # --- Column 2+3: tag pill + title on line 1, location on line 2
        content_x = box.x + 12 + DATE_COL_W + GAP_AFTER_DATE
        content_max_x = box.x2 - 10
        avail_w = content_max_x - content_x

        tag, title_rest = _split_tag(ev.summary)
        cursor_x = content_x
        if tag:
            pill_w = _draw_tag_pill(draw, cursor_x, y + 1, tag, tag_f)
            cursor_x += pill_w + GAP_AFTER_TAG

        # Title (truncated to remaining width on this row)
        title_max_w = max(0, content_max_x - cursor_x)
        title_text = _truncate(draw, title_rest, title_f_row, title_max_w)
        draw_crisp_text(draw, (cursor_x, y), title_text, title_f_row)

        # Location line (smaller, dimmer — but we only have black/white,
        # so we just use a smaller font; left-align under the title column).
        if ev.location:
            # Calendar location strings often contain newlines (multi-line
            # postal addresses). Pillow's textlength can't measure multi-line
            # text, so collapse all whitespace runs to single spaces first.
            loc_one_line = " ".join(ev.location.split())
            loc_text = _truncate(draw, loc_one_line, loc_f, avail_w)
            draw_crisp_text(draw, (content_x, y + 19), loc_text, loc_f)

        y += row_h + row_gap
