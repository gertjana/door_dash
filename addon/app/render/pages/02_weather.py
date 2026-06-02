"""Page 2 — full-screen weather.

Three-row layout::

    +------------------------------------------------------------------+
    | [ICON] Partly cloudy | 18.5 °C        | [gauge] Pressure  1014   |
    |        7 hours ago   | max 19 / min 14| [drop]  Humidity    66 % |
    |                                       | [wind]  Wind 13 km/h WNW |
    |------------------------------------------------------------------|
    | Hourly forecast                                                  |
    | Sun                Mon                                           |
    | 21:00 22:00 23:00 0:00 1:00 2:00 3:00 4:00                       |
    | [ic]  [ic]  [ic]  [ic] [ic] [ic] [ic] [ic]                       |
    | 17.6° 16.8° 16.2° 15.9° 15.5° 15.1° 14.7° 14.3°                  |
    |------------------------------------------------------------------|
    | Weekly forecast                                                  |
    | Mon  Tue  Wed  Thu  Fri  Sat  Sun                                |
    | [ic] [ic] [ic] [ic] [ic] [ic] [ic]                               |
    | 19°  21°  20°  18°  17°  16°  15°                                |
    | 12°  13°  14°  11°  10°   9°   8°                                |
    +------------------------------------------------------------------+

All data comes from a single ``sources.weather.fetch`` call. Stats
column shows whatever attributes the HA entity exposes; missing values
render as an em-dash so partial data still produces a sensible layout.
"""  # noqa: N999

from __future__ import annotations

from datetime import datetime
from typing import TYPE_CHECKING
from zoneinfo import ZoneInfo

from PIL import Image, ImageDraw

from ...sources import weather as weather_src
from ...sources.weather import bearing_to_cardinal
from ..badge import draw_version_badge
from ..fonts import draw_crisp_text, font
from ..icons import draw_icon, icon_for_weather_state

if TYPE_CHECKING:
    from ...config import Settings
    from ...sources.local_sensors import LocalSensors
    from ...sources.weather import ForecastEntry, Weather

TITLE = "Weather"

# Layout tunables — keep all the magic numbers in one place so the
# layout stays easy to retune without hunting through the render code.
SIDE_INSET = 24
TOP_INSET = 14
HERO_HEIGHT = 140  # row 1: hero + stats merged
HOURLY_HEIGHT = 160  # row 2: hourly forecast strip
# Row 3 (weekly) takes whatever vertical space remains.
SECTION_GAP = 12  # gap above/below the separator rule between rows
FORECAST_LABEL_HEIGHT = 26  # heading + breathing room within a row
HOURLY_COL_MIN = 86  # min width per hourly column
WEEKLY_COL_MIN = 88  # min width per weekly column
STATS_ROW_GAP = 4

# Hero row column splits (fractions of the available width).
HERO_LEFT_FRAC = 0.34
HERO_MID_FRAC = 0.30
# Right column gets the remainder (≈ 0.36).


def _tz(settings: Settings) -> ZoneInfo:
    try:
        return ZoneInfo(settings.timezone)
    except Exception:
        return ZoneInfo("UTC")


def _fmt_temp(t: float | None, unit: str = "°") -> str:
    if t is None:
        return "—"
    return f"{t:.1f}{unit}"


def _fmt_temp_int(t: float | None, unit: str = "°") -> str:
    if t is None:
        return "—"
    return f"{round(t)}{unit}"


def _humanise_age(then: datetime | None, now: datetime) -> str | None:
    """Render a "N minutes/hours/days ago" string. None if no timestamp."""
    if then is None:
        return None
    if then.tzinfo is None:
        then = then.replace(tzinfo=now.tzinfo)
    delta = now - then
    seconds = int(delta.total_seconds())
    if seconds < 0:
        return "just now"
    if seconds < 60:
        return "just now"
    minutes = seconds // 60
    if minutes < 60:
        return f"{minutes} minute{'s' if minutes != 1 else ''} ago"
    hours = minutes // 60
    if hours < 24:
        return f"{hours} hour{'s' if hours != 1 else ''} ago"
    days = hours // 24
    return f"{days} day{'s' if days != 1 else ''} ago"


