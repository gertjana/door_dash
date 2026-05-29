"""Tesla source: reads battery %, range, cabin temp + climate state from HA.

Each entity is independently optional; whichever is configured gets read.
When HA is unreachable, or no battery entity is configured, returns demo
fallback data so the widget always renders during development / pre-setup.

`climate_on` is True only when the climate entity reports a non-off mode.
While the car is asleep, the climate entity goes unknown/unavailable; in
that case `climate_on` is None (we can't tell) and the widget should hide
the indicator rather than imply it's off.
"""

from __future__ import annotations

from dataclasses import dataclass

from ..config import Settings
from ..ha_client import HAClient


@dataclass
class TeslaState:
    battery_pct: float | None = None
    range_km: float | None = None
    inside_temp_c: float | None = None
    climate_on: bool | None = None  # None = unknown (car asleep)


_FALLBACK = TeslaState(battery_pct=73.0, range_km=309.0, inside_temp_c=None, climate_on=None)
_UNKNOWN_STATES = {"unknown", "unavailable", "none", ""}


def _to_float(value) -> float | None:
    try:
        return float(value)
    except (TypeError, ValueError):
        return None


def _read_float(ha: HAClient, entity_id: str) -> float | None:
    if not entity_id:
        return None
    state = ha.get_state(entity_id)
    if not state:
        return None
    return _to_float(state.get("state"))


def _read_climate_on(ha: HAClient, entity_id: str) -> bool | None:
    """Return True if climate is on, False if off, None if unknown/asleep."""
    if not entity_id:
        return None
    state = ha.get_state(entity_id)
    if not state:
        return None
    raw = (state.get("state") or "").strip().lower()
    if raw in _UNKNOWN_STATES:
        return None
    return raw != "off"


def fetch(settings: Settings) -> TeslaState:
    ha = HAClient(settings)
    if not ha.available:
        return _FALLBACK

    if not settings.tesla_battery_entity:
        return _FALLBACK

    return TeslaState(
        battery_pct=_read_float(ha, settings.tesla_battery_entity),
        range_km=_read_float(ha, settings.tesla_range_entity),
        inside_temp_c=_read_float(ha, settings.tesla_inside_temp_entity),
        climate_on=_read_climate_on(ha, settings.tesla_climate_entity),
    )
