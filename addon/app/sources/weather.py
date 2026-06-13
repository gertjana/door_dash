"""Weather source: reads a Home Assistant `weather.*` entity + forecasts.

Falls back to a fixture so the dashboard always renders during local dev.
Forecasts use `weather.get_forecasts` service (HA 2024.x+) which replaced
the deprecated `forecast` state attribute. We fetch both **daily** and
**hourly** forecasts so different pages can render whichever they need.
"""

from __future__ import annotations

import logging
from dataclasses import dataclass, field
from datetime import datetime, timedelta

from ..config import Settings
from ..ha_client import HAClient

log = logging.getLogger(__name__)


@dataclass
class ForecastEntry:
    when: datetime  # the moment the forecast is for (start of hour or day)
    condition: str  # e.g. "sunny"
    # For daily entries both highs and lows are meaningful; for hourly
    # entries HA typically only populates ``temp_high`` (treated as the
    # forecast temperature for that hour) and leaves ``temp_low`` None.
    temp_high: float | None
    temp_low: float | None
    precipitation_probability: int | None  # 0-100


# Back-compat alias — pre-existing callers refer to ``ForecastDay``.
# Same shape, just renamed to better reflect that the same dataclass
# now also represents hourly entries.
ForecastDay = ForecastEntry


@dataclass
class Weather:
    condition: str
    temperature: float | None
    temperature_unit: str
    precipitation_probability: int | None
    wind_speed: float | None
    wind_unit: str
    # New attributes — all optional so older code that constructs
    # Weather objects without them keeps working.
    humidity: float | None = None  # %
    pressure: float | None = None
    pressure_unit: str = "hPa"
    wind_bearing: float | None = None  # degrees, 0=N
    last_updated: datetime | None = None  # entity's last state change
    icon: str | None = None
    # Daily and hourly forecasts. Daily is shown on the dashboard widget;
    # hourly is shown on the full-screen weather page.
    forecast: list[ForecastEntry] = field(default_factory=list)
    forecast_hourly: list[ForecastEntry] = field(default_factory=list)


# 16-point compass abbreviations; index = round(degrees / 22.5) % 16.
_COMPASS_16 = [
    "N", "NNE", "NE", "ENE",
    "E", "ESE", "SE", "SSE",
    "S", "SSW", "SW", "WSW",
    "W", "WNW", "NW", "NNW",
]  # fmt: skip


def bearing_to_cardinal(bearing: float | None) -> str | None:
    """Convert a wind bearing (degrees from north, clockwise) to a 16-point
    compass label like ``"WNW"``. Returns None if the bearing is missing
    or non-numeric so callers can skip rendering the suffix cleanly.
    """
    if bearing is None:
        return None
    try:
        idx = int(round(float(bearing) / 22.5)) % 16
    except (TypeError, ValueError):
        return None
    return _COMPASS_16[idx]


# Beaufort upper bounds in km/h — the canonical thresholds (WMO). The
# index is the Beaufort number; speeds at or below the value at index N
# fall in force N. Anything above the last entry (force 12) is hurricane
# force, capped at 12 to keep labels short.
_BEAUFORT_KMH_UPPER = [1, 5, 11, 19, 28, 38, 49, 61, 74, 88, 102, 117]


def wind_speed_to_beaufort(speed: float | None, unit: str | None = "km/h") -> int | None:
    """Convert a wind speed to its Beaufort-scale force number (0–12).

    The Beaufort thresholds are defined in km/h; values arriving in m/s
    are converted via the standard 3.6× factor before bucketing. Unknown
    or unparseable units fall back to km/h on the assumption that most
    Home Assistant ``weather.*`` integrations report km/h. Returns
    ``None`` only when the input speed itself is missing/invalid so
    callers can suppress the row cleanly.
    """
    if speed is None:
        return None
    try:
        v = float(speed)
    except (TypeError, ValueError):
        return None
    u = (unit or "").strip().lower()
    if u in {"m/s", "ms", "meter/s"}:
        v *= 3.6
    elif u in {"mph", "mi/h"}:
        v *= 1.609344
    elif u in {"kn", "kt", "knot", "knots"}:
        v *= 1.852
    # Default and km/h need no conversion.
    for force, upper in enumerate(_BEAUFORT_KMH_UPPER):
        if v <= upper:
            return force
    return 12


def _build_fallback_forecast() -> list[ForecastEntry]:
    """Daily fallback anchored on *today* so dates stay current in dev."""
    today = datetime.now().date()
    samples = [
        ("sunny", 22, 12, 5),
        ("partlycloudy", 20, 11, 20),
        ("rainy", 17, 10, 70),
        ("cloudy", 19, 9, 30),
        ("partlycloudy", 21, 10, 10),
        ("sunny", 24, 13, 0),
        ("partlycloudy", 23, 13, 5),
    ]
    return [
        ForecastEntry(
            when=datetime.combine(today + timedelta(days=i + 1), datetime.min.time()),
            condition=cond,
            temp_high=hi,
            temp_low=lo,
            precipitation_probability=pp,
        )
        for i, (cond, hi, lo, pp) in enumerate(samples)
    ]


