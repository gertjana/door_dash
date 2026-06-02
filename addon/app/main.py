"""FastAPI app entry point — serves the rendered dashboard image."""

from __future__ import annotations

import logging
import time
from collections import OrderedDict
from threading import Lock

from fastapi import FastAPI, Query, Response
from fastapi.responses import HTMLResponse

from . import __version__ as ADDON_VERSION  # noqa: N812
from .config import Settings, get_settings
from .render.image_io import to_bmp_bytes, to_png_bytes
from .render.pages import PAGES, get_page
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


def _lookup_or_render(
    settings: Settings,
    sensors: LocalSensors,
    fw_version: str | None,
    page_key: str | int | None,
    nocache: bool = False,
) -> tuple[dict, bool]:
    page = get_page(page_key)
    key = (sensors.cache_key(), _bucket(settings), fw_version or "", page.name)
    if not nocache:
        with _cache_lock:
            entry = _cache.get(key)
            if entry is not None:
                # Move to most-recent position
                _cache.move_to_end(key)
                return entry, True

    # Render outside the lock (Pillow + HTTP calls)
    img = page.render(settings, sensors, fw_version=fw_version)
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
    return {"ok": True, "cache_entries": len(_cache), "addon_version": ADDON_VERSION}


@app.get("/dashboard.bmp")
def dashboard_bmp(
    indoor_temp: float | None = Query(default=None),
    indoor_hum: float | None = Query(default=None),
    battery_pct: float | None = Query(default=None),
    fw: str | None = Query(default=None),
    page: str | None = Query(
        default=None,
        description="Page index (0,1,…) or canonical name (e.g. 'dashboard').",
    ),
    nocache: int = Query(
        default=0,
        description="If 1, bypass the image cache and force a fresh render.",
    ),
) -> Response:
    settings = get_settings()
    sensors = _build_sensors(indoor_temp, indoor_hum, battery_pct)
    entry, hit = _lookup_or_render(settings, sensors, fw, page, nocache=bool(nocache))
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
    fw: str | None = Query(default=None),
    page: str | None = Query(
        default=None,
        description="Page index (0,1,…) or canonical name (e.g. 'dashboard').",
    ),
    nocache: int = Query(
        default=0,
        description="If 1, bypass the image cache and force a fresh render.",
    ),
) -> Response:
    settings = get_settings()
    sensors = _build_sensors(indoor_temp, indoor_hum, battery_pct)
    entry, hit = _lookup_or_render(settings, sensors, fw, page, nocache=bool(nocache))
    return Response(
        content=entry["png"],
        media_type="image/png",
        headers={"X-Cache": "hit" if hit else "miss"},
    )


@app.get("/pages")
def list_pages() -> dict:
    """List registered pages so the firmware can discover the page count."""
    return {
        "count": len(PAGES),
        "pages": [{"index": p.index, "name": p.name, "title": p.title} for p in PAGES],
    }


@app.post("/refresh")
def refresh() -> dict:
    with _cache_lock:
        _cache.clear()
    return {"ok": True}


