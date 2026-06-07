"""Page 4 — Energy + indoor environment.

Three top-to-bottom sections:

  1. Phase table — 3 rows × (label + L1 value + 24h sparkline with axes):
       Consumed (kW)
       Voltage  (V)
       Current  (A)
     Single-phase install, so no L2/L3 columns. Solar production is also
     omitted; reinstate when/if the install grows.

  2. Indoor row — Temperature + Humidity, big number plus 24h sparkline
     (also axis-labelled).

  3. Totals row — cumulative Tariff 1, Tariff 2, Gas counters.

All sparklines carry small axis labels:

  * Y-axis: y-min bottom-left and y-max top-left of the plot box, in
    the same numeric scale draw_sparkline actually mapped to. The unit
    is implied by the row label; numbers are unitless to keep the font
    small enough not to compete with the trace.
  * X-axis: 24h-ago / 12h-ago / now, all rendered as ``HH:MM`` in the
    user's resolved timezone. Labels reflect *current* clock time
    (a label of "14:30" on the left means the leftmost sample was
    taken at 14:30 yesterday); the leftmost and rightmost will read
    identically when the panel happens to refresh on a 24 h boundary,
    which is fine — the column header already says "Last 24 h".

All data comes from a single ``sources.energy.fetch`` call. Layout is
bespoke (not widget-based) — the table is dense enough that
parameterising existing widgets to handle the axis-labelled cells is
more code than just drawing it directly.
"""  # noqa: N999

from __future__ import annotations

from datetime import datetime, timedelta
from typing import TYPE_CHECKING

from PIL import Image, ImageDraw

from ...sources import energy as energy_src
from ...timezone import resolve_timezone
from ..badge import draw_version_badge
from ..fonts import draw_crisp_text, font
from ..sparkline import draw_sparkline
from ..widgets.base import Box

if TYPE_CHECKING:
    from collections.abc import Callable

    from ...config import Settings
    from ...sources.energy import EnergyState
    from ...sources.local_sensors import LocalSensors

TITLE = "Energy"

# === Layout constants =====================================================
# Vertical budget on a 480 px panel:
#   12 (top inset)
# + 28 (page title)
# + 22 + 3*62 (phase table: header + 3 rows) = 208
# + 10 + 1 + 10 (gap, rule, gap)             = 21
# + 22 + 2*44 (indoor: header + 2 rows)      = 110
# + 10 + 1 + 10 (gap, rule, gap)             = 21
# + 22 + 42 (totals: header + body)          = 64
# = 464, leaving ~16 px breathing room above the bottom edge.
SIDE_INSET = 20
TOP_INSET = 12

# Page header
PAGE_TITLE_H = 28

# Phase table — single-phase, so just one numeric column next to the
# label, with a generous sparkline cell on the right.
TABLE_HEADER_H = 22
TABLE_ROW_H = 62
TABLE_LABEL_W = 110
TABLE_VALUE_W = 110
SPARK_INSET = 4  # padding above the plot inside its cell

# Indoor section
INDOOR_HEADER_H = 22
INDOOR_ROW_H = 44
INDOOR_LABEL_W = 110
INDOOR_VALUE_W = 110

# Totals section
TOTALS_HEADER_H = 22
TOTALS_BODY_H = 42

# Axis label gutters inside a sparkline cell. Y-labels sit in a column
# at the left of the cell; X-labels sit on a row below the plot. Both
# are rendered at font(11) which is ~8-9 px cap-height + 2-3 px descender.
#
# Sizes are deliberately a bit roomy: ``AXIS_Y_W`` accommodates 5-char
# labels like ``"1,774"`` plus the ``AXIS_Y_LABEL_GAP`` between label
# and plot. ``AXIS_X_H`` includes the ``AXIS_X_LABEL_GAP`` separation
# from the sparkline's bottom-baseline rule, so labels don't visually
# stick to the plot border.
AXIS_Y_W = 38
AXIS_X_H = 16
AXIS_LABEL_PT = 11
AXIS_Y_LABEL_GAP = 6  # horizontal gap from y-label right edge to plot.x
AXIS_X_LABEL_GAP = 4  # vertical gap from sparkline baseline to x-label top
# Inset y-min/y-max labels slightly inward from the plot's vertical
# extremes so they don't sit literally on the top edge or descend into
# the bottom baseline rule. Values are in pixels.
AXIS_Y_TOP_INSET = 1
AXIS_Y_BOTTOM_INSET = 3

