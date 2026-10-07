"""Reference resolution for benchmark fixtures.

Per Principle 5 ("Evidence over claims"):
- Pandoc reader edges are never scored against Pandoc's own reading.
- Generated fixtures use authored truth companions written by fixtures/gen.
- Markdown fixtures use Pandoc's GFM reader as an independent implementation.
- Unscored fixtures (e.g. public government DOCX files or edge cases exceeding canonical form) return None.
"""

from __future__ import annotations

import json
from pathlib import Path
import subprocess
from typing import Any

from ariad_bench.canonical import CanonicalDoc, normalize


class ReferenceResolutionError(RuntimeError):
    """Raised when an expected reference cannot be resolved or computed."""


def get_reference_for_fixture(
    fixture: dict[str, Any],
    root: Path,
    pandoc_bin: Path | str = "pandoc",
) -> CanonicalDoc | None:
    """Resolve the independent canonical reference for a fixture.

    Returns:
        CanonicalDoc if a valid independent reference exists, or None if the fixture
        is unscored (e.g. external document without an authored companion).
    """
    fmt = fixture.get("format", "").lower()
    fixture_path = root / fixture.get("path", "")

    # 1. Authored truth companions for generated fixtures (DOCX, HTML, EPUB)
    companions = fixture.get("companions", [])
    for comp in companions:
        comp_path = root / comp.get("path", "")
        if comp_path.name.endswith(".truth.json") and comp_path.is_file():
            data = json.loads(comp_path.read_text(encoding="utf-8"))
            if data.get("unscored") is True:
                return None
            return normalize(data)

    # Also check convention-based path if companions metadata not yet loaded
    fixture_id = fixture.get("id", "")
    direct_companion = fixture_path.parent / f"{fixture_id}.truth.json"
    if direct_companion.is_file():
        data = json.loads(direct_companion.read_text(encoding="utf-8"))
        if data.get("unscored") is True:
            return None
        return normalize(data)

    # 2. Markdown fixtures: read by Pandoc GFM reader (independent from our comrak reader)
    if fmt in ("md", "markdown"):
        if not fixture_path.is_file():
            raise ReferenceResolutionError(f"Markdown fixture does not exist: {fixture_path}")

        try:
            proc = subprocess.run(
                [str(pandoc_bin), "-f", "gfm", "-t", "json", str(fixture_path)],
                capture_output=True,
                text=True,
                encoding="utf-8",
                errors="replace",
                timeout=30,
                check=False,
            )
        except subprocess.TimeoutExpired as exc:
            raise ReferenceResolutionError(
                f"Pandoc GFM reader timed out after 30s on {fixture_path}"
            ) from exc

        if proc.returncode != 0:
            raise ReferenceResolutionError(
                f"Pandoc GFM reader failed on {fixture_path}: {proc.stderr.strip()}"
            )
        pandoc_ast = json.loads(proc.stdout)
        return normalize(pandoc_ast)

    # 3. External fixtures without companions are unscored
    return None
