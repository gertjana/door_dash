"""Sparkline rendering for the 1-bit ePaper canvas.

A sparkline is a small inline chart showing the trajectory of a value over
a window of time. Each input point is one resampled bucket; ``None`` means
the value was unknown for that bucket and renders as a gap in the line.

Y-axis behaviour:

* ``include_zero=True`` — clamp y-min to 0. Use for power and current
  where zero is a meaningful reference (you want to see "the line went
  up from idle" rather than auto-zoom into the noise floor).
* ``include_zero=False`` — auto-zoom to the data's own min/max with a
  one-pixel margin top and bottom. Use for voltage (~230 V steady) and
  indoor temperature where the absolute scale is uninteresting.

Drawing is done with ``ImageDraw.line`` at ``fill=0`` (black). The 1-bit
threshold in ``image_io.to_mono`` keeps the line crisp; we never draw at
intermediate grey values because they get thresholded unpredictably.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

from PIL import ImageDraw

if TYPE_CHECKING:
    from PIL import Image

    from .widgets.base import Box


def draw_sparkline(
    img: Image.Image,
    box: Box,
    points: list[float | None],
    *,
    fill: int = 0,
    include_zero: bool = False,
    show_baseline: bool = True,
) -> None:
    """Draw ``points`` as a polyline inside ``box`` (in place).

    Args:
        img: target Pillow image. Drawn in place.
        box: rectangle to render into. Line stays inside (with 1 px margin
            top/bottom so it never touches the box edge).
        points: y-values (oldest-to-newest). ``None`` entries are gaps.
        fill: line colour. ``0`` = black, ``255`` = white. Default 0.
        include_zero: clamp y-min to 0 (use for power/current).
        show_baseline: draw a thin bottom rule across the box so the
            sparkline reads as a chart even when the line happens to sit
            near the top of the cell.
    """
    draw = ImageDraw.Draw(img)
    x0, y0 = box.x, box.y
    w, h = box.w, box.h
    if w <= 1 or h <= 1:
        return

    # Subtle bottom rule. Top rule is omitted on purpose — adding both
    # makes the cell read like a heavy table border, which clashes with
    # the actual table rules drawn by the page above.
    if show_baseline:
        draw.line((x0, y0 + h - 1, x0 + w - 1, y0 + h - 1), fill=fill, width=1)

    valid = [p for p in points if p is not None]
    if not valid:
        # No data yet (entity unconfigured, or just powered up). Draw a
        # short dashed line through the middle so the cell isn't visually
        # empty but is clearly distinguishable from a real flat trace.
        midy = y0 + h // 2
        for dx in range(0, w, 4):
            draw.point((x0 + dx, midy), fill=fill)
        return

    y_min = min(valid)
    y_max = max(valid)
    if include_zero:
        y_min = min(0.0, y_min)
        y_max = max(0.0, y_max)

    # Constant-series special case (e.g. voltage flat-lined at 230.0).
    # Auto-scaling would map the single value to the bottom edge by
    # convention; centring it is more useful and clearly says "no
    # variation in this window".
    if y_max <= y_min:
        flat_y = y0 + h // 2
        n = len(points)
        prev_xy: tuple[int, int] | None = None
        for i, p in enumerate(points):
            if p is None:
                prev_xy = None
                continue
            x = x0 + int(i * (w - 1) / max(1, n - 1))
            if prev_xy is None:
                draw.point((x, flat_y), fill=fill)
            else:
                draw.line((prev_xy[0], prev_xy[1], x, flat_y), fill=fill, width=1)
            prev_xy = (x, flat_y)
        return

    span = y_max - y_min
    pad = 1  # 1-px margin top + bottom so the line doesn't touch the box edge
    plot_h = max(1, h - 2 * pad)
    n = len(points)

    prev_xy = None
    for i, p in enumerate(points):
        if p is None:
            # Gap in the data — break the line so we don't draw across a
            # region with no real samples.
            prev_xy = None
            continue
        x = x0 + int(i * (w - 1) / max(1, n - 1))
        # Map p in [y_min, y_max] -> y in [y0+pad, y0+pad+plot_h-1] inverted
        # (top of box = y_max; bottom of box = y_min).
        norm = (p - y_min) / span
        y = y0 + pad + int((1.0 - norm) * (plot_h - 1))
        if prev_xy is None:
            # Single-pixel point so the trace doesn't disappear at the
            # start, or when isolated between gaps.
            draw.point((x, y), fill=fill)
        else:
            draw.line((prev_xy[0], prev_xy[1], x, y), fill=fill, width=1)
        prev_xy = (x, y)


__all__ = ["draw_sparkline"]