def _draw_hero_left(
    draw: ImageDraw.ImageDraw,
    weather: Weather,
    settings: Settings,
    x: int,
    y: int,
    w: int,
    h: int,
) -> None:
    """Left column of the hero row: weather icon + condition + age."""
    icon_size = int(h * 0.62)
    icon_x = x
    icon_y = y + (h - icon_size) // 2
    icon_name = weather.icon or icon_for_weather_state(weather.condition)
    draw_icon(draw, icon_name, icon_x, icon_y, icon_size)

    cond_label = weather.condition.replace("_", " ").replace("-", " ").title()
    cond_f = font(26, bold=True)
    cond_bbox = draw.textbbox((0, 0), cond_label, font=cond_f)
    cond_h = cond_bbox[3] - cond_bbox[1]

    age_text = _humanise_age(weather.last_updated, datetime.now(_tz(settings)))
    age_f = font(14)
    age_h = age_f.size

    block_h = cond_h + 6 + age_h
    block_top = y + (h - block_h) // 2
    text_x = icon_x + icon_size + 12
    # Truncate condition to the column width so a long state label
    # ("Partly Cloudy Showers") doesn't bleed into the centre column.
    avail = max(0, x + w - text_x)
    cond_label = _truncate_to_width(draw, cond_label, cond_f, avail)

    draw.text((text_x, block_top), cond_label, font=cond_f, fill=0)
    if age_text:
        draw_crisp_text(draw, (text_x, block_top + cond_h + 6), age_text, age_f, fill=0)


def _draw_hero_middle(
    draw: ImageDraw.ImageDraw,
    weather: Weather,
    x: int,
    y: int,
    w: int,
    h: int,
) -> None:
    """Middle column of the hero row: big current temperature + hi/lo line."""
    temp_text = _fmt_temp(weather.temperature, " " + weather.temperature_unit)
    temp_f = font(48, bold=False)
    temp_bbox = draw.textbbox((0, 0), temp_text, font=temp_f)
    temp_w = temp_bbox[2] - temp_bbox[0]
    temp_h = temp_bbox[3] - temp_bbox[1]

    upcoming = weather.forecast or [None]
    today_fc = upcoming[0]
    if today_fc and today_fc.temp_high is not None and today_fc.temp_low is not None:
        hilo_text = (
            f"max {today_fc.temp_high:.1f} {weather.temperature_unit}"
            f"  /  min {today_fc.temp_low:.1f} {weather.temperature_unit}"
        )
    else:
        hilo_text = ""

    hilo_f = font(16)
    hilo_bbox = draw.textbbox((0, 0), hilo_text, font=hilo_f) if hilo_text else (0, 0, 0, 0)
    hilo_w = hilo_bbox[2] - hilo_bbox[0]
    hilo_h = hilo_bbox[3] - hilo_bbox[1] if hilo_text else 0

    temp_hilo_gap = 18
    block_h = temp_h + (temp_hilo_gap + hilo_h if hilo_text else 0)
    block_top = y + (h - block_h) // 2

    # Centre-align within the middle column for visual balance.
    temp_x = x + (w - temp_w) // 2
    draw.text((temp_x, block_top), temp_text, font=temp_f, fill=0)
    if hilo_text:
        hilo_x = x + (w - hilo_w) // 2
        draw_crisp_text(
            draw,
            (hilo_x, block_top + temp_h + temp_hilo_gap),
            hilo_text,
            hilo_f,
            fill=0,
        )


