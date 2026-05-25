"""Widget contract.

Every widget renders into a target box (x, y, w, h) on the parent image.
Widgets must NOT draw outside their box. They receive Settings so they can
read user options, and a `data` payload from their corresponding source.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Protocol

from PIL import Image


@dataclass
class Box:
    x: int
    y: int
    w: int
    h: int

    @property
    def x2(self) -> int:
        return self.x + self.w

    @property
    def y2(self) -> int:
        return self.y + self.h


class Widget(Protocol):
    def render(self, img: Image.Image, box: Box) -> None: ...
