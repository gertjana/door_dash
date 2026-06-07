"""Resolve the effective timezone for the dashboard.

Precedence:

1. **Home Assistant** — if the supervisor token is present and
   ``/api/config`` returns a valid ``time_zone`` field, that wins.
   This makes the addon track the user's HA setting automatically;
   on a typical HA install it's always the right answer.
2. **Addon option** — ``settings.timezone`` (default
   ``Europe/Amsterdam``). Used when HA is unreachable, or when the
   addon is being run outside HA for development.
3. **UTC** — last-resort default if neither of the above yields a
   parseable zone name.

A module-level cache with a 60 s TTL keeps things cheap when multiple
pages + widgets all resolve timezone in the same render pass (common —
the dashboard, calendar, weather, and energy pages all need it). The
TTL is shorter than the typical refresh interval so an actual HA
timezone change still propagates within roughly one render cycle.

The cache is process-local; each addon container restart drops it.
That's fine: container restarts are rare and the timezone is unlikely
to change between them.
"""

from __future__ import annotations

import logging
import time
from typing import TYPE_CHECKING
from zoneinfo import ZoneInfo, ZoneInfoNotFoundError

from .ha_client import HAClient

if TYPE_CHECKING:
    from .config import Settings

log = logging.getLogger(__name__)

# 60 s is enough to dedupe within a render cycle (which takes <30 s in
# practice) without holding stale state long enough to matter to a user
# who just tweaked their HA timezone.
_CACHE_TTL_S = 60.0

# (monotonic_expiry, resolved_tz). ``None`` means uninitialised.
_cache: tuple[float, ZoneInfo] | None = None


def resolve_timezone(settings: Settings) -> ZoneInfo:
    """Return the effective ZoneInfo for rendering.

    Cached for ``_CACHE_TTL_S`` seconds across calls; subsequent calls
    within the TTL return the same object without re-hitting HA.
    """
    global _cache
    now = time.monotonic()
    if _cache is not None and _cache[0] > now:
        return _cache[1]

    tz = _try_ha(settings) or _try_settings(settings) or ZoneInfo("UTC")
    _cache = (now + _CACHE_TTL_S, tz)
    return tz


def _try_ha(settings: Settings) -> ZoneInfo | None:
    """Look up timezone from HA's ``/api/config``. Returns ``None`` on any
    failure — caller falls through to the next source."""
    ha = HAClient(settings)
    if not ha.available:
        return None
    cfg = ha.get_config()
    if not cfg:
        return None
    # HA exposes the field as ``time_zone`` (snake_case with underscore);
    # accept ``timezone`` as an alias too in case a future version renames.
    name = cfg.get("time_zone") or cfg.get("timezone")
    if not name:
        return None
    try:
        return ZoneInfo(str(name))
    except (ZoneInfoNotFoundError, ValueError):
        log.warning(
            "HA reported timezone %r which is not a known IANA zone — falling back",
            name,
        )
        return None


def _try_settings(settings: Settings) -> ZoneInfo | None:
    """Use the addon option ``timezone`` as a fallback. Returns ``None`` if
    unset or invalid."""
    name = (settings.timezone or "").strip()
    if not name:
        return None
    try:
        return ZoneInfo(name)
    except (ZoneInfoNotFoundError, ValueError):
        log.warning(
            "addon option timezone %r is not a valid IANA zone — falling back to UTC",
            name,
        )
        return None


def _clear_cache() -> None:
    """Reset the module-level cache. Test-only escape hatch — production
    code should never need this; the TTL handles real refreshes."""
    global _cache
    _cache = None


__all__ = ["resolve_timezone"]
