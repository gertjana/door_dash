"""Compact energy widget for the dashboard left column.

Shows a "Power" title and the current consumed power as a single large
value. Intentionally minimal — the full energy page (04_energy.py) has
the sparklines and detailed breakdown; this widget is just the at-a-glance
number for the main dashboard.
"""

from __future__ import annotations

from PIL import Image, ImageDraw

from ...sources.energy import EnergyState
from ..fonts import font
from .base import Box

TITLE = "Energy"


def _fmt_power(v: float | None) -> str:
    """kW input → compact W or kW string, same convention as the energy page."""
    if v is None:
        return "—"
    watts = v * 1000.0
    if abs(watts) >= 10_000:
        return f"{v:.2f} kW"
    return f"{watts:,.0f} W"


def render(state: EnergyState, img: Image.Image, box: Box) -> None:
    """Draw the mini energy widget into *box* on *img*."""
    draw = ImageDraw.Draw(img)

    title_f = font(20, bold=True)
    draw.text((box.x + 8, box.y + 4), TITLE, font=title_f, fill=0)

    value_f = font(18)
    value_text = f"Power = {_fmt_power(state.power_consumed)}"
    draw.text((box.x + 10, box.y + 30), value_text, font=value_f, fill=0)
