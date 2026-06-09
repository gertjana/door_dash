# Changelog

All notable changes to the **ePaper Dashboard** add-on are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project adheres to [Semantic Versioning](https://semver.org/).

## 1.2.0 - 2026-06-09

### Added
- Display battery percentage now appears in the top-right version badge
  on every page (small icon + label, e.g. `■ 87%  v1.2.0 · fw0.3.0`),
  giving a single source of truth for charge state regardless of which
  page is showing.

### Changed
- **Indoors** widget on the dashboard page slimmed down to just the
  Temp/Humidity readings — the redundant battery bar has moved to the
  badge. The freed pixels in the left column are absorbed by the QR and
  Tesla widgets for a slightly more spacious layout.

## 1.1.0 - 2026-06-07

### Added

 - Moved from local development (copy over files to HA) to a repository based addon

## 1.0.9 - 2026-06-07

### Fixed
- Loosen sparkline axis-label spacing so left/right ticks no longer collide
  with the values; the right-hand tick now shows the current clock time.

## 1.0.8 - 2026-06-07

### Changed
- Refine the energy page to a single-phase, consumption-only layout
  (3 rows × 1 column), freeing horizontal space for fully axis-labelled
  sparklines on both the phase rows and the indoor temp/humidity rows.
- Resolve the active timezone from Home Assistant's `/api/config` with a
  60 s cache, falling back to the add-on option and finally UTC. All four
  pages and shared widgets now use the same resolver.

### Removed
- L2/L3 columns and solar-production fields from the energy data layer
  (kept in the schema for backward compatibility — see 1.0.9).

## 1.0.7 - 2026-06-07

First post-port stable release of the Python add-on. The version was
bumped past `1.0.6` so it cleanly supersedes any Rust-port build that may
have been installed locally during development.

### Added
- **Energy page** with DSMR P1 data from the Zuidwijk SlimmeLezer:
  per-phase power, voltage and current, plus 24-hour sparklines and
  cumulative tariff/gas counters. Fetches roughly 17 entities and one
  batched `/api/history/period` call per render.
- **Multi-page dashboard** with auto-discovered `render/pages/*.py`
  modules, a `/pages` endpoint, `?page=N` and `?nocache=1` query params,
  and prev/next + arrow-key navigation in the preview UI.
- **Fullscreen month calendar** as page 1 — a 7×6 Mon-first grid showing
  every event from the configured calendar entities for the month.
  All-day events render as filled black bars; timed events as outlined
  bars prefixed with `HH:MM`. Today is bold; adjacent-month days are
  de-emphasised.
- **Tesla integration** widget driven by configurable HA entities for
  battery, range, inside temperature, and climate state.
- New `calendar.fetch_range()` helper for arbitrary date ranges
  (uncapped by `max_events`, which the month grid needs).

### Changed
- Drop the sparkline baseline when fewer than 3 valid samples are
  available, so noisy short series don't draw a misleading flat line.
- Drop borders around timed events in the month calendar for a cleaner
  look at small sizes.
- Replace the bundled DSMR-Reader firmware with the SlimmeLezer setup;
  corresponding API/OTA secrets removed from the example file.

### Fixed
- All-day events no longer spill onto the next day in the calendar grid.

## 0.2.1 - 2026-05-26

### Fixed
- Bundle `config.yaml` into the add-on image at `/opt/app/config.yaml`
  so the in-app version reader resolves the real version at runtime
  instead of falling back to `0.0.0`.

## 0.2.0 - 2026-05-26

### Added
- Version badge in the top-right corner of the dashboard image. Shows
  both the running add-on version (read from `config.yaml` at import
  time) and the firmware version (sent as a `?fw=` query param by the
  device). Renders as a 10 px crisp `vX.Y.Z · fwX.Y.Z` line.
- Independent semver for add-on and firmware: bumping either side is
  enough to see which is live on the panel.

## 0.1.0 - 2026-05-25

### Added
- Initial release of the ePaper Dashboard add-on.
- Renders an 800×480 1-bit dashboard image for the Seeed reTerminal
  E1001 ePaper display.
- Endpoints: `/` HTML preview, `/dashboard.png`, `/dashboard.bmp`,
  `/healthz`, and `POST /refresh`.
- Sensor query parameters (`indoor_temp`, `indoor_hum`, `battery_pct`)
  pushed by the firmware on each wake feed the "Indoors" widget.
- LRU response cache keyed on rounded sensor values; responses include
  an `X-Cache: hit|miss` header.
- Companion ESPHome firmware fetches the image every 15 minutes.
