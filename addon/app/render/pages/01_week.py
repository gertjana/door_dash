"""Page 1 — Calendar / week view.

Placeholder for now: renders the page title centred on a blank canvas
plus the standard version badge in the corner. Content will be filled in
once the page navigation plumbing is verified end-to-end.
"""  # noqa: N999

from __future__ import annotations

from typing import TYPE_CHECKING

from PIL import Image, ImageDraw

from ... import __version__ as ADDON_VERSION  # noqa: N812
from ..fonts import draw_crisp_text, font

if TYPE_CHECKING:
    from ...config import Settings
    from ...sources.local_sensors import LocalSensors

TITLE = "Calendar — Week"


def render(
    settings: Settings,
    sensors: LocalSensors,
    fw_version: str | None = None,
) -> Image.Image:
    w, h = settings.width, settings.height
    img = Image.new("L", (w, h), color=255)
    draw = ImageDraw.Draw(img)

    # Centred page title — big, bold, anti-aliased (large enough that AA
    # thresholding doesn't eat thin strokes).
    title_f = font(72, bold=True)
    bbox = draw.textbbox((0, 0), TITLE, font=title_f)
    tw, th = bbox[2] - bbox[0], bbox[3] - bbox[1]
    draw.text(((w - tw) // 2, (h - th) // 2 - 20), TITLE, font=title_f, fill=0)

    # Subtle hint about the page index so we can tell at a glance which
    # button mapping landed us here during firmware debugging.
    hint_f = font(18)
    hint = "page 1"
    bbox = draw.textbbox((0, 0), hint, font=hint_f)
    hw = bbox[2] - bbox[0]
    draw_crisp_text(
        draw,
        ((w - hw) // 2, (h - th) // 2 + th + 8),
        hint,
        hint_f,
        fill=0,
    )

    # Version badge in the top-right, matching layout.py.
    fw = fw_version or "?"
    badge = f"v{ADDON_VERSION} \u00b7 fw{fw}"
    bf = font(10)
    bbbox = draw.textbbox((0, 0), badge, font=bf)
    bw = bbbox[2] - bbbox[0]
    bh = bbbox[3] - bbbox[1]
    draw_crisp_text(draw, (w - bw - 4, 2), badge, bf, fill=0)
    _ = bh  # silence unused; kept for symmetry with layout.py

    return img
