"""Regenerate all preview scenarios (PNG + BMP) into ./preview/."""

from __future__ import annotations

import os
import sys

sys.path.insert(0, "addon")

SCENARIOS = [
    ("01_normal", {"indoor_temp": 21.3, "indoor_hum": 48, "battery_pct": 87}, {}),
    ("02_cold_boot", {}, {}),
    ("03_low_battery", {"indoor_temp": 20.0, "indoor_hum": 50, "battery_pct": 7}, {}),
    ("04_hot_humid", {"indoor_temp": 28.7, "indoor_hum": 78, "battery_pct": 64}, {}),
    ("05_cold_room", {"indoor_temp": 12.4, "indoor_hum": 41, "battery_pct": 92}, {}),
    ("06_battery_full", {"indoor_temp": 22.0, "indoor_hum": 45, "battery_pct": 100}, {}),
    ("07_partial_data", {"indoor_temp": 21.5, "battery_pct": 55}, {}),
    (
        "08_sensors_disabled",
        {"indoor_temp": 21.3, "indoor_hum": 48, "battery_pct": 87},
        {"EPDASH_SHOW_LOCAL_SENSORS": "false"},
    ),
]


def _qs(params: dict) -> str:
    return "&".join(f"{k}={v}" for k, v in params.items())


def render_all() -> None:
    for name, params, envset in SCENARIOS:
        # Apply env overrides (config is re-read per request via Settings cache reset)
        for k, v in envset.items():
            os.environ[k] = v
        try:
            # Re-import to pick up env-driven settings
            for mod in list(sys.modules):
                if mod.startswith("app"):
                    del sys.modules[mod]
            from app.main import app  # noqa: WPS433
            from fastapi.testclient import TestClient

            c = TestClient(app)
            c.post("/refresh")
            qs = _qs({**params, "fw": "0.2.0"})
            png = c.get(f"/dashboard.png?{qs}").content
            bmp = c.get(f"/dashboard.bmp?{qs}").content
            with open(f"preview/{name}.png", "wb") as f:
                f.write(png)
            with open(f"preview/{name}.bmp", "wb") as f:
                f.write(bmp)
            print(f"{name}: png={len(png)}B bmp={len(bmp)}B")
        finally:
            for k in envset:
                os.environ.pop(k, None)


if __name__ == "__main__":
    render_all()
