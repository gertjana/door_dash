"""Energy / DSMR P1 source.

Single-phase, consumption-only model: this house only has L1 and no
solar inverter, so we don't fan out per-phase or model production.
Reads instantaneous L1 values for power consumed, voltage and current;
the cumulative tariff and gas counters; and indoor temp/humidity. Also
fetches a 24-hour history strip for the values that get sparklines on
the energy page (L1 power consumed, L1 voltage, L1 current, indoor
temp, indoor humidity).

Follows the same shape as ``tesla.py`` / ``weather.py``:

    @dataclass EnergyState
    fetch(settings) -> EnergyState

Returns a synthesized fallback when HA is unreachable so dev renders
always have plausible-looking data.
"""

from __future__ import annotations

import logging
import math
from collections.abc import Callable
from dataclasses import dataclass, field
from datetime import UTC, datetime, timedelta

from ..config import Settings
from ..ha_client import HAClient

log = logging.getLogger(__name__)

# 24-hour window resampled onto a uniform grid. 96 buckets = 15 min each,
# which gives ~3-4 px per bucket on a ~380 px wide sparkline cell — coarse
# enough to be readable on a 1-bit panel without losing the daily shape.
HISTORY_BUCKETS = 96
HISTORY_HOURS = 24


@dataclass
class EnergyState:
    """Snapshot of P1 + indoor environment values for the energy page.

    All instantaneous fields refer to L1 only; this addon does not model
    multi-phase or solar production.
    """

    # Instantaneous L1 values. Power is in kW (DSMR/SlimmeLezer native unit);
    # voltage in V; current in A.
    power_consumed: float | None = None
    voltage: float | None = None
    current: float | None = None

    # Cumulative counters (since meter install).
    energy_tariff1: float | None = None  # kWh consumed (low)
    energy_tariff2: float | None = None  # kWh consumed (high)
    gas: float | None = None  # m³

    # Indoor environment (any HA sensor; configured per addon options).
    indoor_temp: float | None = None  # °C
    indoor_humidity: float | None = None  # %

    # 24h history grids (length HISTORY_BUCKETS, oldest -> newest).
    # Each entry is a value in the same unit as the live field above, or
    # ``None`` for buckets where no value was available yet.
    history_power_consumed: list[float | None] = field(default_factory=list)
    history_voltage: list[float | None] = field(default_factory=list)
    history_current: list[float | None] = field(default_factory=list)
    history_indoor_temp: list[float | None] = field(default_factory=list)
    history_indoor_humidity: list[float | None] = field(default_factory=list)


def _safe_float(value) -> float | None:
    """Coerce HA state strings to float, returning None for unknown/unavailable."""
    if value is None:
        return None
    if isinstance(value, str):
        s = value.strip().lower()
        if s in ("", "unknown", "unavailable", "none"):
            return None
    try:
        return float(value)
    except (TypeError, ValueError):
        return None


def _read_state(ha: HAClient, entity_id: str) -> float | None:
    """Read a single numeric state value, returning None on any failure."""
    if not entity_id:
        return None
    s = ha.get_state(entity_id)
    if not s:
        return None
    return _safe_float(s.get("state"))


def _bucket_history(
    raw: list[dict],
    start: datetime,
    end: datetime,
    n: int,
) -> list[float | None]:
    """Resample HA history (state-change list) onto an N-point uniform time grid.

    HA's history endpoint returns events when the state *changed*, not on a
    uniform schedule. To plot a sparkline we need N evenly-spaced samples,
    so for each grid timestamp we take the most recent state at-or-before
    that moment ("step" interpolation, the standard for state recordings).

    Buckets before the first recorded state-change are returned as ``None``
    so the sparkline shows a gap rather than a flat-line lie.
    """
    if not raw:
        return [None] * n

    # Parse + sort points by timestamp. Skip events with no usable timestamp
    # (HA always provides one but be defensive — minimal_response can omit
    # state values too if the entity went unavailable mid-sample).
    parsed: list[tuple[datetime, float | None]] = []
    for item in raw:
        ts = item.get("last_changed") or item.get("last_updated")
        if not ts:
            continue
        try:
            t = datetime.fromisoformat(str(ts).replace("Z", "+00:00"))
        except Exception:
            continue
        v = _safe_float(item.get("state"))
        parsed.append((t, v))
    if not parsed:
        return [None] * n
    parsed.sort(key=lambda p: p[0])

    span = (end - start).total_seconds()
    if span <= 0:
        return [None] * n

    out: list[float | None] = []
    j = 0  # walking index into parsed
    last_v: float | None = None
    for i in range(n):
        # Grid timestamp for bucket i — endpoints inclusive at both ends.
        t_grid = start + timedelta(seconds=span * i / max(1, n - 1))
        # Advance through every state change at or before this grid point.
        while j < len(parsed) and parsed[j][0] <= t_grid:
            last_v = parsed[j][1]
            j += 1
        out.append(last_v)
    return out


