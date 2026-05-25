"""Material Design Icons (MDI) glyph rendering for Pillow.

Uses the MDI webfont bundled in `assets/icons/`. Icon names match the
HA / `mdi:...` convention (e.g. `weather-partly-cloudy`).

The codepoint table below is a curated subset — we only ship the names
we actually use so unknown lookups fail loudly during development.
Extend `_CODEPOINTS` to add more.
"""

from __future__ import annotations

from functools import lru_cache
from pathlib import Path

from PIL import ImageDraw, ImageFont

_FONT_PATH = Path(__file__).parent / "assets" / "icons" / "materialdesignicons-webfont.ttf"

# Curated codepoint table. Generated from @mdi/svg meta.json.
# Add more entries as new widgets need them.
_CODEPOINTS: dict[str, str] = {
    # Weather (HA `weather.*` state values map here directly)
    "weather-cloudy": "F0590",
    "weather-fog": "F0591",
    "weather-hail": "F0592",
    "weather-hazy": "F0F30",
    "weather-hurricane": "F0898",
    "weather-lightning": "F0593",
    "weather-lightning-rainy": "F067E",
    "weather-night": "F0594",
    "weather-night-partly-cloudy": "F0F31",
    "weather-partly-cloudy": "F0595",
    "weather-partly-lightning": "F0F32",
    "weather-partly-rainy": "F0F33",
    "weather-partly-snowy": "F0F34",
    "weather-partly-snowy-rainy": "F0F35",
    "weather-pouring": "F0596",
    "weather-rainy": "F0597",
    "weather-snowy": "F0598",
    "weather-snowy-heavy": "F0F36",
    "weather-snowy-rainy": "F067F",
    "weather-sunny": "F0599",
    "weather-sunset": "F059A",
    "weather-tornado": "F0F38",
    "weather-windy": "F059D",
    "weather-windy-variant": "F059E",
    # General
    "alert-circle": "F0028",
    "battery": "F0079",
    "battery-charging": "F0084",
    "battery-outline": "F008E",
    "calendar": "F00ED",
    "cloud": "F015F",
    "help-circle": "F02D7",
    "home": "F02DC",
    "snowflake": "F0717",
    "thermometer": "F050F",
    "umbrella": "F054A",
    "water-percent": "F058E",
}

# Maps Home Assistant `weather.*` state values to MDI icon names.
# Reference: https://www.home-assistant.io/integrations/weather/
HA_WEATHER_STATE_TO_ICON: dict[str, str] = {
    "clear-night": "weather-night",
    "cloudy": "weather-cloudy",
    "exceptional": "alert-circle",
    "fog": "weather-fog",
    "hail": "weather-hail",
    "lightning": "weather-lightning",
    "lightning-rainy": "weather-lightning-rainy",
    "partlycloudy": "weather-partly-cloudy",
    "pouring": "weather-pouring",
    "rainy": "weather-rainy",
    "snowy": "weather-snowy",
    "snowy-rainy": "weather-snowy-rainy",
    "sunny": "weather-sunny",
    "windy": "weather-windy",
    "windy-variant": "weather-windy-variant",
}


@lru_cache(maxsize=32)
def _font(size: int) -> ImageFont.FreeTypeFont:
    return ImageFont.truetype(str(_FONT_PATH), size=size)


def _glyph_for(name: str) -> str | None:
    """Return the unicode character for an MDI icon name, or None."""
    # Strip a leading "mdi:" if present (HA attribute style)
    if name.startswith("mdi:"):
        name = name[4:]
    code = _CODEPOINTS.get(name)
    if not code:
        return None
    return chr(int(code, 16))


def icon_for_weather_state(state: str) -> str:
    """Map HA weather entity state -> MDI icon name (with fallback)."""
    return HA_WEATHER_STATE_TO_ICON.get(state.lower(), "help-circle")


def draw_icon(
    draw: ImageDraw.ImageDraw,
    name: str,
    x: int,
    y: int,
    size: int,
    fill: int = 0,
) -> None:
    """Draw an MDI glyph at (x, y) sized roughly `size`x`size`.

    The MDI font is designed on a 24x24 grid; we use the requested size
    directly as the font px size, which yields a glyph close to that
    square box (with the usual font-metric slack).

    Falls back to `help-circle` when the name is unknown.
    """
    glyph = _glyph_for(name)
    if glyph is None:
        glyph = _glyph_for("help-circle")
        if glyph is None:
            # Last-resort: just draw an outlined box so we see something
            draw.rectangle((x, y, x + size, y + size), outline=fill, width=2)
            return
    f = _font(size)
    draw.text((x, y), glyph, font=f, fill=fill)
