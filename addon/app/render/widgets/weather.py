"""Weather widget — current condition (left) + multi-day forecast (right).

Layout (single row beneath the title):

    [icon] [BIG TEMP]  |  Tue  Wed  Thu  Fri
     Cond / details    | [ic] [ic] [ic] [ic]
                       | 22°  20°  17°  19°
"""

from __future__ import annotations

from datetime import datetime
from zoneinfo import ZoneInfo

from PIL import Image, ImageDraw

from ...config import Settings
from ...sources.weather import Weather
from ..fonts import draw_crisp_text, font
from ..icons import draw_icon, icon_for_weather_state
from .base import Box


def _fmt_temp(t, unit: str = "°") -> str:
    if t is None:
        return "—"
    return f"{round(t)}{unit}"


def _today(settings: Settings) -> datetime.date:
    try:
        tz = ZoneInfo(settings.timezone)
    except Exception:
        tz = ZoneInfo("UTC")
    return datetime.now(tz).date()


def render(weather: Weather, img: Image.Image, box: Box, settings: Settings | None = None) -> None:
    draw = ImageDraw.Draw(img)

    title_f = font(20, bold=True)
    draw.text((box.x + 8, box.y + 4), "Weather", font=title_f, fill=0)

    content_top = box.y + 38
    content_bottom = box.y2 - 6

    # ----- Left group: icon + big temperature (top-aligned with forecast) -----
    if weather.temperature is not None:
        temp_text = f"{round(weather.temperature)}{weather.temperature_unit}"
    else:
        temp_text = "—"
    temp_f = font(28, bold=True)
    temp_bbox = draw.textbbox((0, 0), temp_text, font=temp_f)
    temp_w = temp_bbox[2] - temp_bbox[0]
    temp_h = temp_bbox[3] - temp_bbox[1]

    icon_size = 40
    gap = 10
    left_x = box.x + 10

    # Top-align icon + temp with the forecast columns (which start at content_top + 2)
    top_y = content_top + 2

    icon_name = weather.icon or icon_for_weather_state(weather.condition)
    icon_x = left_x
    # Vertically centre the icon against the big temperature text
    icon_y = top_y + (temp_h - icon_size) // 2
    draw_icon(draw, icon_name, icon_x, icon_y, icon_size)

    temp_x = icon_x + icon_size + gap
    temp_y = top_y
    draw.text((temp_x, temp_y), temp_text, font=temp_f, fill=0)

    # ----- Detail lines: left-aligned condition + wind below the icon/temp -----
    detail_f = font(13)
    line_h = detail_f.size + 2
    detail_x = left_x
    detail_y = top_y + temp_h + 10

    cond_label = weather.condition.replace("_", " ").replace("-", " ").title()
    draw_crisp_text(draw, (detail_x, detail_y), cond_label, detail_f)

    if weather.wind_speed is not None:
        wind_line = f"Wind {round(weather.wind_speed)} {weather.wind_unit}"
        wind_y = detail_y + line_h
        if wind_y + detail_f.size <= content_bottom:
            draw_crisp_text(draw, (detail_x, wind_y), wind_line, detail_f)

    left_group_right = temp_x + temp_w
    forecast_x = left_group_right + 16

    # ----- Right group: next-N-days forecast -----
    available_w = box.x2 - 10 - forecast_x
    if available_w < 80:
        return

    today = _today(settings) if settings else datetime.utcnow().date()
    upcoming = [f for f in (weather.forecast or []) if f.when.date() > today]
    if not upcoming:
        return

    # Vertical divider for visual separation
    div_x = forecast_x - 8
    draw.line(
        (div_x, content_top + 2, div_x, content_bottom - 2),
        fill=0,
        width=1,
    )

    # Decide how many days we can fit (min 56px per column)
    col_min = 56
    max_cols = max(1, min(len(upcoming), available_w // col_min))
    cols = upcoming[:max_cols]
    col_w = available_w // max_cols

    day_f = font(12, bold=True)
    icon_sz = 24
    temp_small_f = font(13, bold=True)

    for i, fcast in enumerate(cols):
        cx = forecast_x + i * col_w
        # Day label (e.g. "Tue")
        day_label = fcast.when.strftime("%a")
        day_bbox = draw.textbbox((0, 0), day_label, font=day_f)
        day_w = day_bbox[2] - day_bbox[0]
        day_h = day_bbox[3] - day_bbox[1]
        day_x = cx + (col_w - day_w) // 2
        day_y = content_top + 2
        draw_crisp_text(draw, (day_x, day_y), day_label, day_f)

        # Icon centred
        ic_name = icon_for_weather_state(fcast.condition)
        ic_x = cx + (col_w - icon_sz) // 2
        ic_y = day_y + day_h + 2
        draw_icon(draw, ic_name, ic_x, ic_y, icon_sz)

        # Temp: "high°/low°" or just "high°"
        if fcast.temp_high is not None and fcast.temp_low is not None:
            t_text = f"{round(fcast.temp_high)}°/{round(fcast.temp_low)}°"
        else:
            t_text = _fmt_temp(fcast.temp_high)
        t_bbox = draw.textbbox((0, 0), t_text, font=temp_small_f)
        t_w = t_bbox[2] - t_bbox[0]
        t_x = cx + (col_w - t_w) // 2
        t_y = ic_y + icon_sz + 1
        if t_y + (t_bbox[3] - t_bbox[1]) <= content_bottom:
            draw_crisp_text(draw, (t_x, t_y), t_text, temp_small_f)
