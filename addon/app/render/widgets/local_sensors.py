"""Indoors widget — onboard temp/humidity + battery state.

Layout:

    +-----------------+-----------------+
    |  Temp           |  Humidity       |
    +-----------------+-----------------+
    |  [████████      ]  87%            |
    +-----------------+-----------------+

Top row holds the two readings side-by-side; the bottom strip is a
full-width battery bar with its percentage label to the right. All
values are optional; missing readings render as an em-dash so cold
boot still produces sensible output.
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
    """Draw a battery-shaped bar with a small positive-terminal nub.

    ``w`` is the *total* width including the nub, so callers can size
    the whole battery to a fixed region without separately accounting
    for the terminal.
    """
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

    # Geometry ----------------------------------------------------------
    # The widget is split into two horizontal strips:
    #   * top: temp + humidity side-by-side (the bulk of the height)
    #   * bottom: battery bar spanning the widget width
    # Side insets give the bar a little breathing room from the column
    # divider on the right and the canvas edge on the left.
    content_top = box.y + 32
    side_inset = 10
    bottom_pad = 8  # gap above the next widget's separator line

    # Battery row reserves a fixed strip at the bottom; everything above
    # it is for the temp/hum readings.
    battery_h = 16
    battery_y = box.y + box.h - bottom_pad - battery_h
    pct_label_w = 56  # room for "100%" in the bold label font

    # Top row: temp + humidity, evenly split
    half_w = box.w // 2
    tl_x = box.x + side_inset
    tr_x = box.x + half_w + side_inset - 6
    top_y = content_top + 4

    _draw_metric(draw, tl_x, top_y, "Temp", _fmt(sensors.indoor_temp, "°C", "{:.0f}"))
    _draw_metric(draw, tr_x, top_y, "Hum.", _fmt(sensors.indoor_hum, "%", "{:.0f}"))

    # Bottom: full-width battery bar + percentage label to the right.
    # Bar takes the available width minus the label column; label is
    # right-aligned within its column so the % digits line up nicely
    # regardless of whether the value is "9%" or "100%".
    bar_x = box.x + side_inset
    bar_right_limit = box.x + box.w - side_inset
    bar_w = bar_right_limit - bar_x - pct_label_w - 6  # 6px gap before label
    _draw_battery_bar(draw, bar_x, battery_y, bar_w, battery_h, sensors.battery_pct)

    pct_text = _fmt(sensors.battery_pct, "%", "{:.0f}")
    pct_f = font(14, bold=True)
    pct_bbox = draw.textbbox((0, 0), pct_text, font=pct_f)
    pct_w = pct_bbox[2] - pct_bbox[0]
    pct_h = pct_bbox[3] - pct_bbox[1]
    # Right-align label within its reserved column; vertically centre
    # against the bar.
    pct_x = bar_right_limit - pct_w
    pct_y = battery_y + (battery_h - pct_h) // 2 - 1
    draw_crisp_text(draw, (pct_x, pct_y), pct_text, pct_f)
