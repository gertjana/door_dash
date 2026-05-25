"""Weather source: reads a Home Assistant `weather.*` entity + daily forecast.

Falls back to a fixture so the dashboard always renders during local dev.
Forecast uses `weather.get_forecasts` service (HA 2024.x+) which replaced
the deprecated `forecast` state attribute.
"""

from __future__ import annotations

import logging
from dataclasses import dataclass, field
from datetime import datetime

from ..config import Settings
from ..ha_client import HAClient

log = logging.getLogger(__name__)


@dataclass
class ForecastDay:
    when: datetime  # day the forecast is for
    condition: str  # e.g. "sunny"
    temp_high: float | None
    temp_low: float | None
    precipitation_probability: int | None  # 0-100


@dataclass
class Weather:
    condition: str
    temperature: float | None
    temperature_unit: str
    precipitation_probability: int | None
    wind_speed: float | None
    wind_unit: str
    icon: str | None = None
    forecast: list[ForecastDay] = field(default_factory=list)


_FALLBACK_FORECAST = [
    ForecastDay(
        when=datetime(2026, 5, 26),
        condition="sunny",
        temp_high=22,
        temp_low=12,
        precipitation_probability=5,
    ),
    ForecastDay(
        when=datetime(2026, 5, 27),
        condition="partlycloudy",
        temp_high=20,
        temp_low=11,
        precipitation_probability=20,
    ),
    ForecastDay(
        when=datetime(2026, 5, 28),
        condition="rainy",
        temp_high=17,
        temp_low=10,
        precipitation_probability=70,
    ),
]

_FALLBACK = Weather(
    condition="partlycloudy",
    temperature=18.0,
    temperature_unit="°C",
    precipitation_probability=20,
    wind_speed=12.0,
    wind_unit="km/h",
    forecast=_FALLBACK_FORECAST,
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


def _fetch_forecast(ha: HAClient, entity_id: str) -> list[ForecastDay]:
    resp = ha.call_service(
        "weather",
        "get_forecasts",
        data={"entity_id": entity_id, "type": "daily"},
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
    out: list[ForecastDay] = []
    for f in raw:
        when = _parse_dt(f.get("datetime"))
        if when is None:
            continue
        out.append(
            ForecastDay(
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
        return _FALLBACK
    attrs = state.get("attributes", {}) or {}
    forecast: list[ForecastDay] = []
    try:
        forecast = _fetch_forecast(ha, settings.weather_entity)
    except Exception as exc:
        log.warning("weather: forecast fetch failed: %s", exc)
    log.info(
        "weather: entity=%s state=%s temp=%s forecast_days=%d",
        settings.weather_entity,
        state.get("state"),
        attrs.get("temperature"),
        len(forecast),
    )
    return Weather(
        condition=state.get("state", "unknown"),
        temperature=attrs.get("temperature"),
        temperature_unit=attrs.get("temperature_unit", "°C"),
        precipitation_probability=attrs.get("precipitation_probability"),
        wind_speed=attrs.get("wind_speed"),
        wind_unit=attrs.get("wind_speed_unit", "km/h"),
        icon=attrs.get("icon"),
        forecast=forecast,
    )
