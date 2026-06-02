"""Page 2 — "Yes please Maureen".

Renders a large filled heart centred horizontally, with the romantic
prompt "Yes please Maureen" below it. Self-contained: no data sources,
no widgets — just direct PIL drawing.

The heart is drawn from primitives (two circles + a downward-pointing
triangle) rather than as a font glyph so we're not at the mercy of which
emoji-capable fonts happen to be available on the host, and so we get a
crisp 1-bit silhouette on the ePaper panel.
"""  # noqa: N999

from __future__ import annotations

from typing import TYPE_CHECKING

from PIL import Image, ImageDraw

from ..badge import draw_version_badge
from ..fonts import font

if TYPE_CHECKING:
    from ...config import Settings
    from ...sources.local_sensors import LocalSensors

TITLE = "Yes please Maureen"

# Layout tunables. The heart is sized as a fraction of the panel's
# shorter dimension so it stays well-proportioned on any panel size.
HEART_SIZE_FRACTION = 0.55  # of min(width, height)
PROMPT = "Yes please Maureen"
PROMPT_FONT_SIZE = 44
# Vertical gap between the bottom of the heart and the top of the prompt.
GAP_BELOW_HEART = 24


def _draw_heart(draw: ImageDraw.ImageDraw, cx: int, cy: int, size: int) -> None:
    """Draw a filled black heart centred on ``(cx, cy)`` with overall ``size``.

    Uses the classic parametric heart curve

        x(t) = 16 sin³(t)
        y(t) = 13 cos(t) − 5 cos(2t) − 2 cos(3t) − cos(4t)

    sampled at 360 points to produce a closed polygon. This yields a
    proper anatomical heart silhouette in one shape — no fusion seams
    between primitive circles + triangle, no inward "notches" to hide.
    """
    import math

    n = 360
    pts: list[tuple[int, int]] = []
    # First pass: compute raw (x, y) in curve coordinates so we can
    # measure the bounding box and rescale to the requested ``size``.
    raw: list[tuple[float, float]] = []
    for i in range(n):
        t = 2 * math.pi * i / n
        x = 16 * math.sin(t) ** 3
        # Curve's y axis points UP in math convention; we flip later.
        y = 13 * math.cos(t) - 5 * math.cos(2 * t) - 2 * math.cos(3 * t) - math.cos(4 * t)
        raw.append((x, y))

    xs = [p[0] for p in raw]
    ys = [p[1] for p in raw]
    raw_w = max(xs) - min(xs)
    raw_h = max(ys) - min(ys)
    # Scale so the heart's longer dimension matches ``size``.
    scale = size / max(raw_w, raw_h)
    raw_cx = (max(xs) + min(xs)) / 2
    raw_cy = (max(ys) + min(ys)) / 2

    for x, y in raw:
        # Centre on (raw_cx, raw_cy), scale, then flip Y so the heart's
        # apex points down on screen (PIL Y axis grows downward).
        px = int(cx + (x - raw_cx) * scale)
        py = int(cy - (y - raw_cy) * scale)
        pts.append((px, py))

    draw.polygon(pts, fill=0)


def render(
    settings: Settings,
    sensors: LocalSensors,
    fw_version: str | None = None,
) -> Image.Image:
    """Render the heart + prompt. Returns an 8-bit-grey PIL image."""
    w, h = settings.width, settings.height
    img = Image.new("L", (w, h), color=255)
    draw = ImageDraw.Draw(img)

    # Size the heart relative to the panel so the page looks right on
    # any display size (not just 800x480).
    heart_size = int(min(w, h) * HEART_SIZE_FRACTION)

    prompt_f = font(PROMPT_FONT_SIZE, bold=True)
    prompt_bbox = draw.textbbox((0, 0), PROMPT, font=prompt_f)
    prompt_w = prompt_bbox[2] - prompt_bbox[0]
    prompt_h = prompt_bbox[3] - prompt_bbox[1]

    # Vertically centre the heart+prompt group as a single unit, then
    # place the heart on top and the prompt below with a fixed gap.
    group_h = heart_size + GAP_BELOW_HEART + prompt_h
    group_top = (h - group_h) // 2

    heart_cx = w // 2
    heart_cy = group_top + heart_size // 2
    _draw_heart(draw, heart_cx, heart_cy, heart_size)

    prompt_x = (w - prompt_w) // 2
    prompt_y = group_top + heart_size + GAP_BELOW_HEART
    draw.text((prompt_x, prompt_y), PROMPT, font=prompt_f, fill=0)

    # Tiny version badge in the top-right corner, same as the other pages.
    draw_version_badge(img, settings, fw_version, draw=draw)

    return img
