"""Shared version-badge helper.

Every page draws a tiny ``vX.Y.Z · fwA.B.C`` badge in the top-right
corner so the running addon + firmware versions are visible at a glance.
A small battery bar with percentage is drawn to the left of the version
text when battery data is available.

The drawing primitive is identical across pages — only the page-specific
content above/around it changes — so it lives here rather than being
copy-pasted into each page module.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

from .. import __version__ as ADDON_VERSION  # noqa: N812
from .fonts import draw_crisp_text, font

if TYPE_CHECKING:
    from PIL import Image, ImageDraw

    from ..config import Settings


def _draw_mini_battery(
    draw: ImageDraw.ImageDraw, x: int, y: int, w: int, h: int, pct: float
) -> None:
    """Draw a tiny battery icon (outline + fill level) at the given position."""
    # Nub (positive terminal) on the right side
    nub_w = 2
    nub_h = h // 2
    nub_x = x + w
    nub_y = y + (h - nub_h) // 2
    draw.rectangle((nub_x, nub_y, nub_x + nub_w - 1, nub_y + nub_h - 1), fill=0)

    # Battery body outline
    draw.rectangle((x, y, x + w - 1, y + h - 1), outline=0)

    # Fill level (inset by 1px)
    fill_max_w = w - 2
    fill_w = max(1, int(fill_max_w * pct / 100))
    if fill_max_w > 0:
        draw.rectangle((x + 1, y + 1, x + 1 + fill_w - 1, y + h - 2), fill=0)


def draw_version_badge(
    img: Image.Image | None,
    settings: Settings,
    fw_version: str | None,
    *,
    battery_pct: float | None = None,
    draw: ImageDraw.ImageDraw | None = None,
) -> None:
    """Render the addon+firmware version badge in the top-right corner.

    Either ``img`` or ``draw`` must be provided; a ``draw`` argument
    avoids re-wrapping when the caller already has an ``ImageDraw``.

    When ``battery_pct`` is provided (0–100), a small battery bar and
    percentage label are drawn to the left of the version text.

    The badge is drawn on a small white pad so it remains legible if a
    page chooses to draw content right up to the canvas edge.
    """
    from PIL import ImageDraw as _ImageDraw  # local import keeps cold-start cheap

    if draw is None:
        if img is None:
            raise ValueError("draw_version_badge needs either img or draw")
        draw = _ImageDraw.Draw(img)

    fw = fw_version or "?"
    version_text = f"v{ADDON_VERSION} \u00b7 fw{fw}"
    f = font(10)
    bbox = draw.textbbox((0, 0), version_text, font=f)
    text_w = bbox[2] - bbox[0]
    text_h = bbox[3] - bbox[1]
    pad = 2
    y = 2

    # Battery indicator dimensions
    bat_bar_w = 14
    bat_bar_h = 10
    bat_nub_w = 2
    bat_gap = 3  # gap between battery section and version text
    bat_section_w = 0

    if battery_pct is not None:
        pct_label = f"{battery_pct:.0f}%"
        pct_bbox = draw.textbbox((0, 0), pct_label, font=f)
        pct_w = pct_bbox[2] - pct_bbox[0]
        bat_section_w = bat_bar_w + bat_nub_w + 2 + pct_w + bat_gap

    total_w = bat_section_w + text_w
    x = settings.width - total_w - 4

    # White pad so the badge stays readable over any underlying content.
    draw.rectangle(
        (x - pad, y - pad, x + total_w + pad, y + text_h + pad),
        fill=255,
    )

    # Draw battery bar + percentage if available
    if battery_pct is not None:
        # Top-aligned with the text baseline; the 2px white pad above
        # provides the visual breathing room from the canvas edge.
        bat_y = y
        _draw_mini_battery(draw, x, bat_y, bat_bar_w, bat_bar_h, battery_pct)
        pct_x = x + bat_bar_w + bat_nub_w + 2
        draw_crisp_text(draw, (pct_x, y), pct_label, f, fill=0)

    # Version text
    draw_crisp_text(draw, (x + bat_section_w, y), version_text, f, fill=0)


__all__ = ["draw_version_badge"]
