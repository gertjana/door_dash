"""Indoors widget — onboard temp/humidity + battery state.

Layout is a 2x2 grid:

    +-----------------+-----------------+
    |  Temp           |  Humidity       |
    +-----------------+-----------------+
    |  Battery        |  (reserved)     |
    +-----------------+-----------------+

The bottom-right quadrant is intentionally left empty for a future widget.
All values are optional; missing readings render as an em-dash so cold boot
still produces sensible output.
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


def _draw_battery_bar(draw: ImageDraw.ImageDraw, x: int, y: int, w: int, h: int, pct) -> None:
    """Draw a battery-shaped bar with a small positive-terminal nub."""
    nub_w = 4
    body_w = w - nub_w
    draw.rectangle((x, y, x + body_w, y + h), outline=0, width=2)
    draw.rectangle((x + body_w, y + h // 4, x + body_w + nub_w, y + h - h // 4), fill=0)
    if pct is not None:
        inner_x = x + 3
        inner_y = y + 3
        inner_w = body_w - 6
        inner_h = h - 6
        fill_w = int(inner_w * max(0, min(100, pct)) / 100)
        if fill_w > 0:
            draw.rectangle((inner_x, inner_y, inner_x + fill_w, inner_y + inner_h), fill=0)


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

    # Quadrant geometry
    content_top = box.y + 32
    # Reserve a few pixels at the bottom so the battery % label doesn't kiss
    # the horizontal divider line drawn between widgets.
    bottom_pad = 8
    content_bottom = box.y + box.h - bottom_pad
    content_h = content_bottom - content_top
    half_w = box.w // 2
    half_h = content_h // 2

    # Quadrant origins (with a small inset for breathing room)
    inset_x = 10
    inset_y = 4
    tl_x = box.x + inset_x
    tr_x = box.x + half_w + inset_x - 6
    top_y = content_top + inset_y
    bot_y = content_top + half_h + inset_y

    # Top-left: temperature
    _draw_metric(draw, tl_x, top_y, "Temp", _fmt(sensors.indoor_temp, "°C", "{:.0f}"))

    # Top-right: humidity
    _draw_metric(draw, tr_x, top_y, "Hum.", _fmt(sensors.indoor_hum, "%", "{:.0f}"))

    # Bottom-left: battery bar (~80% of half-width) with % label beneath
    bar_w = int((half_w - inset_x) * 0.80)
    bar_h = 14
    bar_x = tl_x
    bar_y = bot_y + 4
    _draw_battery_bar(draw, bar_x, bar_y, bar_w, bar_h, sensors.battery_pct)

    pct_text = _fmt(sensors.battery_pct, "%", "{:.0f}")
    pct_f = font(14, bold=True)
    draw.text((bar_x, bar_y + bar_h + 4), pct_text, font=pct_f, fill=0)

    # Bottom-right: intentionally left empty (reserved for a future widget).
