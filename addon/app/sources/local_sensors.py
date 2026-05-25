"""Local sensors source.

Unlike weather/calendar which fetch from HA, local sensor values are *pushed*
into the renderer by the device on each wake (as query parameters). This
module just normalises and clamps the incoming values.
"""

from __future__ import annotations

import math
from dataclasses import dataclass


def _sanitize(value: float | None) -> float | None:
    if value is None:
        return None
    try:
        f = float(value)
    except (TypeError, ValueError):
        return None
    if math.isnan(f) or math.isinf(f):
        return None
    return f


@dataclass
class LocalSensors:
    indoor_temp: float | None = None  # °C
    indoor_hum: float | None = None  # %
    battery_pct: float | None = None  # 0-100

    @classmethod
    def from_query(
        cls,
        indoor_temp: float | None = None,
        indoor_hum: float | None = None,
        battery_pct: float | None = None,
    ) -> LocalSensors:
        t = _sanitize(indoor_temp)
        h = _sanitize(indoor_hum)
        b = _sanitize(battery_pct)
        if h is not None:
            h = max(0.0, min(100.0, h))
        if b is not None:
            b = max(0.0, min(100.0, b))
        return cls(indoor_temp=t, indoor_hum=h, battery_pct=b)

    def cache_key(self) -> tuple:
        """Quantize values for cache lookup (integer rounding per plan)."""
        return (
            round(self.indoor_temp) if self.indoor_temp is not None else None,
            round(self.indoor_hum) if self.indoor_hum is not None else None,
            round(self.battery_pct) if self.battery_pct is not None else None,
        )