# Vertical gap between sections (top + horizontal rule + bottom).
SECTION_GAP = 10


# === Value formatters =====================================================
# These format full strings with units for the big numeric value cells.
# Each value column is ~110 px wide at font(16, bold=True), leaving
# headroom for "12,345 W" (8 chars) without truncation.


def _fmt_power(v: float | None) -> str:
    """kW input -> compact W or kW string."""
    if v is None:
        return "—"
    watts = v * 1000.0
    # Break point at 10 kW: residential single-phase very rarely exceeds
    # this even when EV-charging, so the W form is the dominant case.
    if abs(watts) >= 10_000:
        return f"{v:.2f} kW"
    return f"{watts:,.0f} W"


def _fmt_voltage(v: float | None) -> str:
    if v is None:
        return "—"
    return f"{v:.1f} V"


def _fmt_current(v: float | None) -> str:
    if v is None:
        return "—"
    return f"{v:.2f} A"


def _fmt_kwh(v: float | None) -> str:
    if v is None:
        return "—"
    return f"{v:,.1f} kWh"


def _fmt_gas(v: float | None) -> str:
    if v is None:
        return "—"
    return f"{v:,.3f} m\u00b3"


def _fmt_temp(v: float | None) -> str:
    if v is None:
        return "—"
    return f"{v:.1f} \u00b0C"


def _fmt_pct(v: float | None) -> str:
    if v is None:
        return "—"
    return f"{round(v)} %"


# === Axis label formatters ================================================
# Same numeric conventions as the value formatters but without units, so
# the small axis labels read as bare numbers. Unit context comes from the
# row label ("Consumed" → kW, "Voltage" → V, etc.).


def _axis_power(v: float) -> str:
    """kW input -> bare number (W if small, kW if ≥10 kW)."""
    watts = v * 1000.0
    if abs(watts) >= 10_000:
        return f"{v:.1f}k"
    return f"{watts:,.0f}"


def _axis_voltage(v: float) -> str:
    return f"{v:.1f}"


def _axis_current(v: float) -> str:
    return f"{v:.2f}"


def _axis_temp(v: float) -> str:
    return f"{v:.1f}"


def _axis_pct(v: float) -> str:
    return f"{round(v)}"


# === Time helpers =========================================================


def _x_labels(settings: Settings) -> tuple[str, str, str]:
    """Return (24h-ago, 12h-ago, now) clock-time labels for the x-axis.

    All three are formatted ``HH:MM`` in the resolved timezone (HA's
    ``/api/config`` if available, falling back to the addon option).
    The leftmost and rightmost will read identically when the panel
    refreshes exactly at the hour boundary — that's expected, since
    the window is by definition the last 24 h, and the column header
    already labels it as such.
    """
    tz = resolve_timezone(settings)
    now = datetime.now(tz)
    return (
        (now - timedelta(hours=24)).strftime("%H:%M"),
        (now - timedelta(hours=12)).strftime("%H:%M"),
        now.strftime("%H:%M"),
    )


# === Sparkline cell with axis labels ======================================


