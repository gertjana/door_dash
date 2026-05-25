"""Wi-Fi QR widget.

Encodes Wi-Fi credentials using the de-facto-standard MECARD-style string
`WIFI:T:<auth>;S:<ssid>;P:<password>;H:<true|false>;;` recognised by iOS and
modern Android cameras.
"""

from __future__ import annotations

import qrcode
from PIL import Image, ImageDraw

from ...config import Settings
from ..fonts import font
from .base import Box


def _escape(value: str) -> str:
    # MECARD escaping: backslash, semicolon, comma, colon, double-quote
    out = []
    for ch in value:
        if ch in ("\\", ";", ",", ":", '"'):
            out.append("\\" + ch)
        else:
            out.append(ch)
    return "".join(out)


def _payload(settings: Settings) -> str:
    sec_raw = (settings.wifi_security or "nopass").strip()
    sec_upper = sec_raw.upper()
    # Normalise: accept case insensitive "nopass", "open", "none" -> nopass
    sec = "nopass" if sec_upper in ("NOPASS", "OPEN", "NONE", "") else sec_upper

    ssid = _escape(settings.wifi_ssid)
    pwd = _escape(settings.wifi_password or "")
    hidden = "true" if settings.wifi_hidden else "false"

    if sec == "nopass":
        return f"WIFI:T:nopass;S:{ssid};H:{hidden};;"
    return f"WIFI:T:{sec};S:{ssid};P:{pwd};H:{hidden};;"


def render(settings: Settings, img: Image.Image, box: Box) -> None:
    draw = ImageDraw.Draw(img)

    title_f = font(20, bold=True)
    draw.text((box.x + 8, box.y + 4), "Wi-Fi", font=title_f, fill=0)

    qr = qrcode.QRCode(
        version=None,
        error_correction=qrcode.constants.ERROR_CORRECT_M,
        box_size=10,
        border=2,
    )
    qr.add_data(_payload(settings))
    qr.make(fit=True)
    qr_img = qr.make_image(fill_color="black", back_color="white").convert("L")

    # Fit into the box (below the title), keep square.
    title_h = 30
    available = min(box.w - 16, box.h - title_h - 28)
    qr_img = qr_img.resize((available, available), Image.NEAREST)
    qx = box.x + (box.w - available) // 2
    qy = box.y + title_h
    img.paste(qr_img, (qx, qy))

    # SSID caption
    ssid_f = font(14, bold=True)
    ssid = settings.wifi_ssid
    sw = draw.textlength(ssid, font=ssid_f)
    draw.text((box.x + (box.w - sw) / 2, qy + available + 4), ssid, font=ssid_f, fill=0)
