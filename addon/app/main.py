"""FastAPI app entry point — serves the rendered dashboard image."""

from __future__ import annotations

import logging
import time
from collections import OrderedDict
from threading import Lock

from fastapi import FastAPI, Query, Response
from fastapi.responses import HTMLResponse

from .config import Settings, get_settings
from .render.image_io import to_bmp_bytes, to_png_bytes
from .render.layout import compose
from .sources.local_sensors import LocalSensors

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s %(levelname)s %(name)s: %(message)s",
)

app = FastAPI(title="reTerminal ePaper Dashboard")

# LRU cache: key = (sensor cache_key, minute_bucket) -> {ts, bmp, png}
_CACHE_MAX = 8
_cache: OrderedDict[tuple, dict] = OrderedDict()
_cache_lock = Lock()


def _bucket(settings: Settings) -> int:
    """Time bucket so the cache expires roughly every refresh_cache_seconds."""
    return int(time.time() // max(1, settings.refresh_cache_seconds))


def _lookup_or_render(settings: Settings, sensors: LocalSensors) -> tuple[dict, bool]:
    key = (sensors.cache_key(), _bucket(settings))
    with _cache_lock:
        entry = _cache.get(key)
        if entry is not None:
            # Move to most-recent position
            _cache.move_to_end(key)
            return entry, True

    # Render outside the lock (Pillow + HTTP calls)
    img = compose(settings, sensors)
    entry = {
        "ts": time.time(),
        "bmp": to_bmp_bytes(img),
        "png": to_png_bytes(img),
    }
    with _cache_lock:
        _cache[key] = entry
        _cache.move_to_end(key)
        while len(_cache) > _CACHE_MAX:
            _cache.popitem(last=False)
    return entry, False


def _build_sensors(
    indoor_temp: float | None,
    indoor_hum: float | None,
    battery_pct: float | None,
) -> LocalSensors:
    return LocalSensors.from_query(
        indoor_temp=indoor_temp,
        indoor_hum=indoor_hum,
        battery_pct=battery_pct,
    )


@app.get("/healthz")
def healthz() -> dict:
    return {"ok": True, "cache_entries": len(_cache)}


@app.get("/dashboard.bmp")
def dashboard_bmp(
    indoor_temp: float | None = Query(default=None),
    indoor_hum: float | None = Query(default=None),
    battery_pct: float | None = Query(default=None),
) -> Response:
    settings = get_settings()
    sensors = _build_sensors(indoor_temp, indoor_hum, battery_pct)
    entry, hit = _lookup_or_render(settings, sensors)
    return Response(
        content=entry["bmp"],
        media_type="image/bmp",
        headers={"X-Cache": "hit" if hit else "miss"},
    )


@app.get("/dashboard.png")
def dashboard_png(
    indoor_temp: float | None = Query(default=None),
    indoor_hum: float | None = Query(default=None),
    battery_pct: float | None = Query(default=None),
) -> Response:
    settings = get_settings()
    sensors = _build_sensors(indoor_temp, indoor_hum, battery_pct)
    entry, hit = _lookup_or_render(settings, sensors)
    return Response(
        content=entry["png"],
        media_type="image/png",
        headers={"X-Cache": "hit" if hit else "miss"},
    )


@app.post("/refresh")
def refresh() -> dict:
    with _cache_lock:
        _cache.clear()
    return {"ok": True}


@app.get("/debug")
def debug() -> dict:
    """Dump live config + probe the HA weather entity + list calendar entities.
    Useful for diagnosing 'why is the dashboard showing fallback data?' issues.
    """
    from .ha_client import HAClient

    settings = get_settings()
    ha = HAClient(settings)
    weather_state = ha.get_state(settings.weather_entity)
    # Probe configured calendars
    calendar_probe = {}
    for cal in settings.calendar_entities:
        state = ha.get_state(cal)
        calendar_probe[cal] = "ok" if state else "missing"
    # Discover all calendar.* entities
    discovered = []
    try:
        import httpx

        r = httpx.get(
            f"{settings.ha_base_url}/api/states",
            headers={"Authorization": f"Bearer {settings.supervisor_token}"},
            timeout=8.0,
        )
        if r.status_code == 200:
            discovered = [
                s["entity_id"] for s in r.json() if s.get("entity_id", "").startswith("calendar.")
            ]
    except Exception:
        pass
    return {
        "weather_entity": settings.weather_entity,
        "ha_base_url": settings.ha_base_url,
        "ha_token_present": bool(settings.supervisor_token),
        "weather_state_ok": weather_state is not None,
        "weather_temp": (weather_state or {}).get("attributes", {}).get("temperature"),
        "calendar_entities_configured": settings.calendar_entities,
        "calendar_entities_probe": calendar_probe,
        "calendar_entities_discovered": discovered,
        "cache_entries": len(_cache),
    }


@app.get("/", response_class=HTMLResponse)
def index() -> str:
    # NOTE: all URLs below are RELATIVE (no leading slash) so they resolve
    # correctly when served via HA Ingress (which prefixes a long path).
    return """
    <!doctype html><html><head><meta charset="utf-8">
    <title>ePaper Dashboard preview</title>
    <base href="./">
    <style>
      body { font-family: -apple-system, sans-serif; background:#222; color:#eee; margin:0; padding:24px; }
      img { background:#fff; image-rendering: pixelated; max-width:100%; box-shadow:0 4px 24px #0008; }
      a { color:#8cf; }
      .bar { margin-bottom:12px; }
      form { display:inline; margin-right:12px; }
      input { width:60px; }
      label { margin-right:4px; }
    </style></head>
    <body>
      <div class="bar">
        <strong>reTerminal E1001 dashboard preview</strong> —
        <a href="dashboard.png">dashboard.png</a> ·
        <a href="dashboard.bmp">dashboard.bmp</a> ·
        <a href="javascript:fetch('refresh',{method:'POST'}).then(()=>location.reload())">force refresh</a>
      </div>
      <div class="bar">
        <form onsubmit="event.preventDefault(); reload();">
          <label>Temp °C</label><input id="t" type="number" step="0.1" value="21.3">
          <label>Hum %</label><input id="h" type="number" step="1" value="48">
          <label>Bat %</label><input id="b" type="number" step="1" value="87">
          <button>Preview</button>
          <button type="button" onclick="document.getElementById('t').value='';document.getElementById('h').value='';document.getElementById('b').value='';reload();">Cold boot (no sensors)</button>
        </form>
      </div>
      <img id="img" src="dashboard.png">
      <script>
        function reload() {
          const t = document.getElementById('t').value;
          const h = document.getElementById('h').value;
          const b = document.getElementById('b').value;
          const qs = new URLSearchParams();
          if (t !== '') qs.set('indoor_temp', t);
          if (h !== '') qs.set('indoor_hum', h);
          if (b !== '') qs.set('battery_pct', b);
          qs.set('_', Date.now());
          document.getElementById('img').src = 'dashboard.png?' + qs.toString();
        }
        reload();
      </script>
    </body></html>
    """
