"""Page 2 — placeholder for a future page.

Same skeleton as 01_week.py. Pick a real purpose and rename the file
(keeping the numeric prefix) when you're ready.
"""  # noqa: N999

from __future__ import annotations

from typing import TYPE_CHECKING

from PIL import Image, ImageDraw

from ... import __version__ as ADDON_VERSION  # noqa: N812
from ..fonts import draw_crisp_text, font

if TYPE_CHECKING:
    from ...config import Settings
    from ...sources.local_sensors import LocalSensors

TITLE = "Page 3"


def render(
    settings: Settings,
    sensors: LocalSensors,
    fw_version: str | None = None,
) -> Image.Image:
    w, h = settings.width, settings.height
    img = Image.new("L", (w, h), color=255)
    draw = ImageDraw.Draw(img)

    title_f = font(72, bold=True)
    bbox = draw.textbbox((0, 0), TITLE, font=title_f)
    tw, th = bbox[2] - bbox[0], bbox[3] - bbox[1]
    draw.text(((w - tw) // 2, (h - th) // 2 - 20), TITLE, font=title_f, fill=0)

    hint_f = font(18)
    hint = "page 2"
    bbox = draw.textbbox((0, 0), hint, font=hint_f)
    hw = bbox[2] - bbox[0]
    draw_crisp_text(
        draw,
        ((w - hw) // 2, (h - th) // 2 + th + 8),
        hint,
        hint_f,
        fill=0,
    )

    fw = fw_version or "?"
    badge = f"v{ADDON_VERSION} \u00b7 fw{fw}"
    bf = font(10)
    bbbox = draw.textbbox((0, 0), badge, font=bf)
    bw = bbbox[2] - bbbox[0]
    draw_crisp_text(draw, (w - bw - 4, 2), badge, bf, fill=0)

    return img
