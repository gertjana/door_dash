"""Indoors widget — onboard temp/humidity readings.

Layout:

    +-----------------+-----------------+
    |  Temp           |  Humidity       |
    +-----------------+-----------------+

A single row of two side-by-side metrics under an "Indoors" title.
The display battery used to live below the metrics here, but it's now
shown in the version badge in the top-right corner of every page, so
the widget shrinks to just the indoor climate readings. Missing
values render as an em-dash so cold boot still produces sensible
output.
"""

from __future__ import annotations

from PIL import Image, ImageDraw

from ...sources.local_sensors import LocalSensors
from ..fonts import draw_crisp_text, font
from .base import Box


def _fmt(value, suffix: str, fmt: str = "{:.0f}") -> str:
    if value is None:
        return "—"
    return fmt.format(value) + suffix


def _draw_metric(
    draw: ImageDraw.ImageDraw,
    x: int,
    y: int,
    label: str,
    value: str,
) -> None:
    """Render a labeled metric (label small on top, value big below)."""
    label_f = font(13)
    value_f = font(22, bold=True)
    draw_crisp_text(draw, (x, y), label, label_f)
    draw.text((x, y + 14), value, font=value_f, fill=0)


def render(sensors: LocalSensors, img: Image.Image, box: Box) -> None:
    draw = ImageDraw.Draw(img)

    # Title
    title_f = font(20, bold=True)
    draw.text((box.x + 8, box.y + 4), "Indoors", font=title_f, fill=0)

    # Geometry — single row of two metrics, evenly split across the
    # widget width. Side insets give the values a little breathing
    # room from the column divider on the right and the canvas edge
    # on the left.
    content_top = box.y + 32
    side_inset = 10
    half_w = box.w // 2
    tl_x = box.x + side_inset
    tr_x = box.x + half_w + side_inset - 6
    top_y = content_top + 4

    _draw_metric(draw, tl_x, top_y, "Temp", _fmt(sensors.indoor_temp, "°C", "{:.0f}"))
    _draw_metric(draw, tr_x, top_y, "Hum.", _fmt(sensors.indoor_hum, "%", "{:.0f}"))
