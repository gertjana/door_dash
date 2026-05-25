"""Finn McCool widget — battery percentage for the Tesla.

Title + large percentage. Lightning bolt next to the title while charging.
Falls back to em-dash when the entity isn't configured or unreachable.
"""

from __future__ import annotations

from PIL import Image, ImageDraw

from ...sources.tesla import TeslaState
from ..fonts import font
from .base import Box

NAME = "Finn McCool"
RANGE_AT_FULL_KM = 423


def _draw_bolt(draw: ImageDraw.ImageDraw, x: int, y: int, size: int) -> None:
    """Small lightning bolt to indicate charging."""
    s = size
    pts = [
        (x + s * 0.55, y),
        (x + s * 0.1, y + s * 0.55),
        (x + s * 0.45, y + s * 0.55),
        (x + s * 0.30, y + s),
        (x + s * 0.85, y + s * 0.40),
        (x + s * 0.55, y + s * 0.40),
        (x + s * 0.75, y),
    ]
    draw.polygon(pts, fill=0)


def render(state: TeslaState, img: Image.Image, box: Box) -> None:
    draw = ImageDraw.Draw(img)

    title_f = font(20, bold=True)
    title_x = box.x + 8
    title_y = box.y + 4
    draw.text((title_x, title_y), NAME, font=title_f, fill=0)

    if state.charging:
        # Place bolt just to the right of the title text
        title_w = draw.textlength(NAME, font=title_f)
        _draw_bolt(draw, int(title_x + title_w + 6), title_y + 2, 16)

    if state.battery_pct is None:
        pct_text = "—"
    else:
        pct = round(state.battery_pct)
        km = round(RANGE_AT_FULL_KM * state.battery_pct / 100)
        pct_text = f"{pct}%  ·  {km} km"
    pct_f = font(22, bold=True)
    draw.text((box.x + 10, box.y + 30), pct_text, font=pct_f, fill=0)
