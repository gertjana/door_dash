"""Font loader. Prefers bundled Inter; falls back to system fonts.

ePaper note: this is a 1-bit display. Anti-aliased glyphs at small sizes
produce grey edges that get thresholded to black/white in `to_mono`, which
eats thin strokes (descenders, dots on `i`, etc.). For small text we draw
without AA via `draw_crisp_text()`; for big titles AA still looks fine
because the thresholded edges are wider than 1px.
"""

from __future__ import annotations

import logging
from functools import lru_cache
from pathlib import Path

from PIL import ImageDraw, ImageFont

log = logging.getLogger(__name__)

ASSETS = Path(__file__).parent / "assets" / "fonts"

# Order matters: first existing wins. Bundled Inter is the canonical choice
# so rendering is identical across dev (macOS) and prod (Alpine in HA).
CANDIDATES_REGULAR = [
    ASSETS / "Inter-Regular.ttf",
    # macOS dev fallback
    Path("/System/Library/Fonts/Supplemental/Arial.ttf"),
    # Alpine ttf-dejavu (HA addon container)
    Path("/usr/share/fonts/dejavu/DejaVuSans.ttf"),
    # Debian/Ubuntu fallback
    Path("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"),
]
CANDIDATES_BOLD = [
    ASSETS / "Inter-Bold.ttf",
    Path("/System/Library/Fonts/Supplemental/Arial Bold.ttf"),
    Path("/usr/share/fonts/dejavu/DejaVuSans-Bold.ttf"),
    Path("/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"),
]


def _pick(paths) -> Path | None:
    for p in paths:
        if p.exists():
            return p
    return None


_warned = False


@lru_cache(maxsize=64)
def font(size: int, bold: bool = False) -> ImageFont.ImageFont:
    global _warned
    path = _pick(CANDIDATES_BOLD if bold else CANDIDATES_REGULAR)
    if path is None:
        if not _warned:
            log.warning(
                "No TTF font found; falling back to PIL bitmap default. "
                "Sizes will be ignored. Bundle assets/fonts/Inter-{Regular,Bold}.ttf."
            )
            _warned = True
        return ImageFont.load_default()
    return ImageFont.truetype(str(path), size=size)


# Common font roles used across widgets. Tweak in one place to retune
# the visual hierarchy.
TITLE_SIZE = 20
BODY_SIZE = 14
SMALL_SIZE = 13  # bumped from 12 for ePaper readability


def title_font() -> ImageFont.ImageFont:
    return font(TITLE_SIZE, bold=True)


def body_font(bold: bool = False) -> ImageFont.ImageFont:
    return font(BODY_SIZE, bold=bold)


def small_font(bold: bool = False) -> ImageFont.ImageFont:
    return font(SMALL_SIZE, bold=bold)


def draw_crisp_text(
    draw: ImageDraw.ImageDraw,
    xy: tuple[int, int],
    text: str,
    font_obj: ImageFont.ImageFont,
    fill: int = 0,
) -> None:
    """Draw text without anti-aliasing.

    Sets `draw.fontmode = "1"` before drawing so glyphs render as pure
    black/white pixels — no grey edges. This is the right mode for small
    text on a 1-bit ePaper panel; AA edges otherwise get thresholded
    unpredictably and break thin strokes.

    Restores the previous fontmode afterwards so we don't affect other draws.
    """
    prev = getattr(draw, "fontmode", "L")
    draw.fontmode = "1"
    try:
        draw.text(xy, text, font=font_obj, fill=fill)
    finally:
        draw.fontmode = prev
