"""Tiny Home Assistant REST client.

Inside an HA add-on we get a `SUPERVISOR_TOKEN` env var and can reach the
Core API at `http://supervisor/core/api/...`. Outside HA (local dev) the user
can set EPDASH_HA_BASE_URL and EPDASH_SUPERVISOR_TOKEN to a real instance.
"""

from __future__ import annotations

from typing import Any

import httpx

from .config import Settings


class HAClient:
    def __init__(self, settings: Settings):
        self.settings = settings
        self.base = settings.ha_base_url.rstrip("/")
        self.token = settings.supervisor_token

    @property
    def available(self) -> bool:
        return bool(self.token)

    def _headers(self) -> dict:
        return {
            "Authorization": f"Bearer {self.token}",
            "Content-Type": "application/json",
        }

    def get_state(self, entity_id: str) -> dict | None:
        if not self.available:
            return None
        url = f"{self.base}/api/states/{entity_id}"
        try:
            r = httpx.get(url, headers=self._headers(), timeout=8.0)
            if r.status_code == 200:
                return r.json()
        except httpx.HTTPError:
            return None
        return None

    def get_calendar(self, entity_id: str, start_iso: str, end_iso: str) -> list[dict[str, Any]]:
        if not self.available:
            return []
        url = f"{self.base}/api/calendars/{entity_id}"
        try:
            r = httpx.get(
                url,
                headers=self._headers(),
                params={"start": start_iso, "end": end_iso},
                timeout=8.0,
            )
            if r.status_code == 200:
                return r.json() or []
        except httpx.HTTPError:
            return []
        return []

    def call_service(
        self,
        domain: str,
        service: str,
        data: dict | None = None,
        return_response: bool = False,
    ) -> dict | None:
        """Call an HA service. With return_response=True, returns the service
        response payload (HA 2024.x+ supports this for `weather.get_forecasts`,
        `calendar.get_events`, etc.).
        """
        if not self.available:
            return None
        url = f"{self.base}/api/services/{domain}/{service}"
        if return_response:
            url += "?return_response"
        try:
            r = httpx.post(
                url,
                headers=self._headers(),
                json=data or {},
                timeout=10.0,
            )
            if r.status_code in (200, 201):
                return r.json() if return_response else {}
        except httpx.HTTPError:
            return None
        return None
