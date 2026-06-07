# ePaper Dashboard add-on

Renders an 800×480 1-bit dashboard image for the Seeed reTerminal E1001
ePaper display. The ESP32-S3 firmware fetches the image every 15 minutes.

## Install

Add this repository to Home Assistant and install via the add-on store —
see the [top-level README](../README.md#install-the-add-on) for the
one-click badge and step-by-step instructions.

For local development against a checkout of this repo, see
[Local development](#local-development) below.

## Endpoints

* `GET /` — HTML preview that auto-refreshes, with controls to simulate sensor values
* `GET /dashboard.png` — preview-friendly PNG
* `GET /dashboard.bmp` — 1-bit BMP for ESPHome `online_image`
* `GET /healthz` — health probe (also reports cache entry count)
* `POST /refresh` — clears the cache

All image endpoints accept optional sensor query parameters that the firmware
pushes on each wake. They appear in the "Indoors" widget:

* `indoor_temp` — °C (float)
* `indoor_hum` — % (float)
* `battery_pct` — % (0–100)

Examples:

```
/dashboard.png?indoor_temp=21.3&indoor_hum=48&battery_pct=87   # normal
/dashboard.png?indoor_temp=21.3&indoor_hum=48&battery_pct=12   # low battery
/dashboard.png                                                 # cold boot
```

Responses include an `X-Cache: hit|miss` header. The cache keys on rounded
sensor values, so requests differing only in sub-degree noise reuse the
same rendered image.

## Local development

```
python3 -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt
uvicorn app.main:app --reload --port 8099
open http://localhost:8099/
```

Without a `SUPERVISOR_TOKEN` env var the weather and calendar widgets use
fallback fixture data.