def _draw_hero_stats(
    draw: ImageDraw.ImageDraw,
    weather: Weather,
    x: int,
    y: int,
    w: int,
    h: int,
) -> None:
    """Right column of the hero row: three stacked stat rows.

    Compact compared to the previous full-width stats strip — icons and
    fonts are shrunk so the three rows fit inside the hero's height.
    """
    cardinal = bearing_to_cardinal(weather.wind_bearing)
    if weather.wind_speed is None:
        wind_value = "—"
    elif cardinal:
        wind_value = f"{round(weather.wind_speed)} {weather.wind_unit} ({cardinal})"
    else:
        wind_value = f"{round(weather.wind_speed)} {weather.wind_unit}"

    pressure_value = (
        f"{weather.pressure:,.0f} {weather.pressure_unit}" if weather.pressure is not None else "—"
    )
    humidity_value = f"{round(weather.humidity)} %" if weather.humidity is not None else "—"

    rows: list[tuple[str, str, str]] = [
        ("gauge", "Pressure", pressure_value),
        ("water-percent", "Humidity", humidity_value),
        ("weather-windy", "Wind", wind_value),
    ]

    n = len(rows)
    row_h = (h - STATS_ROW_GAP * (n - 1)) // n
    icon_size = min(row_h - 6, 26)
    label_f = font(14)
    value_f = font(15, bold=True)

    for i, (icon_name, label, value) in enumerate(rows):
        row_y = y + i * (row_h + STATS_ROW_GAP)
        icon_y = row_y + (row_h - icon_size) // 2
        draw_icon(draw, icon_name, x, icon_y, icon_size)

        label_bbox = draw.textbbox((0, 0), label, font=label_f)
        label_h = label_bbox[3] - label_bbox[1]
        label_x = x + icon_size + 8
        label_y = row_y + (row_h - label_h) // 2 - 1
        draw_crisp_text(draw, (label_x, label_y), label, label_f, fill=0)

        value_bbox = draw.textbbox((0, 0), value, font=value_f)
        value_w = value_bbox[2] - value_bbox[0]
        value_h = value_bbox[3] - value_bbox[1]
        value_x = x + w - value_w
        value_y = row_y + (row_h - value_h) // 2 - 1
        draw.text((value_x, value_y), value, font=value_f, fill=0)


def _truncate_to_width(
    draw: ImageDraw.ImageDraw,
    text: str,
    f,
    max_w: int,
) -> str:
    """Trim ``text`` with an ellipsis so its rendered width fits ``max_w``."""
    if max_w <= 0:
        return ""
    bbox = draw.textbbox((0, 0), text, font=f)
    if bbox[2] - bbox[0] <= max_w:
        return text
    ell = "…"
    while text and draw.textbbox((0, 0), text + ell, font=f)[2] > max_w:
        text = text[:-1]
    return (text + ell) if text else ""


def _draw_hero(
    draw: ImageDraw.ImageDraw,
    weather: Weather,
    settings: Settings,
    x: int,
    y: int,
    w: int,
    h: int,
) -> None:
    """Compose the merged hero row from the three column helpers.

    The split is fractional so the row scales gracefully if the canvas
    width changes; a thin vertical rule between columns keeps the eye
    from running across the row.
    """
    left_w = int(w * HERO_LEFT_FRAC)
    mid_w = int(w * HERO_MID_FRAC)
    right_w = w - left_w - mid_w

    # Slim vertical rules between columns. We inset them slightly from
    # the row's top/bottom so they read as separators rather than borders.
    rule_pad = 8
    rule_x1 = x + left_w
    rule_x2 = x + left_w + mid_w
    draw.line((rule_x1, y + rule_pad, rule_x1, y + h - rule_pad), fill=0, width=1)
    draw.line((rule_x2, y + rule_pad, rule_x2, y + h - rule_pad), fill=0, width=1)

    _draw_hero_left(draw, weather, settings, x, y, left_w - 6, h)
    _draw_hero_middle(draw, weather, x + left_w + 6, y, mid_w - 12, h)
    _draw_hero_stats(draw, weather, x + left_w + mid_w + 8, y, right_w - 8, h)