def _build_fallback_hourly() -> list[ForecastEntry]:
    """Hourly fallback for the next ~12 hours; smoothly varying temps so
    dev renders look believable rather than randomly noisy.
    """
    base = datetime.now().replace(minute=0, second=0, microsecond=0) + timedelta(hours=1)
    # Walk through a small condition cycle and a gentle temperature curve.
    conds = [
        "partlycloudy",
        "partlycloudy",
        "cloudy",
        "cloudy",
        "rainy",
        "rainy",
        "cloudy",
        "partlycloudy",
        "clear-night",
        "clear-night",
        "clear-night",
        "clear-night",
    ]
    return [
        ForecastEntry(
            when=base + timedelta(hours=i),
            condition=conds[i % len(conds)],
            # Mild diurnal-ish curve from 18 down to 14 over 12h.
            temp_high=round(18 - i * 0.35, 1),
            temp_low=None,
            precipitation_probability=(40 if 4 <= i <= 5 else 10),
        )
        for i in range(12)
    ]


def _fallback() -> Weather:
    """Built fresh on each call so dates stay relative to "today"."""
    return Weather(
        condition="partlycloudy",
        temperature=18.5,
        temperature_unit="°C",
        precipitation_probability=20,
        wind_speed=13.0,
        wind_unit="km/h",
        humidity=66.0,
        pressure=1014.2,
        pressure_unit="hPa",
        wind_bearing=292.5,  # WNW
        last_updated=datetime.now() - timedelta(hours=7),  # matches reference
        forecast=_build_fallback_forecast(),
        forecast_hourly=_build_fallback_hourly(),
    )


def _parse_dt(s) -> datetime | None:
    if not s:
        return None
    if isinstance(s, datetime):
        return s
    try:
        # HA returns ISO 8601 with timezone, e.g. "2026-05-26T00:00:00+00:00"
        return datetime.fromisoformat(str(s).replace("Z", "+00:00"))
    except Exception:
        return None


def _fetch_forecast(ha: HAClient, entity_id: str, kind: str = "daily") -> list[ForecastEntry]:
    """Fetch a forecast list of the given kind ("daily" or "hourly")."""
    resp = ha.call_service(
        "weather",
        "get_forecasts",
        data={"entity_id": entity_id, "type": kind},
        return_response=True,
    )
    if not resp:
        return []
    # Response shape: {"service_response": {"<entity_id>": {"forecast": [...]}}}
    service_resp = resp.get("service_response") or resp
    entity_resp = service_resp.get(entity_id) if isinstance(service_resp, dict) else None
    if not entity_resp:
        return []
    raw = entity_resp.get("forecast") or []
    out: list[ForecastEntry] = []
    for f in raw:
        when = _parse_dt(f.get("datetime"))
        if when is None:
            continue
        out.append(
            ForecastEntry(
                when=when,
                condition=f.get("condition", "unknown"),
                temp_high=f.get("temperature"),
                temp_low=f.get("templow"),
                precipitation_probability=f.get("precipitation_probability"),
            )
        )
    return out


def fetch(settings: Settings) -> Weather:
    ha = HAClient(settings)
    state = ha.get_state(settings.weather_entity)
    if not state:
        log.warning(
            "weather: no state for entity=%r (ha.available=%s, base=%s) — using fallback",
            settings.weather_entity,
            ha.available,
            settings.ha_base_url,
        )
        return _fallback()
    attrs = state.get("attributes", {}) or {}
    forecast: list[ForecastEntry] = []
    forecast_hourly: list[ForecastEntry] = []
    try:
        forecast = _fetch_forecast(ha, settings.weather_entity, "daily")
    except Exception as exc:
        log.warning("weather: daily forecast fetch failed: %s", exc)
    try:
        forecast_hourly = _fetch_forecast(ha, settings.weather_entity, "hourly")
    except Exception as exc:
        log.warning("weather: hourly forecast fetch failed: %s", exc)

    last_updated = _parse_dt(state.get("last_updated") or state.get("last_changed"))

    log.info(
        "weather: entity=%s state=%s temp=%s daily=%d hourly=%d",
        settings.weather_entity,
        state.get("state"),
        attrs.get("temperature"),
        len(forecast),
        len(forecast_hourly),
    )
    return Weather(
        condition=state.get("state", "unknown"),
        temperature=attrs.get("temperature"),
        temperature_unit=attrs.get("temperature_unit", "°C"),
        precipitation_probability=attrs.get("precipitation_probability"),
        wind_speed=attrs.get("wind_speed"),
        wind_unit=attrs.get("wind_speed_unit", "km/h"),
        humidity=attrs.get("humidity"),
        pressure=attrs.get("pressure"),
        pressure_unit=attrs.get("pressure_unit", "hPa"),
        wind_bearing=attrs.get("wind_bearing"),
        last_updated=last_updated,
        icon=attrs.get("icon"),
        forecast=forecast,
        forecast_hourly=forecast_hourly,
    )
