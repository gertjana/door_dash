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

### Version badge

A tiny `v{addon} · fw{firmware}` badge is rendered in the top-right corner of
every image. The addon version is read at startup from `addon/config.yaml`'s
`version:` field; the firmware version is sent by the device as a `?fw=`
query parameter on every fetch. Bumping either side requires only:

* **Addon**: edit `version:` in `addon/config.yaml`, redeploy
* **Firmware**: edit `firmware_version:` in `firmware/reterminal-dashboard.yaml`,
  reflash (USB or OTA)

This gives you an at-a-glance way to confirm what's actually running on the
panel after a deploy.

## Firmware workflow

The firmware lives in `firmware/reterminal-dashboard.yaml` and is built and
flashed **from your Mac**, not from HA's ESPHome Device Builder add-on. The
repo is the single source of truth.

### Two things named "ESPHome" in HA — they are not the same

HA has two unrelated pieces of software both branded ESPHome. Confusing them
will cost you an afternoon:

| Name | Where | What it does | We use it? |
|---|---|---|---|
| **ESPHome Device Builder** | Settings → Add-ons | Compiles firmware on the Pi from a YAML stored in `/config/esphome/`. | **No** — Pi is too slow and runs out of RAM. We build on Mac instead. |
| **ESPHome integration** | Settings → Devices & services | Talks to a running ESPHome device over its Native API (port 6053) and exposes its sensors as HA entities. | **Yes** — gives us `sensor.indoor_temperature` etc. for history and automations. |

The two are completely independent. You can use the integration without ever
installing the add-on.

### First flash / recovery (USB)

```bash
# 1. Set up a Python venv with esphome (one-time)
python3.13 -m venv ~/.esphome-venv
source ~/.esphome-venv/bin/activate
pip install esphome

# 2. Make sure firmware/secrets.yaml exists with the four required keys
#    (wifi_ssid, wifi_password, api_encryption_key, ota_password).
#    See firmware/secrets.yaml.example.

# 3. Put device into download mode: hold BOOT → tap RESET → release BOOT
# 4. Flash
esphome run firmware/reterminal-dashboard.yaml --device /dev/cu.usbserial-110
```

### Subsequent updates (wireless OTA)

Once the device is running with `dev_mode: "0"`, it deep-sleeps between
refreshes and is unreachable over the network most of the time. To push an
OTA update:

1. Press the **right white button** (GPIO4) — wakes the device and pins it
   awake (the on-boot lambda detects button wake-cause and calls
   `prevent_deep_sleep()`).
2. From the Mac (no `--device` flag needed; mDNS discovers `doordash.local`):
   ```bash
   source ~/.esphome-venv/bin/activate
   esphome run firmware/reterminal-dashboard.yaml
   ```
3. Device receives the update, reboots, fetches one image, then sleeps on
   the next cycle.

If mDNS is blocked on your network, supply the device IP explicitly:
`esphome run firmware/reterminal-dashboard.yaml --device 192.168.50.159`.

### Re-pairing the ESPHome integration

Only needed if you rotate `api_encryption_key` in `firmware/secrets.yaml` and
reflash. HA's integration stores the key separately, so after a key change:

1. Wake the device (button)
2. Settings → Devices & services → ESPHome → device → **Reconfigure** (or
   delete + re-add if no banner appears)
3. Paste the new key from `firmware/secrets.yaml`

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