def _fallback() -> EnergyState:
    """Synthesise believable demo data for offline dev rendering.

    Single-phase domestic load: baseload around 0.2 kW with morning and
    evening cooking peaks, voltage drifting around 230 V, indoor at a
    comfortable 21 °C / 48 % rh.
    """
    n = HISTORY_BUCKETS

    # Consumption: morning + evening peaks, baseload 0.2 kW.
    hist_cons: list[float | None] = [
        round(
            0.2 + 0.4 * math.exp(-((i - 30) ** 2) / 50.0) + 0.6 * math.exp(-((i - 78) ** 2) / 60.0),
            3,
        )
        for i in range(n)
    ]
    hist_v: list[float | None] = [round(230 + 0.5 * math.sin(i / 7.0), 2) for i in range(n)]
    hist_a: list[float | None] = [round(0.7 + 0.6 * math.cos(i / 9.0), 2) for i in range(n)]
    # Indoor: cooler at night, warmer midday.
    hist_t: list[float | None] = [
        round(20.5 + 1.2 * math.sin((i - 36) / 18.0), 2) for i in range(n)
    ]
    hist_h: list[float | None] = [round(50 - 5 * math.sin((i - 36) / 18.0), 1) for i in range(n)]

    return EnergyState(
        power_consumed=0.180,
        voltage=230.1,
        current=0.78,
        energy_tariff1=12345.6,
        energy_tariff2=7890.1,
        gas=2345.678,
        indoor_temp=21.3,
        indoor_humidity=48.0,
        history_power_consumed=hist_cons,
        history_voltage=hist_v,
        history_current=hist_a,
        history_indoor_temp=hist_t,
        history_indoor_humidity=hist_h,
    )


def fetch(settings: Settings) -> EnergyState:
    """Pull current values + 24h history for the energy page.

    Strategy:

    1. One HA state call per configured entity for the live numbers
       (~8 calls now that we're single-phase + consumption-only).
    2. A single ``/api/history/period`` call for all five sparkline
       entities at once (HA accepts a comma-separated list).

    Falls back to synthetic data if HA is unreachable or no live values
    came through (typical "entity IDs are wrong" symptom — better to show
    *something* than a page full of em-dashes).
    """
    ha = HAClient(settings)
    if not ha.available:
        log.info("energy: HA unavailable, using fallback")
        return _fallback()

    out = EnergyState()

    # Per-phase live values. Each ``_read_state`` swallows errors and
    # returns None, so partial data still renders gracefully.
    out.power_consumed = _read_state(ha, settings.energy_power_consumed_l1_entity)
    out.voltage = _read_state(ha, settings.energy_voltage_l1_entity)
    out.current = _read_state(ha, settings.energy_current_l1_entity)
    out.energy_tariff1 = _read_state(ha, settings.energy_tariff1_entity)
    out.energy_tariff2 = _read_state(ha, settings.energy_tariff2_entity)
    out.gas = _read_state(ha, settings.energy_gas_entity)
    out.indoor_temp = _read_state(ha, settings.indoor_temp_entity)
    out.indoor_humidity = _read_state(ha, settings.indoor_humidity_entity)

    # Fetch history for the five sparkline traces in a single HTTP call.
    # We tolerate any of these being unconfigured (empty entity ID) by
    # excluding them and filling those histories with empty lists later.
    end = datetime.now(UTC)
    start = end - timedelta(hours=HISTORY_HOURS)
    history_targets: list[tuple[str, str]] = [
        ("history_power_consumed", settings.energy_power_consumed_l1_entity),
        ("history_voltage", settings.energy_voltage_l1_entity),
        ("history_current", settings.energy_current_l1_entity),
        ("history_indoor_temp", settings.indoor_temp_entity),
        ("history_indoor_humidity", settings.indoor_humidity_entity),
    ]
    configured = [(attr, ent) for attr, ent in history_targets if ent]
    if configured:
        ent_ids = [ent for _, ent in configured]
        history_resp = ha.get_history(ent_ids, start.isoformat(), end.isoformat())
        for (attr, _ent), raw in zip(configured, history_resp, strict=False):
            buckets = _bucket_history(raw or [], start, end, HISTORY_BUCKETS)
            setattr(out, attr, buckets)
    # Unconfigured histories stay as the dataclass default (empty list);
    # the sparkline drawer treats that as "no data" and renders a stub.

    # If literally nothing came through on the live values, the entity IDs
    # are probably wrong — render fallback so the page still demos cleanly.
    has_any_live = any(
        v is not None
        for v in (
            out.power_consumed,
            out.voltage,
            out.current,
            out.energy_tariff1,
            out.indoor_temp,
        )
    )
    if not has_any_live:
        log.warning(
            "energy: no live values returned for any configured entity — "
            "using fallback. Check entity IDs in addon options."
        )
        return _fallback()

    log.info(
        "energy: P_cons=%s V=%s I=%s T1=%s T2=%s gas=%s indoor_t=%s indoor_h=%s histories=%d",
        out.power_consumed,
        out.voltage,
        out.current,
        out.energy_tariff1,
        out.energy_tariff2,
        out.gas,
        out.indoor_temp,
        out.indoor_humidity,
        sum(1 for attr, _ in history_targets if getattr(out, attr)),
    )
    return out


__all__ = ["EnergyState", "fetch", "HISTORY_BUCKETS", "HISTORY_HOURS"]

# `Callable` is imported for type hints used by the page module — keeping
# it imported here means downstream pages don't need their own import for
# matching format-function signatures.
_Formatter = Callable[[float | None], str]  # noqa: F841
