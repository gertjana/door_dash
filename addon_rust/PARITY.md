# Parity report: Rust port vs. Python addon

Phase 9 of the Rust port. Captures the state of the Rust addon
(`addon_rust/`, slug `epaper_dashboard_rust`) measured against the
Python addon (`addon/`, slug `epaper_dashboard`) running the same
HEAD on the same machine.

Both addons booted with `EPDASH_HA_BASE_URL=http://127.0.0.1:1` so HA
fetches refuse instantly and every page exercises its fallback path.
That keeps the test reproducible (no live data drift) and exercises
the same code paths the firmware sees during a brief HA outage.

## What matches

| Surface | Status |
| --- | --- |
| `GET /healthz` | Same JSON shape — `ok`, `cache_entries`, `addon_version`. Key order differs (insertion vs. sort), semantics identical. |
| `GET /pages` | Byte-identical JSON: same `count`, same `pages[]` array (`index`, `name`, `title`). |
| `POST /refresh` | Both return `{"ok":true}`. |
| `GET /dashboard.bmp` | Always 48062 bytes (1-bit packed BMP, 800×480). Same MIME (`image/bmp`), same wire format the firmware decodes. |
| `GET /dashboard.png` | Same MIME (`image/png`), same dimensions. Byte sizes differ — Rust uses the `image` crate's default deflate level while Python uses Pillow's; both round-trip cleanly. |
| Query params | Both accept `indoor_temp`, `indoor_hum`, `battery_pct`, `fw`, `page`, `nocache`. Same defaults, same wraparound on numeric `page`. |
| Cache semantics | Both keep an LRU keyed on `(sensors-rounded, time-bucket, fw, page-name)`; `nocache=1` bypasses; concurrent BMP+PNG of the same page share the cache slot. |

## What differs

* **`GET /debug`** — Python exposes a debug endpoint that probes HA
  connectivity, lists detected entities and dumps the active config.
  Rust returns 404. Firmware never calls it; it's an operator
  convenience and was deferred. Easy to add later if needed.
* **BMP pixel content** — All five pages produce 48062-byte BMPs,
  but 4–12% of byte positions differ between Python and Rust output
  (heart 1872 / 48000 ≈ 3.9%, dashboard 5659 / 48000 ≈ 12%). Visual
  inspection on 800×480 1-bit shows the layouts are equivalent; the
  diffs come from font hinting (`ab_glyph` vs Pillow/FreeType), and
  from the few places where each renderer happens to land a sub-pixel
  glyph on a different mono-threshold side.

## Performance

End-to-end time for a cold render (release build, M-series Mac,
`?nocache=1`):

| Page | Python | Rust | Speedup |
| --- | --- | --- | --- |
| dashboard | 389 ms | 4.6 ms | ~85× |
| calendar | 56 ms | 2.1 ms | ~27× |
| weather | 86 ms | 2.7 ms | ~32× |
| heart | 12 ms | 1.7 ms | ~7× |
| energy | 56 ms | 2.2 ms | ~25× |

Rust wins by an order of magnitude even though most of the heavy
lifting in both is glyph rasterisation. Worth re-measuring on
aarch64 (Pi/HA host) since absolute numbers there matter more.

## Reproducing

```sh
# Python
EPDASH_HA_BASE_URL=http://127.0.0.1:1 \
  addon/.venv/bin/python -m uvicorn app.main:app --app-dir addon \
  --host 127.0.0.1 --port 8099 &

# Rust
EPDASH_PORT=8101 EPDASH_HA_BASE_URL=http://127.0.0.1:1 \
  addon_rust/target/release/epaper-dashboard-rust &

# Compare
for p in dashboard calendar weather heart energy; do
  curl -sS -o "py-$p.bmp" "http://127.0.0.1:8099/dashboard.bmp?page=$p&nocache=1"
  curl -sS -o "ru-$p.bmp" "http://127.0.0.1:8101/dashboard.bmp?page=$p&nocache=1"
  cmp "py-$p.bmp" "ru-$p.bmp" || echo "$p differs"
done
```
