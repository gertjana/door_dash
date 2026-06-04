"""Page 4 — Energy + indoor environment.

Three top-to-bottom sections:

  1. Phase table — 4 rows × (label + L1 + L2 + L3 + 24h sparkline)
       Power Produced (kW)
       Power Consumed (kW)
       Voltage        (V)
       Current        (A)
       The sparkline column shows the L1 trace only (matches the
       original spec); per-phase values still appear as numbers.

  2. Indoor row — Temperature + Humidity, big number plus 24h sparkline.

  3. Totals row — cumulative Tariff 1, Tariff 2, Gas counters.

All data comes from a single ``sources.energy.fetch`` call. No widgets
are reused; the layout is dense enough that bespoke drawing code is
clearer than parameterising existing widgets to handle a 4-column table.
"""  # noqa: N999

from __future__ import annotations

from typing import TYPE_CHECKING

from PIL import Image, ImageDraw

from ...sources import energy as energy_src
from ..badge import draw_version_badge
from ..fonts import draw_crisp_text, font
from ..sparkline import draw_sparkline
from ..widgets.base import Box

if TYPE_CHECKING:
    from collections.abc import Callable

    from ...config import Settings
    from ...sources.energy import EnergyState, PhaseValues
    from ...sources.local_sensors import LocalSensors

TITLE = "Energy"

# === Layout constants =====================================================
# All vertical sums must fit within settings.height (480 on the E1001).
# Total budget below: 12 + 28 + 232 + 12 + 1 + 12 + 102 + 12 + 1 + 12 + 64 = 488,
# but the totals section drops to ~46 px in practice (small label + one big
# value) so we land at ~470, leaving a few px of breathing room above the
# bottom edge.
SIDE_INSET = 20
TOP_INSET = 12

# Page header
PAGE_TITLE_H = 28

# Phase table
TABLE_HEADER_H = 22
TABLE_ROW_H = 50
TABLE_LABEL_W = 100
TABLE_PHASE_W = 84  # each of L1, L2, L3
SPARK_INSET = 6  # padding around sparkline within its cell

# Indoor section
INDOOR_HEADER_H = 22
INDOOR_ROW_H = 38
INDOOR_LABEL_W = 110
INDOOR_VALUE_W = 130

# Totals section
TOTALS_HEADER_H = 22
TOTALS_BODY_H = 42

# Vertical gap between sections (top + horizontal rule + bottom).
SECTION_GAP = 10


# === Formatters ===========================================================
# Each value column is ~84 px wide at font(16, bold=True), so we cap each
# string to ~7-8 characters for safety. Format chosen per quantity:
#   power: small in W (no decimals), large in kW (2 decimals)
#   voltage: 1 decimal V
#   current: 2 decimals A
#   energy: 1 decimal kWh, thousands separator
#   gas: 3 decimals m³ (smart meters report mm³ resolution)


