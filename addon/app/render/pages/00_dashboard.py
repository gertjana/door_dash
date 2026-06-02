"""Page 0 — the main dashboard.

This is the original "everything-at-a-glance" page: QR + indoor sensors
+ Tesla on the left, weather across the top right, calendar list filling
the remaining right column, with a date/refresh footer in the left
column. All layout geometry is owned by this module — when you tweak
the dashboard, this is the only file you need to touch.

Layout regions::

    +---------------------+------------------------------------+
    |  QR                 |  Weather                           |
    +---------------------+------------------------------------+
    |  Indoors            |  Calendar list                     |
    +---------------------+                                    |
    |  Tesla              |                                    |
    +---------------------+                                    |
    |  date+hh:mm refresh |                                    |
    +---------------------+------------------------------------+

The ``NN_`` numeric prefix on the filename drives the page order in the
registry; see ``addon/app/render/pages/__init__.py``.

This page implements the registry's ``Page`` interface — a single
``render(settings, sensors, fw_version)`` function returning a PIL image.
"""  # noqa: N999

from __future__ import annotations

from datetime import datetime
from typing import TYPE_CHECKING
from zoneinfo import ZoneInfo

from PIL import Image, ImageDraw

from ...sources import calendar as calendar_src
from ...sources import tesla as tesla_src
from ...sources import weather as weather_src
from ...sources.local_sensors import LocalSensors
from ..badge import draw_version_badge
from ..fonts import draw_crisp_text, font
from ..widgets import calendar_list, local_sensors, qr, tesla, weather
from ..widgets.base import Box

if TYPE_CHECKING:
    from ...config import Settings

TITLE = "Dashboard"

# === Layout constants =====================================================
# Page-specific geometry. Keeping these here rather than in a shared module
# makes it obvious that they only apply to this page; other pages own
# their own constants.
LEFT_COL_WIDTH = 260
WEATHER_ROW_HEIGHT = 124
# Reserved strip at the bottom of the left column for the date + refresh time.
FOOTER_HEIGHT = 38


def _draw_chrome(img: Image.Image, settings: Settings, weather_h: int) -> None:
    """Draw the column/row separator lines that frame the layout."""
    draw = ImageDraw.Draw(img)
    w, h = settings.width, settings.height
    # Vertical divider between left and right columns
    draw.line((LEFT_COL_WIDTH, 0, LEFT_COL_WIDTH, h - 1), fill=0, width=1)
    # Horizontal divider in the right column under the weather row
    draw.line((LEFT_COL_WIDTH, weather_h, w - 1, weather_h), fill=0, width=1)


def _hr(draw: ImageDraw.ImageDraw, left: Box, y: int) -> None:
    """Thin horizontal rule across the left column, slightly inset."""
    draw.line((left.x + 8, y, left.x + left.w - 8, y), fill=0, width=1)


def _draw_footer_timestamp(
    draw: ImageDraw.ImageDraw,
    settings: Settings,
    footer_top: int,
) -> None:
    """Bottom-left footer: today's date and the "Refreshed HH:MM" stamp."""
    try:
        tz = ZoneInfo(settings.timezone)
    except Exception:
        tz = ZoneInfo("UTC")
    now = datetime.now(tz)

    # Thin separator above the footer
    draw.line(
        (8, footer_top, LEFT_COL_WIDTH - 8, footer_top),
        fill=0,
        width=1,
    )

    # Top line: today's date, centred across the column width
    date_text = now.strftime("%A %d %B")  # e.g. "Monday 25 May"
    date_f = font(16, bold=True)
    date_bbox = draw.textbbox((0, 0), date_text, font=date_f)
    date_w = date_bbox[2] - date_bbox[0]
    date_x = (LEFT_COL_WIDTH - date_w) // 2
    draw.text((date_x, footer_top + 3), date_text, font=date_f, fill=0)

    # Bottom line: "Refreshed HH:MM", centred and crisp
    refreshed_text = "Refreshed " + now.strftime("%H:%M")
    refreshed_f = font(12)
    ref_bbox = draw.textbbox((0, 0), refreshed_text, font=refreshed_f)
    ref_w = ref_bbox[2] - ref_bbox[0]
    ref_x = (LEFT_COL_WIDTH - ref_w) // 2
    draw_crisp_text(draw, (ref_x, footer_top + 22), refreshed_text, refreshed_f)


def render(
    settings: Settings,
    sensors: LocalSensors | None = None,
    fw_version: str | None = None,
) -> Image.Image:
    """Lay out and draw the dashboard. Returns an 8-bit-grey PIL image.

    All page-specific layout decisions live here. The width/height are
    taken from ``settings`` so the page works on any panel size, not
    just the 800×480 of the E1001.
    """
    if sensors is None:
        sensors = LocalSensors()

    w, h = settings.width, settings.height
    img = Image.new("L", (w, h), color=255)

    _draw_chrome(img, settings, WEATHER_ROW_HEIGHT)

    main_h = h
    right_w = w - LEFT_COL_WIDTH
    # Left column reserves the bottom strip for the timestamp footer.
    left_widget_h = main_h - FOOTER_HEIGHT

    weather_box = Box(x=LEFT_COL_WIDTH, y=0, w=right_w, h=WEATHER_ROW_HEIGHT)
    calendar_box = Box(
        x=LEFT_COL_WIDTH,
        y=WEATHER_ROW_HEIGHT,
        w=right_w,
        h=main_h - WEATHER_ROW_HEIGHT,
    )
    left = Box(x=0, y=0, w=LEFT_COL_WIDTH, h=left_widget_h)

    show_sensors = settings.show_local_sensors
    show_tesla = settings.show_tesla

    # Left-column row weights (sum normalised to fill the available height).
    # When a section is disabled we just leave it out of the weight list,
    # so the remaining sections grow to fill the space.
    weights: list[tuple[str, float]] = [("qr", 0.46)]
    if show_sensors:
        weights.append(("sensors", 0.26))
    if show_tesla:
        weights.append(("tesla", 0.28))

    total_weight = sum(w_ for _, w_ in weights)
    heights = {name: int(left_widget_h * (w_ / total_weight)) for name, w_ in weights}
    # Push any rounding drift into the last section so we always fill the
    # column exactly — avoids a 1-2px gap above the footer separator.
    drift = left_widget_h - sum(heights.values())
    heights[weights[-1][0]] += drift

    drw = ImageDraw.Draw(img)
    y = 0
    boxes: dict[str, Box] = {}
    for i, (name, _) in enumerate(weights):
        boxes[name] = Box(x=left.x, y=y, w=left.w, h=heights[name])
        y += heights[name]
        if i < len(weights) - 1:
            _hr(drw, left, y)

    # === Render widgets =================================================
    qr.render(settings, img, boxes["qr"])
    weather.render(weather_src.fetch(settings), img, weather_box, settings)
    if show_sensors:
        local_sensors.render(sensors, img, boxes["sensors"])
    if show_tesla:
        tesla.render(tesla_src.fetch(settings), img, boxes["tesla"])
    calendar_list.render(settings, calendar_src.fetch(settings), img, calendar_box)

    # Refresh timestamp in the bottom-left footer strip.
    _draw_footer_timestamp(drw, settings, left_widget_h)

    # Tiny version badge in the top-right corner: "v0.1.0 · fw0.1.0".
    draw_version_badge(img, settings, fw_version, draw=drw)

    return img