def _draw_sparkline_with_axes(
    img: Image.Image,
    draw: ImageDraw.ImageDraw,
    cell: Box,
    points: list[float | None],
    *,
    include_zero: bool,
    y_fmt: Callable[[float], str],
    x_labels: tuple[str, str, str],
    top_inset: int = SPARK_INSET,
) -> None:
    """Draw a sparkline inside ``cell`` with y-axis labels on the left
    and x-axis labels along the bottom.

    The plot itself goes into a sub-box of ``cell`` that excludes the
    label gutters, so the trace never touches the labels. ``y_fmt``
    formats the y-min/y-max labels (no units). ``x_labels`` is
    ``(left, mid, right)`` already-formatted strings.

    When the sparkline reports no usable data range (entity unconfigured
    or all-None history), we skip the y-labels but still render the
    x-labels — the time axis is meaningful regardless.
    """
    plot = Box(
        x=cell.x + AXIS_Y_W + 2,
        y=cell.y + top_inset,
        w=max(1, cell.w - AXIS_Y_W - 4),
        h=max(1, cell.h - top_inset - AXIS_X_H - 1),
    )
    rng = draw_sparkline(img, plot, points, include_zero=include_zero)

    label_f = font(AXIS_LABEL_PT)

    # Y-axis labels (top-left = max, bottom-left = min). They sit in
    # the AXIS_Y_W column to the left of the plot, right-aligned
    # against the plot edge so multi-digit and single-digit values
    # don't visually drift apart in adjacent rows.
    #
    # Vertically the labels are inset a few pixels from the plot's
    # extremes — ``AXIS_Y_TOP_INSET`` keeps the top label off the very
    # top edge (where the trace can also reach), and
    # ``AXIS_Y_BOTTOM_INSET`` lifts the bottom label clear of the
    # ``draw_sparkline`` baseline rule so digit descenders don't
    # collide with it.
    if rng is not None:
        y_min, y_max = rng
        for value, y_pos in (
            (y_max, plot.y + AXIS_Y_TOP_INSET),
            (y_min, plot.y + plot.h - AXIS_LABEL_PT - AXIS_Y_BOTTOM_INSET),
        ):
            text = y_fmt(value)
            tb = draw.textbbox((0, 0), text, font=label_f)
            tw = tb[2] - tb[0]
            draw_crisp_text(
                draw,
                (plot.x - AXIS_Y_LABEL_GAP - tw, y_pos),
                text,
                label_f,
                fill=0,
            )

    # X-axis labels along the bottom: left-aligned start, centred mid,
    # right-aligned end. ``AXIS_X_LABEL_GAP`` keeps them clear of the
    # baseline rule above. Always drawn so the page reads as a chart
    # even when there's no trace data yet.
    left_lbl, mid_lbl, right_lbl = x_labels
    label_y = plot.y + plot.h + AXIS_X_LABEL_GAP

    draw_crisp_text(draw, (plot.x, label_y), left_lbl, label_f, fill=0)

    mid_bb = draw.textbbox((0, 0), mid_lbl, font=label_f)
    mid_w = mid_bb[2] - mid_bb[0]
    draw_crisp_text(
        draw,
        (plot.x + (plot.w - mid_w) // 2, label_y),
        mid_lbl,
        label_f,
        fill=0,
    )

    right_bb = draw.textbbox((0, 0), right_lbl, font=label_f)
    right_w = right_bb[2] - right_bb[0]
    draw_crisp_text(
        draw,
        (plot.x + plot.w - right_w, label_y),
        right_lbl,
        label_f,
        fill=0,
    )


# === Section drawers ======================================================


def _draw_page_title(
    draw: ImageDraw.ImageDraw,
    x: int,
    y: int,
) -> None:
    """Big bold "Energy" title at the top of the page."""
    f = font(22, bold=True)
    draw.text((x, y), "Energy", font=f, fill=0)


def _draw_phase_table(
    draw: ImageDraw.ImageDraw,
    img: Image.Image,
    state: EnergyState,
    x: int,
    y: int,
    w: int,
    x_labels: tuple[str, str, str],
) -> int:
    """Draw the 3-row × (label + value + 24h graph) table.

    Returns the y-coordinate just below the bottom border so the caller
    can stack the next section under it without recomputing offsets.
    """
    label_x = x
    value_x = x + TABLE_LABEL_W
    graph_x = value_x + TABLE_VALUE_W
    graph_w = max(0, w - (graph_x - x))

    # === Header row =======================================================
    head_f = font(13, bold=True)
    head_baseline = y + (TABLE_HEADER_H - 13) // 2

    # Single-phase, so the value column header is just "L1" — gives the
    # reader the same visual signpost as the old multi-phase layout
    # ("which leg is this?") in case the install ever grows.
    bbox = draw.textbbox((0, 0), "L1", font=head_f)
    tw = bbox[2] - bbox[0]
    draw_crisp_text(
        draw,
        (value_x + (TABLE_VALUE_W - tw) // 2, head_baseline),
        "L1",
        head_f,
        fill=0,
    )

    graph_label = "Last 24 h"
    bbox = draw.textbbox((0, 0), graph_label, font=head_f)
    tw = bbox[2] - bbox[0]
    draw_crisp_text(
        draw,
        (graph_x + (graph_w - tw) // 2, head_baseline),
        graph_label,
        head_f,
        fill=0,
    )

    # Header underline.
    draw.line(
        (x, y + TABLE_HEADER_H - 1, x + w - 1, y + TABLE_HEADER_H - 1),
        fill=0,
        width=1,
    )

    # === Data rows ========================================================
    # (label, value, history, include_zero, value_fmt, axis_fmt)
    rows: list[
        tuple[
            str,
            float | None,
            list[float | None],
            bool,
            Callable[[float | None], str],
            Callable[[float], str],
        ]
    ] = [
        (
            "Consumed",
            state.power_consumed,
            state.history_power_consumed,
            True,
            _fmt_power,
            _axis_power,
        ),
        (
            "Voltage",
            state.voltage,
            state.history_voltage,
            False,
            _fmt_voltage,
            _axis_voltage,
        ),
        (
            "Current",
            state.current,
            state.history_current,
            True,
            _fmt_current,
            _axis_current,
        ),
    ]

    label_f = font(15, bold=True)
    value_f = font(16, bold=True)

    row_top = y + TABLE_HEADER_H

    for i, (label, value, hist, include_zero, val_fmt, ax_fmt) in enumerate(rows):
        ry = row_top + i * TABLE_ROW_H

        # Row label (left-aligned, vertically centred).
        lb = draw.textbbox((0, 0), label, font=label_f)
        lh = lb[3] - lb[1]
        draw.text(
            (label_x + 6, ry + (TABLE_ROW_H - lh) // 2 - 1),
            label,
            font=label_f,
            fill=0,
        )

        # L1 numeric value (centred in its column).
        text = val_fmt(value)
        tb = draw.textbbox((0, 0), text, font=value_f)
        tw = tb[2] - tb[0]
        th = tb[3] - tb[1]
        draw.text(
            (value_x + (TABLE_VALUE_W - tw) // 2, ry + (TABLE_ROW_H - th) // 2 - 1),
            text,
            font=value_f,
            fill=0,
        )

        # Sparkline cell with axis labels.
        cell = Box(
            x=graph_x,
            y=ry,
            w=graph_w,
            h=TABLE_ROW_H,
        )
        _draw_sparkline_with_axes(
            img,
            draw,
            cell,
            hist,
            include_zero=include_zero,
            y_fmt=ax_fmt,
            x_labels=x_labels,
        )

        # Inter-row separator (skip after the last row — bottom border
        # is drawn as a single rule below).
        if i < len(rows) - 1:
            sep_y = ry + TABLE_ROW_H
            draw.line((x, sep_y, x + w - 1, sep_y), fill=0, width=1)

    # === Borders ==========================================================
    table_top = y + TABLE_HEADER_H
    table_bot = row_top + len(rows) * TABLE_ROW_H

    # Outer top border is drawn implicitly by the page's section separator
    # above the table; bottom border closes the table here.
    draw.line((x, table_bot, x + w - 1, table_bot), fill=0, width=1)

    # Vertical separators between columns (start below the header underline
    # so we don't double up on that line).
    for sep_x in (value_x, graph_x):
        draw.line((sep_x, table_top, sep_x, table_bot), fill=0, width=1)

    return table_bot


def _draw_indoor(
    draw: ImageDraw.ImageDraw,
    img: Image.Image,
    state: EnergyState,
    x: int,
    y: int,
    w: int,
    x_labels: tuple[str, str, str],
) -> int:
    """Indoor temperature + humidity rows with 24h sparklines."""
    head_f = font(15, bold=True)
    draw_crisp_text(draw, (x, y), "Indoor", head_f, fill=0)
    body_top = y + INDOOR_HEADER_H

    rows: list[
        tuple[
            str,
            str,
            list[float | None],
            Callable[[float], str],
        ]
    ] = [
        (
            "Temperature",
            _fmt_temp(state.indoor_temp),
            state.history_indoor_temp,
            _axis_temp,
        ),
        (
            "Humidity",
            _fmt_pct(state.indoor_humidity),
            state.history_indoor_humidity,
            _axis_pct,
        ),
    ]

    label_f = font(13)
    value_f = font(20, bold=True)

    graph_x = x + INDOOR_LABEL_W + INDOOR_VALUE_W + 12
    graph_w = max(1, w - (graph_x - x))

    for i, (label, value, hist, ax_fmt) in enumerate(rows):
        ry = body_top + i * INDOOR_ROW_H

        # Label (left).
        lb = draw.textbbox((0, 0), label, font=label_f)
        lh = lb[3] - lb[1]
        draw_crisp_text(
            draw,
            (x + 4, ry + (INDOOR_ROW_H - lh) // 2),
            label,
            label_f,
            fill=0,
        )

        # Value (right-aligned within its column for tabular feel).
        vb = draw.textbbox((0, 0), value, font=value_f)
        vw = vb[2] - vb[0]
        vh = vb[3] - vb[1]
        draw.text(
            (
                x + INDOOR_LABEL_W + INDOOR_VALUE_W - vw - 4,
                ry + (INDOOR_ROW_H - vh) // 2 - 1,
            ),
            value,
            font=value_f,
            fill=0,
        )

        # Sparkline cell with axis labels. Indoor swings are small so
        # auto-scale (include_zero=False) — zero is uninteresting for
        # both temp and humidity.
        cell = Box(x=graph_x, y=ry, w=graph_w, h=INDOOR_ROW_H)
        _draw_sparkline_with_axes(
            img,
            draw,
            cell,
            hist,
            include_zero=False,
            y_fmt=ax_fmt,
            x_labels=x_labels,
            top_inset=2,
        )

    return body_top + len(rows) * INDOOR_ROW_H


def _draw_totals(
    draw: ImageDraw.ImageDraw,
    state: EnergyState,
    x: int,
    y: int,
    w: int,
) -> int:
    """T1 / T2 / Gas counters in three centred columns."""
    head_f = font(15, bold=True)
    draw_crisp_text(draw, (x, y), "Totals", head_f, fill=0)
    body_top = y + TOTALS_HEADER_H

    cols: list[tuple[str, str]] = [
        ("Tariff 1 (low)", _fmt_kwh(state.energy_tariff1)),
        ("Tariff 2 (high)", _fmt_kwh(state.energy_tariff2)),
        ("Gas", _fmt_gas(state.gas)),
    ]

    n = len(cols)
    col_w = w // n
    label_f = font(12)
    value_f = font(20, bold=True)

    for i, (label, value) in enumerate(cols):
        cx = x + i * col_w

        # Label centred above value.
        lb = draw.textbbox((0, 0), label, font=label_f)
        lw = lb[2] - lb[0]
        draw_crisp_text(
            draw,
            (cx + (col_w - lw) // 2, body_top),
            label,
            label_f,
            fill=0,
        )

        # Big value below.
        vb = draw.textbbox((0, 0), value, font=value_f)
        vw = vb[2] - vb[0]
        draw.text(
            (cx + (col_w - vw) // 2, body_top + 14),
            value,
            font=value_f,
            fill=0,
        )

    return body_top + TOTALS_BODY_H


def render(
    settings: Settings,
    sensors: LocalSensors,  # noqa: ARG001 — page contract; unused on this page
    fw_version: str | None = None,
) -> Image.Image:
    """Compose the energy page. Returns a PIL ``L``-mode (8-bit grey) image
    that the rest of the pipeline thresholds to 1-bit at output time.
    """
    w, h = settings.width, settings.height
    img = Image.new("L", (w, h), color=255)
    draw = ImageDraw.Draw(img)

    state = energy_src.fetch(settings)
    x_labels = _x_labels(settings)

    inset_x = SIDE_INSET
    avail_w = w - 2 * SIDE_INSET

    cy = TOP_INSET

    # Page title.
    _draw_page_title(draw, inset_x, cy)
    cy += PAGE_TITLE_H

    # Section 1: phase table (single-phase consumption stats).
    cy = _draw_phase_table(draw, img, state, inset_x, cy, avail_w, x_labels)
    cy += SECTION_GAP

    # Horizontal rule between sections — keeps the dense page from feeling
    # like one giant blob of text.
    draw.line((inset_x, cy, inset_x + avail_w, cy), fill=0, width=1)
    cy += SECTION_GAP

    # Section 2: indoor temp + humidity.
    cy = _draw_indoor(draw, img, state, inset_x, cy, avail_w, x_labels)
    cy += SECTION_GAP

    draw.line((inset_x, cy, inset_x + avail_w, cy), fill=0, width=1)
    cy += SECTION_GAP

    # Section 3: cumulative totals.
    _draw_totals(draw, state, inset_x, cy, avail_w)

    # Tiny version badge in the top-right corner: matches the other pages.
    draw_version_badge(img, settings, fw_version, draw=draw)

    return img
