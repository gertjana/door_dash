"""Shared version-badge helper.

Every page draws a tiny ``vX.Y.Z · fwA.B.C`` badge in the top-right
corner so the running addon + firmware versions are visible at a glance.
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


def draw_version_badge(
    img: Image.Image | None,
    settings: Settings,
    fw_version: str | None,
    *,
    draw: ImageDraw.ImageDraw | None = None,
) -> None:
    """Render the addon+firmware version badge in the top-right corner.

    Either ``img`` or ``draw`` must be provided; a ``draw`` argument
    avoids re-wrapping when the caller already has an ``ImageDraw``.

    The badge is drawn on a small white pad so it remains legible if a
    page chooses to draw content right up to the canvas edge.
    """
    from PIL import ImageDraw as _ImageDraw  # local import keeps cold-start cheap

    if draw is None:
        if img is None:
            raise ValueError("draw_version_badge needs either img or draw")
        draw = _ImageDraw.Draw(img)

    fw = fw_version or "?"
    text = f"v{ADDON_VERSION} \u00b7 fw{fw}"
    f = font(10)
    bbox = draw.textbbox((0, 0), text, font=f)
    text_w = bbox[2] - bbox[0]
    text_h = bbox[3] - bbox[1]
    pad = 2
    x = settings.width - text_w - 4
    y = 2
    # White pad so the badge stays readable over any underlying content.
    draw.rectangle(
        (x - pad, y - pad, x + text_w + pad, y + text_h + pad),
        fill=255,
    )
    draw_crisp_text(draw, (x, y), text, f, fill=0)


__all__ = ["draw_version_badge"]