def _draw_hourly(
    draw: ImageDraw.ImageDraw,
    weather: Weather,
    settings: Settings,
    x: int,
    y: int,
    w: int,
    h: int,
) -> None:
    """Hourly forecast strip: heading + columns of hour/icon/temp."""
    head_f = font(18, bold=True)
    draw_crisp_text(draw, (x, y), "Hourly forecast", head_f, fill=0)
    grid_top = y + FORECAST_LABEL_HEIGHT

    tz = _tz(settings)
    now_local = datetime.now(tz)

    hour_floor = now_local.replace(minute=0, second=0, microsecond=0)
    upcoming: list[tuple[ForecastEntry, datetime]] = []
    for f in weather.forecast_hourly or []:
        when_aware = f.when if f.when.tzinfo else f.when.replace(tzinfo=tz)
        when_local = when_aware.astimezone(tz)
        if when_local >= hour_floor:
            upcoming.append((f, when_local))
    if not upcoming:
        msg = "No hourly forecast available."
        mf = font(16)
        mb = draw.textbbox((0, 0), msg, font=mf)
        mw = mb[2] - mb[0]
        mh = mb[3] - mb[1]
        draw_crisp_text(
            draw,
            (x + (w - mw) // 2, grid_top + (h - FORECAST_LABEL_HEIGHT - mh) // 2),
            msg,
            mf,
            fill=0,
        )
        return

    max_cols = max(1, min(len(upcoming), w // HOURLY_COL_MIN))
    cols = upcoming[:max_cols]
    col_w = w // max_cols

    day_f = font(14, bold=True)
    hour_f = font(14)
    temp_f = font(16, bold=True)
    icon_sz = 32

    inner_top = grid_top
    day_h = day_f.size + 2
    hour_h = hour_f.size + 2
    prev_day = None
    for i, (f, when_local) in enumerate(cols):
        cx = x + i * col_w

        this_day = when_local.date()
        if i == 0 or this_day != prev_day:
            day_label = when_local.strftime("%a")
            db = draw.textbbox((0, 0), day_label, font=day_f)
            dw = db[2] - db[0]
            draw_crisp_text(draw, (cx + (col_w - dw) // 2, inner_top), day_label, day_f, fill=0)
        prev_day = this_day

        hour_label = f"{when_local.hour}:{when_local.minute:02d}"
        hb = draw.textbbox((0, 0), hour_label, font=hour_f)
        hw = hb[2] - hb[0]
        hour_y = inner_top + day_h
        draw_crisp_text(draw, (cx + (col_w - hw) // 2, hour_y), hour_label, hour_f, fill=0)

        icon_y = hour_y + hour_h + 2
        icon_x = cx + (col_w - icon_sz) // 2
        draw_icon(draw, icon_for_weather_state(f.condition), icon_x, icon_y, icon_sz)

        t_text = _fmt_temp(f.temp_high, "°") if f.temp_high is not None else "—"
        tb = draw.textbbox((0, 0), t_text, font=temp_f)
        tw = tb[2] - tb[0]
        t_y = icon_y + icon_sz + 4
        draw_crisp_text(draw, (cx + (col_w - tw) // 2, t_y), t_text, temp_f, fill=0)


def _draw_weekly(
    draw: ImageDraw.ImageDraw,
    weather: Weather,
    settings: Settings,
    x: int,
    y: int,
    w: int,
    h: int,
) -> None:
    """Weekly forecast strip: heading + 5-7 daily columns."""
    head_f = font(18, bold=True)
    draw_crisp_text(draw, (x, y), "Weekly forecast", head_f, fill=0)
    grid_top = y + FORECAST_LABEL_HEIGHT

    tz = _tz(settings)
    today = datetime.now(tz).date()

    daily: list[tuple[ForecastEntry, datetime]] = []
    for f in weather.forecast or []:
        when_aware = f.when if f.when.tzinfo else f.when.replace(tzinfo=tz)
        when_local = when_aware.astimezone(tz)
        # Skip past entries (some providers include "today" with already-elapsed data).
        if when_local.date() < today:
            continue
        daily.append((f, when_local))
    if not daily:
        msg = "No weekly forecast available."
        mf = font(16)
        mb = draw.textbbox((0, 0), msg, font=mf)
        mw = mb[2] - mb[0]
        mh = mb[3] - mb[1]
        draw_crisp_text(
            draw,
            (x + (w - mw) // 2, grid_top + (h - FORECAST_LABEL_HEIGHT - mh) // 2),
            msg,
            mf,
            fill=0,
        )
        return

    max_cols = max(1, min(len(daily), w // WEEKLY_COL_MIN))
    cols = daily[:max_cols]
    col_w = w // max_cols

    day_f = font(14, bold=True)
    hi_f = font(15, bold=True)
    lo_f = font(14)
    icon_sz = 32

    day_h = day_f.size + 2
    hi_h = hi_f.size + 2
    inner_top = grid_top

    for i, (f, when_local) in enumerate(cols):
        cx = x + i * col_w

        # Today shows "Today" instead of the weekday for clarity.
        day_label = "Today" if when_local.date() == today else when_local.strftime("%a")
        db = draw.textbbox((0, 0), day_label, font=day_f)
        dw = db[2] - db[0]
        draw_crisp_text(draw, (cx + (col_w - dw) // 2, inner_top), day_label, day_f, fill=0)

        icon_y = inner_top + day_h + 2
        icon_x = cx + (col_w - icon_sz) // 2
        draw_icon(draw, icon_for_weather_state(f.condition), icon_x, icon_y, icon_sz)

        hi_text = _fmt_temp_int(f.temp_high, "°")
        lo_text = _fmt_temp_int(f.temp_low, "°")
        hi_bbox = draw.textbbox((0, 0), hi_text, font=hi_f)
        lo_bbox = draw.textbbox((0, 0), lo_text, font=lo_f)
        hi_w = hi_bbox[2] - hi_bbox[0]
        lo_w = lo_bbox[2] - lo_bbox[0]
        hi_y = icon_y + icon_sz + 4
        lo_y = hi_y + hi_h
        draw_crisp_text(draw, (cx + (col_w - hi_w) // 2, hi_y), hi_text, hi_f, fill=0)
        draw_crisp_text(draw, (cx + (col_w - lo_w) // 2, lo_y), lo_text, lo_f, fill=0)


def render(
    settings: Settings,
    sensors: LocalSensors,
    fw_version: str | None = None,
) -> Image.Image:
    """Draw the full-screen weather page."""
    w, h = settings.width, settings.height
    img = Image.new("L", (w, h), color=255)
    draw = ImageDraw.Draw(img)

    weather = weather_src.fetch(settings)

    x = SIDE_INSET
    avail_w = w - 2 * SIDE_INSET
    cursor_y = TOP_INSET

    # Row 1: hero + stats merged.
    _draw_hero(draw, weather, settings, x, cursor_y, avail_w, HERO_HEIGHT)
    cursor_y += HERO_HEIGHT + SECTION_GAP
    draw.line((x, cursor_y, x + avail_w, cursor_y), fill=0, width=1)
    cursor_y += SECTION_GAP

    # Row 2: hourly forecast.
    _draw_hourly(draw, weather, settings, x, cursor_y, avail_w, HOURLY_HEIGHT)
    cursor_y += HOURLY_HEIGHT + SECTION_GAP
    draw.line((x, cursor_y, x + avail_w, cursor_y), fill=0, width=1)
    cursor_y += SECTION_GAP

    # Row 3: weekly forecast — flexes to absorb whatever vertical space remains.
    weekly_h = h - cursor_y - TOP_INSET
    _draw_weekly(draw, weather, settings, x, cursor_y, avail_w, weekly_h)

    # Version badge in the top-right corner (shared helper).
    draw_version_badge(img, settings, fw_version, draw=draw)

    return img
