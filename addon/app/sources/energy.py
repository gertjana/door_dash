"""Energy / DSMR P1 source.

Reads instantaneous values per phase (L1/L2/L3) for power produced,
power consumed, voltage and current; the cumulative tariff and gas
counters; and indoor temp/humidity. Also fetches a 24-hour history
strip for the values that get sparklines on the energy page (L1
power produced, L1 power consumed, L1 voltage, L1 current, indoor
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
class PhaseValues:
    """Per-phase value triplet. Any field may be ``None`` if HA returns
    no state for that phase (older meters often only report L1)."""

    l1: float | None = None
    l2: float | None = None
    l3: float | None = None

    def values(self) -> tuple[float | None, float | None, float | None]:
        return (self.l1, self.l2, self.l3)


@dataclass
class EnergyState:
    """Snapshot of P1 + indoor environment values for the energy page."""

    # Instantaneous phase values. Power is in kW (DSMR/SlimmeLezer native unit);
    # voltage in V; current in A.
    power_produced: PhaseValues = field(default_factory=PhaseValues)
    power_consumed: PhaseValues = field(default_factory=PhaseValues)
    voltage: PhaseValues = field(default_factory=PhaseValues)
    current: PhaseValues = field(default_factory=PhaseValues)

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
    history_power_produced_l1: list[float | None] = field(default_factory=list)
    history_power_consumed_l1: list[float | None] = field(default_factory=list)
    history_voltage_l1: list[float | None] = field(default_factory=list)
    history_current_l1: list[float | None] = field(default_factory=list)
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

    Builds a sunny-midday scenario: ~2.5 kW solar production peaking on L2,
    modest household draw spread across phases, voltage hovering around
    230 V, indoor at a comfortable 21 °C / 48 % rh.
    """
    # Live values — solar producing on L2, mixed consumption on the others.
    power_consumed = PhaseValues(l1=0.180, l2=0.090, l3=0.130)  # kW
    power_produced = PhaseValues(l1=0.0, l2=2.5, l3=0.0)
    voltage = PhaseValues(l1=230.1, l2=231.5, l3=229.4)
    current = PhaseValues(l1=0.78, l2=11.2, l3=0.57)

    n = HISTORY_BUCKETS
    # Solar curve: 0 outside roughly 06:00-18:00 (buckets 24-72 of 96),
    # half-sine peak ~2.5 kW around noon (bucket 48).
    hist_prod: list[float | None] = []
    for i in range(n):
        if 24 <= i <= 72:
            t = (i - 24) / 48  # 0..1 across daylight band
            hist_prod.append(round(2.5 * math.sin(math.pi * t), 3))
        else:
            hist_prod.append(0.0)
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
        power_produced=power_produced,
        power_consumed=power_consumed,
        voltage=voltage,
        current=current,
        energy_tariff1=12345.6,
        energy_tariff2=7890.1,
        gas=2345.678,
        indoor_temp=21.3,
        indoor_humidity=48.0,
        history_power_produced_l1=hist_prod,
        history_power_consumed_l1=hist_cons,
        history_voltage_l1=hist_v,
        history_current_l1=hist_a,
        history_indoor_temp=hist_t,
        history_indoor_humidity=hist_h,
    )


def fetch(settings: Settings) -> EnergyState:
    """Pull current values + 24h history for the energy page.

    Strategy:

    1. One HA state call per configured entity for the live numbers
       (~17 calls, but fast and parallel-friendly inside HA).
    2. A single ``/api/history/period`` call for all six sparkline
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
    out.power_produced = PhaseValues(
        l1=_read_state(ha, settings.energy_power_produced_l1_entity),
        l2=_read_state(ha, settings.energy_power_produced_l2_entity),
        l3=_read_state(ha, settings.energy_power_produced_l3_entity),
    )
    out.power_consumed = PhaseValues(
        l1=_read_state(ha, settings.energy_power_consumed_l1_entity),
        l2=_read_state(ha, settings.energy_power_consumed_l2_entity),
        l3=_read_state(ha, settings.energy_power_consumed_l3_entity),
    )
    out.voltage = PhaseValues(
        l1=_read_state(ha, settings.energy_voltage_l1_entity),
        l2=_read_state(ha, settings.energy_voltage_l2_entity),
        l3=_read_state(ha, settings.energy_voltage_l3_entity),
    )
    out.current = PhaseValues(
        l1=_read_state(ha, settings.energy_current_l1_entity),
        l2=_read_state(ha, settings.energy_current_l2_entity),
        l3=_read_state(ha, settings.energy_current_l3_entity),
    )
    out.energy_tariff1 = _read_state(ha, settings.energy_tariff1_entity)
    out.energy_tariff2 = _read_state(ha, settings.energy_tariff2_entity)
    out.gas = _read_state(ha, settings.energy_gas_entity)
    out.indoor_temp = _read_state(ha, settings.indoor_temp_entity)
    out.indoor_humidity = _read_state(ha, settings.indoor_humidity_entity)

    # Fetch history for the six sparkline traces in a single HTTP call.
    # We tolerate any of these being unconfigured (empty entity ID) by
    # excluding them and filling those histories with empty lists later.
    end = datetime.now(UTC)
    start = end - timedelta(hours=HISTORY_HOURS)
    history_targets: list[tuple[str, str]] = [
        ("history_power_produced_l1", settings.energy_power_produced_l1_entity),
        ("history_power_consumed_l1", settings.energy_power_consumed_l1_entity),
        ("history_voltage_l1", settings.energy_voltage_l1_entity),
        ("history_current_l1", settings.energy_current_l1_entity),
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
            out.power_produced.l1,
            out.power_consumed.l1,
            out.voltage.l1,
            out.current.l1,
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
        "energy: P_prod_l1=%s P_cons_l1=%s V_l1=%s I_l1=%s T1=%s T2=%s gas=%s "
        "indoor_t=%s indoor_h=%s histories=%d",
        out.power_produced.l1,
        out.power_consumed.l1,
        out.voltage.l1,
        out.current.l1,
        out.energy_tariff1,
        out.energy_tariff2,
        out.gas,
        out.indoor_temp,
        out.indoor_humidity,
        sum(1 for attr, _ in history_targets if getattr(out, attr)),
    )
    return out


# Re-exported alias so callers don't need to import the fallback builder
# directly when they want to know what the demo data looks like.
__all__ = ["EnergyState", "PhaseValues", "fetch", "HISTORY_BUCKETS", "HISTORY_HOURS"]

# `Callable` is imported for type hints used by the page module — keeping
# it imported here means downstream pages don't need their own import for
# matching format-function signatures.
_Formatter = Callable[[float | None], str]  # noqa: F841
