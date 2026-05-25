# ePaper Dashboard add-on

Renders an 800×480 1-bit dashboard image for the Seeed reTerminal E1001
ePaper display. The ESP32-S3 firmware fetches the image every 15 minutes.

## Install (local add-on)

1. Copy this `addon/` folder to `/addons/epaper_dashboard/` on your Home
   Assistant host (Samba/SSH add-on makes this easy).
2. In Home Assistant: **Settings → Add-ons → Add-on Store → ⋮ → Check for updates**.
3. The "ePaper Dashboard" add-on appears under **Local add-ons**. Install it.
4. Configure options (Wi-Fi SSID/password for QR, weather entity, calendar
   entities), then start the add-on.
5. Open the Ingress panel (or visit `http://<ha-ip>:8099/`) to preview.

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
