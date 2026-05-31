"""Page registry — auto-discovers page modules in this directory.

A "page" is a module exposing two attributes:

    TITLE: str                 — human-readable page name
    render(settings, sensors, fw_version=None) -> PIL.Image.Image

Pages are discovered by scanning this directory for ``*.py`` files (other
than ``__init__.py``) and imported in **alphabetical filename order**. Use a
numeric prefix (``00_``, ``01_``, …) on the filename to control ordering;
the prefix is stripped from the canonical page name used in URLs.

Examples of canonical names:

    00_dashboard.py   -> name "dashboard", index 0
    01_week.py        -> name "week",      index 1
    02_placeholder.py -> name "placeholder", index 2

The first page (index 0) is the default and is what gets served when no
``?page=`` query parameter is supplied. The firmware persists the current
page index in RTC memory and asks for ``?page=N``; on the server side we
also accept the canonical name for human-friendly URLs in the preview UI.
"""

from __future__ import annotations

import importlib
import logging
import re
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Protocol

if TYPE_CHECKING:
    from PIL import Image

    from ...config import Settings
    from ...sources.local_sensors import LocalSensors

log = logging.getLogger(__name__)

# Strip a leading ``NN_`` numeric prefix from filenames so the canonical
# URL name is just ``dashboard`` rather than ``00_dashboard``.
_PREFIX_RE = re.compile(r"^\d+[_-]")


class _PageModule(Protocol):
    TITLE: str

    def render(
        self,
        settings: Settings,
        sensors: LocalSensors,
        fw_version: str | None = ...,
    ) -> Image.Image: ...


@dataclass(frozen=True)
class Page:
    """A registered page, ready to render."""

    index: int
    name: str  # canonical URL-safe name, prefix-stripped
    title: str  # human-readable title from the module's TITLE constant
    module: _PageModule

    def render(
        self,
        settings: Settings,
        sensors: LocalSensors,
        fw_version: str | None = None,
    ) -> Image.Image:
        return self.module.render(settings, sensors, fw_version=fw_version)


def _discover() -> list[Page]:
    """Import every page module in this package, in alphabetical order."""
    here = Path(__file__).parent
    pages: list[Page] = []
    for path in sorted(here.glob("*.py")):
        if path.name == "__init__.py":
            continue
        stem = path.stem
        canonical = _PREFIX_RE.sub("", stem)
        modname = f"{__name__}.{stem}"
        try:
            module = importlib.import_module(modname)
        except Exception:
            log.exception("Failed to import page module %s; skipping", modname)
            continue
        if not hasattr(module, "render") or not callable(module.render):
            log.warning("Page module %s has no callable render(); skipping", modname)
            continue
        title = getattr(module, "TITLE", canonical.replace("_", " ").title())
        pages.append(Page(index=len(pages), name=canonical, title=title, module=module))
    if not pages:
        log.error(
            "No page modules discovered in %s — the dashboard will fail to render",
            here,
        )
    return pages


# Computed once at import time. Re-import the package (or restart the
# process) to pick up new page files; we don't watch the filesystem.
PAGES: list[Page] = _discover()


def get_page(key: str | int | None) -> Page:
    """Resolve a page by index (int or numeric string) or canonical name.

    ``None`` / empty string / out-of-range index all fall back to index 0
    so the device can never end up with a "not found" page after we add
    or remove a page without re-flashing the firmware.
    """
    if not PAGES:
        raise RuntimeError("No pages registered")

    if key is None or key == "":
        return PAGES[0]

    # Accept numeric index (firmware passes ?page=0, ?page=1, ...).
    # Wrap with modulo so the firmware can blindly increment past the end.
    if isinstance(key, int):
        return PAGES[key % len(PAGES)]
    if isinstance(key, str) and key.lstrip("-").isdigit():
        return PAGES[int(key) % len(PAGES)]

    # Otherwise look up by canonical name (preview UI uses this).
    for p in PAGES:
        if p.name == key:
            return p

    log.warning("Unknown page %r; falling back to %r", key, PAGES[0].name)
    return PAGES[0]


__all__ = ["PAGES", "Page", "get_page"]
