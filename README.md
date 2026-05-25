# reTerminal E1001 ePaper Dashboard

A doorside dashboard for the Seeed reTerminal E1001 ePaper display (7.5" mono, 800×480, UC8179, ESP32-S3).
Renders Wi-Fi QR, weather, indoor sensors, and an upcoming calendar list.

## Architecture

```
┌──────────────────┐                       ┌─────────────────────────────┐
│ reTerminal E1001 │ ── HTTP GET ────────▶ │ Home Assistant (Pi)         │
│ ESPHome firmware │   /dashboard.bmp      │  └─ "EPaper Dashboard"      │
│ deep_sleep 15m   │ ◀──────────────────── │     add-on (FastAPI+Pillow) │
└──────────────────┘   1-bit BMP 960×640   └─────────────────────────────┘
```

* **`addon/`** — Home Assistant add-on. Python service that composes a 1-bit
  800×480 image from the configured widgets and serves it over HTTP.
* **`firmware/`** — ESPHome YAML for the reTerminal E1001. Wakes every 15 min,
  fetches the image, draws it, deep-sleeps.

## Layout (800×480 landscape)

```
┌─────────────────────┬─────────────────────────────────────────┐
│  Wi-Fi              │  Weather                                │
│  [QR code]          │  [icon] 18°C   │  Tue   Wed   Thu       │
│  SSID               │  Partlycloudy  │  ☼     ☁     ☼         │
│                     │  Wind 12 km/h  │ 22/12 20/11 17/10      │
│                     ├─────────────────────────────────────────┤
├─────────────────────┤  Upcoming                               │
│  Indoors            │  ─────────────────────────────────────  │
│  Temp 21°C  Hum 48% │  Today      Dentist appointment         │
│  ▰▰▰▰▰▰▱  87%       │  19:00      Tandartspraktijk Centrum    │
├─────────────────────┤  Tomorrow  [Work] Team standup          │
│  Car Batt - Range   │  18:00      Online                      │
│  73% · 309 km       │  27 May     Dinner with Anna            │
├─────────────────────┤  03:00      Restaurant De Kas           │
│   Monday 25 May     │  …                                      │
│   Refreshed 16:32   │                                         │
└─────────────────────┴─────────────────────────────────────────┘
```

Indoor temperature/humidity and battery percentage are pushed by the firmware
as query parameters (`?indoor_temp=…&indoor_hum=…&battery_pct=…`) on each
wake, so the displayed reading is always current-cycle accurate. They're also
published to Home Assistant as sensor entities for history and automations.

### Rendering notes

The display is 1-bit. The renderer threshold-converts (no dithering) and uses
a custom `draw_crisp_text()` helper that disables Pillow anti-aliasing for
small text (≤ 16 px), so glyphs stay sharp instead of getting eaten by the
threshold pass. Large titles (20–28 px) keep AA since the rounded edges read
fine on the panel.

## Local development

You can preview layouts in a browser without the physical device:

```bash
cd addon/app
python -m venv .venv && source .venv/bin/activate
pip install -r ../requirements.txt
uvicorn main:app --reload --port 8099
open http://localhost:8099/dashboard.png
```


Set environment variables (or copy `.env.example` to `.env`) to point at a real
Home Assistant for live weather/calendar data; otherwise the renderer falls back
to fixture data so the layout is always previewable.