def _fmt_power(v: float | None) -> str:
    """kW input -> compact W or kW string."""
    if v is None:
        return "—"
    watts = v * 1000.0
    # Break point at 10 kW: residential phases very rarely exceed this even
    # when EV-charging on three phases, so the W form is the dominant case.
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
) -> int:
    """Draw the 4-row × (label + L1 + L2 + L3 + 24h graph) table.

    Returns the y-coordinate just below the bottom border.
    """
    label_x = x
    l1_x = x + TABLE_LABEL_W
    l2_x = l1_x + TABLE_PHASE_W
    l3_x = l2_x + TABLE_PHASE_W
    graph_x = l3_x + TABLE_PHASE_W
    graph_w = max(0, w - (graph_x - x))

    # === Header row =======================================================
    head_f = font(13, bold=True)
    head_baseline = y + (TABLE_HEADER_H - 13) // 2

    for col_x, col_w, label in (
        (l1_x, TABLE_PHASE_W, "L1"),
        (l2_x, TABLE_PHASE_W, "L2"),
        (l3_x, TABLE_PHASE_W, "L3"),
    ):
        bbox = draw.textbbox((0, 0), label, font=head_f)
        tw = bbox[2] - bbox[0]
        draw_crisp_text(
            draw,
            (col_x + (col_w - tw) // 2, head_baseline),
            label,
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
    # (label, phase-values, history, include_zero, formatter)
    rows: list[tuple[str, PhaseValues, list[float | None], bool, Callable[[float | None], str]]] = [
        ("Produced", state.power_produced, state.history_power_produced_l1, True, _fmt_power),
        ("Consumed", state.power_consumed, state.history_power_consumed_l1, True, _fmt_power),
        ("Voltage", state.voltage, state.history_voltage_l1, False, _fmt_voltage),
        ("Current", state.current, state.history_current_l1, True, _fmt_current),
    ]

    label_f = font(15, bold=True)
    value_f = font(16, bold=True)

    row_top = y + TABLE_HEADER_H

    for i, (label, vals, hist, include_zero, fmt) in enumerate(rows):
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

        # L1 / L2 / L3 numeric values (centred per column).
        for col_x, col_w, v in (
            (l1_x, TABLE_PHASE_W, vals.l1),
            (l2_x, TABLE_PHASE_W, vals.l2),
            (l3_x, TABLE_PHASE_W, vals.l3),
        ):
            text = fmt(v)
            tb = draw.textbbox((0, 0), text, font=value_f)
            tw = tb[2] - tb[0]
            th = tb[3] - tb[1]
            draw.text(
                (col_x + (col_w - tw) // 2, ry + (TABLE_ROW_H - th) // 2 - 1),
                text,
                font=value_f,
                fill=0,
            )

        # Sparkline cell (L1 trace).
        sb = Box(
            x=graph_x + SPARK_INSET,
            y=ry + SPARK_INSET,
            w=max(1, graph_w - 2 * SPARK_INSET),
            h=max(1, TABLE_ROW_H - 2 * SPARK_INSET),
        )
        draw_sparkline(img, sb, hist, include_zero=include_zero)

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
    for sep_x in (l1_x, l2_x, l3_x, graph_x):
        draw.line((sep_x, table_top, sep_x, table_bot), fill=0, width=1)

    return table_bot


def _draw_indoor(
    draw: ImageDraw.ImageDraw,
    img: Image.Image,
    state: EnergyState,
    x: int,
    y: int,
    w: int,
) -> int:
    """Indoor temperature + humidity rows with 24h sparklines."""
    head_f = font(15, bold=True)
    draw_crisp_text(draw, (x, y), "Indoor", head_f, fill=0)
    body_top = y + INDOOR_HEADER_H

    rows: list[tuple[str, str, list[float | None]]] = [
        ("Temperature", _fmt_temp(state.indoor_temp), state.history_indoor_temp),
        ("Humidity", _fmt_pct(state.indoor_humidity), state.history_indoor_humidity),
    ]

    label_f = font(13)
    value_f = font(20, bold=True)

    graph_x = x + INDOOR_LABEL_W + INDOOR_VALUE_W + 12
    graph_w = max(1, w - (graph_x - x))

    for i, (label, value, hist) in enumerate(rows):
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

        # Sparkline (auto-scale; indoor swings are small so 0 is uninteresting).
        sb = Box(x=graph_x, y=ry + 4, w=graph_w, h=INDOOR_ROW_H - 8)
        draw_sparkline(img, sb, hist, include_zero=False)

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

    inset_x = SIDE_INSET
    avail_w = w - 2 * SIDE_INSET

    cy = TOP_INSET

    # Page title.
    _draw_page_title(draw, inset_x, cy)
    cy += PAGE_TITLE_H

    # Section 1: phase table.
    cy = _draw_phase_table(draw, img, state, inset_x, cy, avail_w)
    cy += SECTION_GAP

    # Horizontal rule between sections — keeps the dense page from feeling
    # like one giant blob of text.
    draw.line((inset_x, cy, inset_x + avail_w, cy), fill=0, width=1)
    cy += SECTION_GAP

    # Section 2: indoor temp + humidity.
    cy = _draw_indoor(draw, img, state, inset_x, cy, avail_w)
    cy += SECTION_GAP

    draw.line((inset_x, cy, inset_x + avail_w, cy), fill=0, width=1)
    cy += SECTION_GAP

    # Section 3: cumulative totals.
    _draw_totals(draw, state, inset_x, cy, avail_w)

    # Tiny version badge in the top-right corner: matches the other pages.
    draw_version_badge(img, settings, fw_version, draw=draw)

    return img
