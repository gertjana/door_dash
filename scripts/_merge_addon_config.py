#!/usr/bin/env python3
"""Deep-merge two YAML or JSON files.

Used by ``scripts/build_addon_tarball.sh`` to apply ``dev/`` overrides
on top of the canonical ``addon/`` config when packing the dev tarball.

Merge rules:
  * dict + dict  -> recursively merged (override keys win, missing keys
                    inherited from base)
  * everything else (lists, scalars) -> override replaces base wholesale

Format is inferred from the file extension (``.yaml`` / ``.yml`` /
``.json``). The output file is written in the same format as the base
file, so JSON in -> JSON out and YAML in -> YAML out, regardless of
the override's format.

Usage:
    _merge_addon_config.py <base> <override> <output>

Exits non-zero on argument or parse errors so the calling shell script
can ``set -e`` its way to a clean failure.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any


def _is_yaml(path: Path) -> bool:
    return path.suffix.lower() in (".yaml", ".yml")


def _load(path: Path) -> Any:
    text = path.read_text(encoding="utf-8")
    if _is_yaml(path):
        import yaml  # local import: JSON-only paths don't need PyYAML

        return yaml.safe_load(text)
    if path.suffix.lower() == ".json":
        return json.loads(text)
    raise SystemExit(f"unsupported file extension: {path.suffix} ({path})")


def _dump(path: Path, data: Any) -> None:
    if _is_yaml(path):
        import yaml

        path.write_text(yaml.safe_dump(data, sort_keys=False), encoding="utf-8")
        return
    if path.suffix.lower() == ".json":
        path.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")
        return
    raise SystemExit(f"unsupported output extension: {path.suffix} ({path})")


def merge(base: Any, override: Any) -> Any:
    """Deep-merge ``override`` onto ``base`` (override wins).

    Dicts are merged key-by-key; anything else is replaced.
    """
    if isinstance(base, dict) and isinstance(override, dict):
        out = dict(base)
        for k, v in override.items():
            out[k] = merge(base[k], v) if k in base else v
        return out
    return override


def main(argv: list[str]) -> int:
    if len(argv) != 4:
        print(
            "usage: _merge_addon_config.py <base> <override> <output>",
            file=sys.stderr,
        )
        return 2

    base_path = Path(argv[1])
    override_path = Path(argv[2])
    out_path = Path(argv[3])

    if not base_path.is_file():
        print(f"base file not found: {base_path}", file=sys.stderr)
        return 1
    if not override_path.is_file():
        print(f"override file not found: {override_path}", file=sys.stderr)
        return 1

    base = _load(base_path)
    override = _load(override_path)
    merged = merge(base, override)

    out_path.parent.mkdir(parents=True, exist_ok=True)
    _dump(out_path, merged)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
