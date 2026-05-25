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
    wifi_ssid: str = "MoTeds_Quest"
    wifi_password: str = ""
    wifi_security: str = "nopass"  # WPA / WEP / nopass
    wifi_hidden: bool = False

    # Weather
    weather_entity: str = "weather.home"

    # Calendar
    calendar_entities: list[str] = Field(default_factory=lambda: ["calendar.personal"])
    max_events: int = 8

    # Tesla — HA entity that reports battery % (set once configured in HA).
    # `show_tesla` controls visibility; entity is read when set, otherwise the
    # widget renders fallback/placeholder values.
    show_tesla: bool = True
    tesla_battery_entity: str = ""
    tesla_charging_entity: str = ""  # optional, used to show charging indicator

    # Local sensors (pushed by device via query params)
    show_local_sensors: bool = True

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
