"""Resolve trusted pack defaults without changing the run_tool wire schema."""

from __future__ import annotations

import platform
import sys
import tomllib
from pathlib import Path
from typing import Any

from ariad_ai.protocol import EngineError


def pack_options(overrides: dict[str, Any], module_path: Path | None = None) -> dict[str, Any]:
    module_path = module_path or Path(__file__)
    for root in module_path.resolve().parents:
        manifest = root / "pack.toml"
        if not manifest.is_file():
            continue
        if manifest.stat().st_size > 1 << 20:
            raise EngineError("limit_exceeded", "pack manifest exceeds 1 MiB")
        obj = tomllib.loads(manifest.read_text(encoding="utf-8"))
        if obj.get("name") != "ai" or obj.get("engine_protocol") != "ariad-engine/2":
            raise EngineError("invalid_request", "pack manifest does not describe this engine")
        arch = "aarch64" if platform.machine().lower() in {"aarch64", "arm64"} else "x86_64"
        suffix = (
            "apple-darwin"
            if sys.platform == "darwin"
            else ("pc-windows-msvc" if sys.platform == "win32" else "unknown-linux-gnu")
        )
        selected = next(
            (entry for entry in obj.get("targets", []) if entry["triple"] == f"{arch}-{suffix}"),
            None,
        )
        if selected is None:
            raise EngineError("invalid_request", "pack manifest lacks this platform")
        defaults = selected.get("options", {})
        if not isinstance(defaults, dict):
            raise EngineError("invalid_request", "pack options must be an object")
        return {
            **{
                key: value.replace("{root}", str(root)) if isinstance(value, str) else value
                for key, value in defaults.items()
            },
            **overrides,
        }
    return dict(overrides)
