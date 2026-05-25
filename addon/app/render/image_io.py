"""Image helpers: convert composed Pillow image into the wire formats."""

from __future__ import annotations

import io

from PIL import Image


def to_mono(img: Image.Image) -> Image.Image:
    """Force a 1-bit monochrome image.

    Uses a hard threshold (no dithering) so anti-aliased text stays crisp.
    The composed canvas is L-mode, so we just round grey pixels to black or
    white at the midpoint. ePaper has no greyscale anyway, and dithering on
    text edges produces noisy speckles.
    """
    if img.mode == "1":
        return img
    if img.mode != "L":
        img = img.convert("L")
    # Threshold at 128: pixels darker than mid-grey become black, else white.
    return img.point(lambda p: 0 if p < 128 else 255, mode="1")


def to_bmp_bytes(img: Image.Image) -> bytes:
    """Encode for ESPHome `online_image` with `type: BINARY`.

    ESPHome treats bit=1 as "draw pixel" (black on ePaper) and bit=0 as
    "background" (white). A standard Pillow 1-bit BMP stores black=0 /
    white=1, which is the opposite — so we get an inverted image on the
    panel (black background, white content) unless we invert here.

    We only invert the BMP wire format; the PNG preview stays in normal
    polarity so the browser shows what the panel will actually render.
    """
    mono = to_mono(img)
    # ImageChops.invert doesn't work on 1-bit mode, so flip via point().
    inverted = mono.point(lambda p: 0 if p else 255, mode="1")
    buf = io.BytesIO()
    inverted.save(buf, format="BMP")
    return buf.getvalue()


def to_png_bytes(img: Image.Image) -> bytes:
    buf = io.BytesIO()
    to_mono(img).save(buf, format="PNG")
    return buf.getvalue()
