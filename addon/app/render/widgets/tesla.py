"""Finn McCool widget — Tesla status.

Title line shows an A/C indicator (small snowflake) when the car's climate
system is on (and only when we know — hidden when the car is asleep).

Two stacked lines under the title:
  - Battery percentage  ·  range in km
  - Cabin (inside) temperature

Each value shows an em-dash when unavailable.
"""

from __future__ import annotations

from PIL import Image, ImageDraw

from ...sources.tesla import TeslaState
from ..fonts import font
from .base import Box

NAME = "Finn McCool"


def _draw_snowflake(draw: ImageDraw.ImageDraw, cx: int, cy: int, r: int) -> None:
    """Tiny 6-arm snowflake centred on (cx, cy)."""
    import math

    for i in range(6):
        angle = math.radians(i * 60)
        x2 = cx + int(r * math.cos(angle))
        y2 = cy + int(r * math.sin(angle))
        draw.line((cx, cy, x2, y2), fill=0, width=1)
    # Small dot in centre for crispness on ePaper.
    draw.rectangle((cx - 1, cy - 1, cx + 1, cy + 1), fill=0)


def render(state: TeslaState, img: Image.Image, box: Box) -> None:
    draw = ImageDraw.Draw(img)

    title_f = font(20, bold=True)
    title_x = box.x + 8
    title_y = box.y + 4
    draw.text((title_x, title_y), NAME, font=title_f, fill=0)

    # A/C indicator: only render when we know climate is on. None (asleep) and
    # False (explicitly off) both render nothing.
    if state.climate_on is True:
        title_w = draw.textlength(NAME, font=title_f)
        cx = int(title_x + title_w + 14)
        cy = title_y + 12
        _draw_snowflake(draw, cx, cy, 8)

    line_f = font(18, bold=True)
    line_x = box.x + 10
    line_y = box.y + 30
    line_gap = 22

    pct_text = "—" if state.battery_pct is None else f"{round(state.battery_pct)}%"
    range_text = "—" if state.range_km is None else f"{round(state.range_km)} km"
    temp_text = "—" if state.inside_temp_c is None else f"{round(state.inside_temp_c)}°C cabin"

    draw.text((line_x, line_y), f"{pct_text}  ·  {range_text}", font=line_f, fill=0)
    draw.text((line_x, line_y + line_gap), temp_text, font=line_f, fill=0)
