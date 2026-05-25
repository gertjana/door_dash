"""Tesla source: reads battery percentage from a Home Assistant sensor entity.

The HA entity to read is configurable (`tesla_battery_entity`). Until the
user has set up the Tesla integration in HA, this source returns a stub
fallback so the widget always renders during development.
"""

from __future__ import annotations

from dataclasses import dataclass

from ..config import Settings
from ..ha_client import HAClient


@dataclass
class TeslaState:
    battery_pct: float | None = None
    charging: bool = False
    range_km: float | None = None


_FALLBACK = TeslaState(battery_pct=73.0, charging=False, range_km=309.0)


def _to_float(value) -> float | None:
    try:
        return float(value)
    except (TypeError, ValueError):
        return None


def fetch(settings: Settings) -> TeslaState:
    ha = HAClient(settings)
    if not ha.available:
        # No HA connection (local dev). Show fallback so the widget is visible.
        return _FALLBACK

    if not settings.tesla_battery_entity:
        # HA reachable but Tesla integration not yet configured — show fake
        # demo data so screenshots / dashboard look complete until Tesla
        # Fleet API approval lands.
        return _FALLBACK

    state = ha.get_state(settings.tesla_battery_entity)
    if not state:
        return _FALLBACK

    battery_pct = _to_float(state.get("state"))
    attrs = state.get("attributes", {}) or {}
    range_km = _to_float(attrs.get("range") or attrs.get("estimated_range"))

    charging = False
    if settings.tesla_charging_entity:
        ch = ha.get_state(settings.tesla_charging_entity)
        if ch:
            ch_state = (ch.get("state") or "").lower()
            charging = ch_state in ("charging", "on", "true")

    return TeslaState(battery_pct=battery_pct, charging=charging, range_km=range_km)
