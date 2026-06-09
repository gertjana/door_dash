# reTerminal E1001 ePaper Dashboard

A doorside dashboard for the Seeed reTerminal E1001 ePaper display (7.5" mono, 800×480, UC8179, ESP32-S3).
A multi-page carousel covering home status, calendar, weather, Tesla state, a personal greeting,
and household energy use — driven by data from Home Assistant.

## Pages

The renderer auto-discovers page modules in `addon/app/render/pages/`; the carousel below reflects the
default ordering (controlled by the `NN_` filename prefix). The firmware learns the page count from
`/pages` so adding a page server-side does not require a reflash.

| # | Page     | Contents                                                                            |
|---|----------|-------------------------------------------------------------------------------------|
| 0 | Dashboard| Wi-Fi QR, indoor temp/humidity, battery %, current weather, Tesla state.            |
| 1 | Calendar | Month grid with the next ~8 events from each configured `calendar.*` entity.        |
| 2 | Weather  | Hourly + multi-day forecast, sun/wind details, precipitation outlook.               |
| 3 | Heart    | Decorative greeting page (configurable text).                                       |
| 4 | Energy   | DSMR P1 phase table (W / V / A per L1/L2/L3) + 24h sparklines, indoor sensor 24h, cumulative tariff & gas counters. |

## Architecture

```
┌──────────────────┐                         ┌─────────────────────────────┐    ┌─────────────────┐
│ reTerminal E1001 │ ── HTTP GET ──────────▶ │ Home Assistant              │◀───| Tesla Fleet API |
│ ESPHome firmware │   /pages                │  └─ "EPaper Dashboard"      │    └─────────────────┘
│ deep_sleep 15m   │ ◀────────────────────── │  │  add-on (FastAPI+Pillow) │    ┌─────────────────┐
│                  │   {"count": N, …}       │  │                          │◀───| Google Calendar |
│                  │                         │  └─ ESPHome                 │    └─────────────────┘
│                  │ ── HTTP GET ──────────▶ │                             │    ┌─────────────────┐
│                  │   /dashboard.bmp?page=1 │                             │◀───| Met.no (Weather)|
│                  │ ◀────────────────────── │                             │    └─────────────────┘
│                  │   1-bit BMP 800×480     |                             |    ┌─────────────────┐
│                  │                         │                             │◀───| Smartmeter P1   |
└──────────────────┘                         └─────────────────────────────┘    └─────────────────┘


```

The firmware learns the current page count from `/pages` (so adding a page
server-side automatically extends the carousel — no firmware reflash) and
fetches the rendered image for the active page from `/dashboard.bmp?page=N`.

* **`addon/`** — Home Assistant add-on. Python service that composes a 1-bit
  800×480 image from the configured widgets and serves it over HTTP.
* **`firmware/`** — ESPHome YAML for the reTerminal E1001. Wakes every 15 min,
  fetches the image, draws it, deep-sleeps.

Indoor temperature/humidity and battery percentage are pushed by the firmware
as query parameters (`?indoor_temp=…&indoor_hum=…&battery_pct=…`) on each
wake, so the displayed reading is always current-cycle accurate. They're also
published to Home Assistant as sensor entities for history and automations.

## Install the add-on

This repository is itself a Home Assistant add-on repository, so the
Supervisor can install and keep the add-on up to date directly from
GitHub — no SSH, Samba, or tarballs required.

### 1. Add the repository

Click the badge to open the **Add repository** dialog with the URL
pre-filled:

[![Open your Home Assistant instance and show the dialog for adding a repository.](https://my.home-assistant.io/badges/supervisor_add_addon_repository.svg)](https://my.home-assistant.io/redirect/supervisor_add_addon_repository/?repository_url=https%3A%2F%2Fgithub.com%2Fgertjana%2Fdoor_dash)

…or do it manually: **Settings → Add-ons → Add-on store → ⋮ →
Repositories**, paste

```
https://github.com/gertjana/door_dash
```

and click **Add**.

### 2. Install the add-on

The store now lists a **reTerminal ePaper Dashboard** section
containing the **ePaper Dashboard** add-on. Open it and click
**Install** — the Supervisor builds the image on the HA host the
first time (a few minutes on a Raspberry Pi).

### 3. Configure & start

In the add-on's **Configuration** tab, set at minimum:

* `wifi_ssid` / `wifi_password` — used only to render the on-screen
  Wi-Fi QR for the firmware to scan
* `weather_entity` (e.g. `weather.home`)
* `calendar_entities` — one or more `calendar.*` entity IDs

Start the add-on, then open the **Web UI** (Ingress) to confirm a
preview renders.

### Updating

Bumping `version:` in `addon/config.yaml` on `main` (and adding a
matching section to [`addon/CHANGELOG.md`](addon/CHANGELOG.md)) is
all that's needed. The Supervisor polls the repository and surfaces
an **Update** button in the UI; the changelog section for the new
version is shown in the update dialog.

### Running a dev build alongside the release

The repo can also produce a *dev* tarball that installs as a
**second** add-on on the same Home Assistant host, on a different
host port, with a `(dev)` suffix in the UI. The release add-on
keeps tracking `main` from GitHub; the dev add-on is whatever you
unpack into `/addons/` locally. Both run at the same time, so you
can A/B-compare a feature branch against the published version
without breaking the firmware's image fetches.

The trick is a thin layer of overrides:

| File | Purpose |
|------|---------|
| `addon/config.yaml`, `addon/build.json` | Canonical source. The release flow ships this as-is. |
| `dev/config-override.yaml` | Keys that differ for the dev variant: `name`, `slug`, `description`, `ports` (host port `8100` instead of `8099`). |
| `dev/build-override.json` | Optional `build.json` overrides (just the name/description by default). |
| `scripts/build_addon_tarball.sh` | Deep-merges the overrides on top of the addon sources at pack time and writes `dist/<merged-slug>.tar.gz`. |
| `scripts/_merge_addon_config.py` | YAML/JSON deep-merge helper used by the build script. |

Because the overrides only patch fields, bumping `version:` in
`addon/config.yaml` still flows through to the dev build with no
extra ceremony — the GitHub-driven release update keeps working
exactly as advertised above.

#### Build & deploy the dev tarball

```bash
# Build (overrides are auto-detected from dev/)
scripts/build_addon_tarball.sh
# -> dist/epaper_dashboard_dev.tar.gz

# Push to the HA host
scp dist/epaper_dashboard_dev.tar.gz root@<ha-host>:/tmp/
ssh root@<ha-host> '
    rm -rf /addons/epaper_dashboard_dev &&
    mkdir -p /addons/epaper_dashboard_dev &&
    tar -xzf /tmp/epaper_dashboard_dev.tar.gz -C /addons/epaper_dashboard_dev
'
```

Then in Home Assistant: **Settings → Add-ons → Store → ⋮ →
Check for updates**. The dev add-on appears under **Local
add-ons** as **ePaper Dashboard (dev)**. Install, configure, and
start it just like the release add-on. It binds host port `8100`
(`http://<ha-ip>:8100/dashboard.bmp`) so the firmware on `:8099`
keeps hitting the released version untouched.

To force a vanilla release tarball (e.g. for an air-gapped HA install
that can't reach `github.com`):

```bash
scripts/build_addon_tarball.sh --no-overrides
# -> dist/epaper_dashboard.tar.gz
```

### Rendering notes

The display is 1-bit. The renderer threshold-converts (no dithering) and uses
a custom `draw_crisp_text()` helper that disables Pillow anti-aliasing for
small text (≤ 16 px), so glyphs stay sharp instead of getting eaten by the
threshold pass. Large titles (20–28 px) keep AA since the rounded edges read
fine on the panel.

### Version badge

A tiny `v{addon} · fw{firmware}` badge is rendered in the top-right corner of
every image. The addon version is read at startup from `addon/config.yaml`'s
`version:` field; the firmware version is set in `firmware/reterminal-dashboard.yaml` sent by the device as a `?fw=`
query parameter on every fetch. Bumping either side requires only:

* **Addon**: edit `version:` in `addon/config.yaml`, redeploy
* **Firmware**: edit `firmware_version:` in `firmware/reterminal-dashboard.yaml`,
  reflash (USB or OTA)

This gives you an at-a-glance way to confirm what's actually running on the
panel after a deploy.

### Optionally "ESPHome" in HA

 **ESPHome integration** Settings → Devices & services
 Talks to a running ESPHome device over its Native API (port 6053) and exposes its sensors as HA entities.

### First flash / recovery (USB)

```bash
# 1. Set up a Python venv with esphome (one-time)
python3.13 -m venv ~/.esphome-venv
source ~/.esphome-venv/bin/activate
pip install esphome

# 2. Make sure firmware/secrets.yaml exists with the four required keys
#    (wifi_ssid, wifi_password, api_encryption_key, ota_password).
#    See firmware/secrets.yaml.example.

# 3. Turn the device off and on asgain
# 4. Flash
esphome run firmware/reterminal-dashboard.yaml --device /dev/cu.usbserial-110
```

### Subsequent updates (wireless OTA)

Once the device is running with `dev_mode: "0"`, it deep-sleeps between
refreshes and is unreachable over the network most of the time. To push an
OTA update:

1. Press any of the buttons to wakes the device up
2. From the Mac (no `--device` flag needed; mDNS discovers the device on the network)
   ```bash
   source ~/.esphome-venv/bin/activate
   esphome run firmware/reterminal-dashboard.yaml
   ```
3. Device receives the update, reboots, fetches one image, then sleeps on
   the next cycle.

If mDNS is blocked on your network, supply the device IP explicitly:
`esphome run firmware/reterminal-dashboard.yaml --device 192.168.50.159`.

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
