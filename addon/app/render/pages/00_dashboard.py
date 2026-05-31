"""Page 0 — the main dashboard.

Thin wrapper around the original ``render.layout.compose``. Kept as a
separate module so we can iterate on the multi-page architecture without
touching the working layout code.

The ``NN_`` numeric prefix is intentional — it drives the page order in
the registry; see ``addon/app/render/pages/__init__.py``.
"""  # noqa: N999

from __future__ import annotations

from typing import TYPE_CHECKING

from ..layout import compose

if TYPE_CHECKING:
    from PIL import Image

    from ...config import Settings
    from ...sources.local_sensors import LocalSensors

TITLE = "Dashboard"


def render(
    settings: Settings,
    sensors: LocalSensors,
    fw_version: str | None = None,
) -> Image.Image:
    return compose(settings, sensors, fw_version=fw_version)
