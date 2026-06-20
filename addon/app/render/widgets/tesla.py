"""Finn McCool widget — Tesla status.

Shows the car name as a title, then a single line with battery percentage
and range. Cabin temperature and climate-on indicator have been removed.

Each value shows an em-dash when unavailable.
"""

from __future__ import annotations

from PIL import Image, ImageDraw

from ...sources.tesla import TeslaState
from ..fonts import font
from .base import Box

NAME = "Finn McCool"


def render(state: TeslaState, img: Image.Image, box: Box) -> None:
    draw = ImageDraw.Draw(img)

    title_f = font(20, bold=True)
    title_x = box.x + 8
    title_y = box.y + 4
    draw.text((title_x, title_y), NAME, font=title_f, fill=0)

    line_f = font(18, bold=True)
    line_x = box.x + 10
    line_y = box.y + 30

    pct_text = "—" if state.battery_pct is None else f"{round(state.battery_pct)}%"
    range_text = "—" if state.range_km is None else f"{round(state.range_km)} km"

    draw.text((line_x, line_y), f"{pct_text}  ·  {range_text}", font=line_f, fill=0)