@app.get("/debug")
def debug() -> dict:
    """Dump live config + probe HA for weather/calendar entities.

    Useful for diagnosing "why is the dashboard showing fallback data?"
    issues. Probes each configured calendar in two ways:

    * ``probe`` — does the entity exist? (calls ``/api/states/<entity>``)
    * ``event_fetch`` — does the calendar API return events for the next
      30 days? (calls ``/api/calendars/<entity>?start=...&end=...``)

    The second probe is what the dashboard actually uses, so if ``probe``
    says ``ok`` but ``event_fetch`` returns ``0 events`` or an HTTP error,
    that's the source of the "no events visible" bug.
    """
    from datetime import UTC, datetime, timedelta

    import httpx

    from .ha_client import HAClient

    settings = get_settings()
    ha = HAClient(settings)
    weather_state = ha.get_state(settings.weather_entity)

    # Probe configured calendars (existence check via /api/states)
    calendar_probe = {}
    for cal in settings.calendar_entities:
        state = ha.get_state(cal)
        calendar_probe[cal] = "ok" if state else "missing"

    # Probe configured calendars (real API fetch — what the dashboard uses).
    # This is the diagnostic that matters when the integration is healthy
    # (entity returns ok) but the dashboard still shows no events.
    now = datetime.now(UTC)
    end_window = now + timedelta(days=30)
    start_iso = now.isoformat()
    end_iso = end_window.isoformat()
    event_fetch: dict[str, dict] = {}
    for cal in settings.calendar_entities:
        url = f"{settings.ha_base_url.rstrip('/')}/api/calendars/{cal}"
        info: dict = {"url": url}
        try:
            r = httpx.get(
                url,
                headers={"Authorization": f"Bearer {settings.supervisor_token}"},
                params={"start": start_iso, "end": end_iso},
                timeout=10.0,
            )
            info["status"] = r.status_code
            if r.status_code == 200:
                data = r.json() or []
                info["count"] = len(data)
                # Include the first 3 raw events so we can see what HA
                # actually returns — useful for diagnosing parsing bugs.
                info["sample"] = data[:3]
            else:
                # Capture the error body so we can see auth/permission
                # failures, missing-entity 404s, etc.
                info["error_body"] = r.text[:500]
        except httpx.HTTPError as e:
            info["error"] = f"{type(e).__name__}: {e}"
        event_fetch[cal] = info

    # Discover all calendar.* entities so the user can spot typos in
    # `calendar_entities` config vs. real entity IDs.
    discovered = []
    try:
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
        "calendar_entities_event_fetch": event_fetch,
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
      button { cursor:pointer; }
      .nav { display:inline-flex; align-items:center; gap:8px; }
      .nav button { font-size:18px; padding:2px 12px; }
      .pageinfo { font-variant-numeric: tabular-nums; opacity:.85; min-width:14em; display:inline-block; }
      kbd { background:#444; padding:1px 6px; border-radius:3px; font-size:11px; }
    </style></head>
    <body>
      <div class="bar">
        <strong>reTerminal E1001 dashboard preview</strong> —
        <a href="dashboard.png">dashboard.png</a> ·
        <a href="dashboard.bmp">dashboard.bmp</a> ·
        <a href="pages">pages</a> ·
        <a href="javascript:fetch('refresh',{method:'POST'}).then(()=>location.reload())">force refresh</a>
      </div>
      <div class="bar nav">
        <button id="prev" title="Previous page (\u2190)">\u25c0</button>
        <span class="pageinfo" id="pageinfo">page \u2026</span>
        <button id="next" title="Next page (\u2192)">\u25b6</button>
        <span style="margin-left:16px;opacity:.6">arrow keys also work</span>
      </div>
      <div class="bar">
        <form onsubmit="event.preventDefault(); reload();">
          <label>Temp \u00b0C</label><input id="t" type="number" step="0.1" value="21.3">
          <label>Hum %</label><input id="h" type="number" step="1" value="48">
          <label>Bat %</label><input id="b" type="number" step="1" value="87">
          <label>fw</label><input id="fw" type="text" style="width:80px" value="0.3.0">
          <button>Preview</button>
          <button type="button" onclick="document.getElementById('t').value='';document.getElementById('h').value='';document.getElementById('b').value='';reload();">Cold boot (no sensors)</button>
        </form>
      </div>
      <img id="img" src="dashboard.png">
      <script>
        let pages = [];
        let idx = 0;

        async function loadPages() {
          try {
            const r = await fetch('pages');
            const j = await r.json();
            pages = j.pages || [];
          } catch (e) {
            pages = [{index:0, name:'dashboard', title:'Dashboard'}];
          }
          updateInfo();
          reload();
        }

        function updateInfo() {
          const p = pages[idx] || {title:'(none)', name:'?'};
          const info = document.getElementById('pageinfo');
          info.textContent = `page ${idx + 1}/${pages.length} \u2014 ${p.title}`;
        }

        function step(delta) {
          if (!pages.length) return;
          idx = (idx + delta + pages.length) % pages.length;
          updateInfo();
          reload();
        }

        function reload() {
          const t = document.getElementById('t').value;
          const h = document.getElementById('h').value;
          const b = document.getElementById('b').value;
          const fw = document.getElementById('fw').value;
          const qs = new URLSearchParams();
          if (t !== '') qs.set('indoor_temp', t);
          if (h !== '') qs.set('indoor_hum', h);
          if (b !== '') qs.set('battery_pct', b);
          if (fw !== '') qs.set('fw', fw);
          if (pages[idx]) qs.set('page', pages[idx].name);
          qs.set('_', Date.now());
          document.getElementById('img').src = 'dashboard.png?' + qs.toString();
        }

        document.getElementById('prev').addEventListener('click', () => step(-1));
        document.getElementById('next').addEventListener('click', () => step(+1));
        document.addEventListener('keydown', (e) => {
          // Ignore arrow keys when typing in an input.
          if (e.target.tagName === 'INPUT') return;
          if (e.key === 'ArrowLeft') step(-1);
          else if (e.key === 'ArrowRight') step(+1);
        });

        loadPages();
      </script>
    </body></html>
    """
