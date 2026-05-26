"""ePaper Dashboard add-on package.

Exposes `__version__` read from the add-on `config.yaml` so the renderer and
HTTP layer can stamp the dashboard image with the running addon version.
"""

from __future__ import annotations

import re
from pathlib import Path

_FALLBACK_VERSION = "0.0.0"


def _read_addon_version() -> str:
    """Read the `version:` field from the add-on's config.yaml.

    Searches a couple of candidate paths so this works both inside the
    add-on container (where config.yaml lives next to the app at
    /usr/src/app/config.yaml or /addon/config.yaml depending on how it's
    packaged) and during local dev (repo layout: addon/config.yaml).
    """

    here = Path(__file__).resolve()
    candidates = [
        here.parent.parent / "config.yaml",  # repo layout: addon/config.yaml
        Path("/addon/config.yaml"),  # in case mounted there
        Path("/data/config.yaml"),
    ]
    for path in candidates:
        try:
            text = path.read_text(encoding="utf-8")
        except OSError:
            continue
        # Minimal parser: avoid pulling in PyYAML just for one line.
        match = re.search(r'^version:\s*"?([^"\s]+)"?\s*$', text, re.MULTILINE)
        if match:
            return match.group(1)
    return _FALLBACK_VERSION


__version__: str = _read_addon_version()
