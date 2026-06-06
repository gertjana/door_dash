"""Add-on configuration loaded from environment variables.

In a Home Assistant add-on, options from `config.yaml` are exposed in
`/data/options.json`. We load that if present, otherwise fall back to env vars
(useful for local development).
"""

from __future__ import annotations

import json
import os
from pathlib import Path

from pydantic import Field
from pydantic_settings import BaseSettings, SettingsConfigDict

OPTIONS_PATH = Path("/data/options.json")


def _load_options() -> dict:
    if OPTIONS_PATH.exists():
        try:
            return json.loads(OPTIONS_PATH.read_text())
        except Exception:
            return {}
    return {}


class Settings(BaseSettings):
    model_config = SettingsConfigDict(env_prefix="EPDASH_", env_file=".env", extra="ignore")

    # Display (reTerminal E1001: 7.5" monochrome ePaper, UC8179, 800x480)
    width: int = 800
    height: int = 480

    # Wi-Fi QR
    wifi_ssid: str = "YourWiFi"
    wifi_password: str = ""
    wifi_security: str = "nopass"  # WPA / WEP / nopass
    wifi_hidden: bool = False

    # Weather
    weather_entity: str = "weather.home"

    # Calendar
    calendar_entities: list[str] = Field(default_factory=lambda: ["calendar.personal"])
    max_events: int = 8

    # Tesla — HA entities from the Tesla Fleet integration. Each is optional;
    # whatever is set gets read. `show_tesla` toggles widget visibility.
    # `tesla_charging_entity` is intentionally absent: rarely charged at home.
    show_tesla: bool = True
    tesla_battery_entity: str = ""  # e.g. sensor.finn_mccool_battery_level
    tesla_range_entity: str = ""  # e.g. sensor.finn_mccool_battery_range
    tesla_inside_temp_entity: str = ""  # e.g. sensor.finn_mccool_inside_temperature
    tesla_climate_entity: str = ""  # e.g. climate.finn_mccool_climate

    # Local sensors (pushed by device via query params)
    show_local_sensors: bool = True

    # Energy / DSMR P1 reader. Defaults match the Zuidwijk SlimmeLezer
    # firmware naming (`Power Consumed Phase 1` -> `sensor.power_consumed_phase_1`).
    # Override per-entity in addon options if your firmware uses different names.
    energy_power_produced_l1_entity: str = "sensor.power_produced_phase_1"
    energy_power_produced_l2_entity: str = "sensor.power_produced_phase_2"
    energy_power_produced_l3_entity: str = "sensor.power_produced_phase_3"
    energy_power_consumed_l1_entity: str = "sensor.power_consumed_phase_1"
    energy_power_consumed_l2_entity: str = "sensor.power_consumed_phase_2"
    energy_power_consumed_l3_entity: str = "sensor.power_consumed_phase_3"
    energy_voltage_l1_entity: str = "sensor.voltage_phase_1"
    energy_voltage_l2_entity: str = "sensor.voltage_phase_2"
    energy_voltage_l3_entity: str = "sensor.voltage_phase_3"
    energy_current_l1_entity: str = "sensor.current_phase_1"
    energy_current_l2_entity: str = "sensor.current_phase_2"
    energy_current_l3_entity: str = "sensor.current_phase_3"
    # Cumulative totals (kWh delivered per tariff, m³ gas)
    energy_tariff1_entity: str = "sensor.energy_consumed_tariff_1"
    energy_tariff2_entity: str = "sensor.energy_consumed_tariff_2"
    energy_gas_entity: str = "sensor.gas_consumed"
    # Indoor temp/humidity from any HA sensor (optional). Used by the
    # energy page for both numbers and 24h sparklines. Defaults match the
    # `door_dashboard` ESPHome firmware (BME280 on the reTerminal). Empty = not configured.
    indoor_temp_entity: str = "sensor.door_dashboard_indoor_temperature"
    indoor_humidity_entity: str = "sensor.door_dashboard_indoor_humidity"

    # Timezone / locale
    timezone: str = "Europe/Amsterdam"

    # Cache (seconds)
    refresh_cache_seconds: int = 60

    # Home Assistant access (supplied by Supervisor inside add-on).
    # Supervisor injects SUPERVISOR_TOKEN — not EPDASH_SUPERVISOR_TOKEN —
    # so we read it directly in get_settings() rather than via pydantic's
    # env_prefix machinery.
    supervisor_token: str | None = None
    ha_base_url: str = "http://supervisor/core"


def get_settings() -> Settings:
    # Layer 1: HA add-on options.json
    options = _load_options()
    # Layer 2: env vars (pydantic-settings)
    settings = Settings(**options)
    # Layer 3: Supervisor-injected token (unprefixed)
    token = os.environ.get("SUPERVISOR_TOKEN")
    if token and not settings.supervisor_token:
        settings.supervisor_token = token
    return settings
