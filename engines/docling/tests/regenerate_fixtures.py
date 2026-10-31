"""Regenerate the DoclingDocument JSON fixtures under ``fixtures/docling``.

The Rust mapper's snapshot tests read these files, so CI needs no engine or models. Run it with
the development environment after ``just docling-dev``::

    .tools/docling/python-env/bin/python engines/docling/tests/regenerate_fixtures.py

It converts each source fixture through the engine exactly as the host does (a cleared
environment, a fresh workspace) and refreshes the SHA-256 values in ``fixtures/manifest.toml``.
"""

from __future__ import annotations

import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

REPOSITORY = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPOSITORY / "fixtures" / "gen" / "src"))

from ariad_fixture_gen.manifest import update_generated_hashes  # noqa: E402
from conftest import clean_env  # noqa: E402

# Fixture id -> (source path, input format, request options).
SOURCES = {
    "en-map-room": ("fixtures/pdf/en-map-room.pdf", "pdf", {}),
    "pd-en-gao-08-35-highlights": ("fixtures/pdf/pd-en-gao-08-35-highlights.pdf", "pdf", {}),
    "vi-garden-300dpi": ("fixtures/scan/vi-garden-300dpi.png", "png", {"lang": ["vi"]}),
    "vi-route-table": ("fixtures/docx/vi-route-table.docx", "docx", {}),
    "vi-garden-slides": ("fixtures/pptx/vi-garden-slides.pptx", "pptx", {}),
    "en-repair-log": ("fixtures/xlsx/en-repair-log.xlsx", "xlsx", {}),
}


def convert(source: Path, input_format: str, options: dict) -> bytes:
    with tempfile.TemporaryDirectory(prefix="docling-fixture-") as directory:
        work = Path(directory)
        for name in ("in", "out", "tmp"):
            (work / name).mkdir()
        copy = work / "in" / source.name
        shutil.copyfile(source, copy)
        request = {
            "protocol": "ariad-engine/1",
            "job": f"fixture-{source.stem}",
            "op": "convert",
            "input": {"path": str(copy), "format": input_format},
            "output": {"dir": str(work / "out"), "format": "docling+json"},
            "work_dir": str(work / "tmp"),
            "options": options,
            "limits": {"max_nesting_depth": 64},
        }
        completed = subprocess.run(
            [sys.executable, "-I", "-m", "ariad_docling"],
            input=json.dumps(request).encode() + b"\n",
            capture_output=True,
            env=clean_env(),
            check=False,
        )
        result = json.loads(completed.stdout.splitlines()[-1])
        if not result.get("ok"):
            raise SystemExit(f"{source}: {result.get('error')}")
        return (work / "out" / "document.docling.json").read_bytes()


def main() -> int:
    written = set()
    for fixture_id, (source, input_format, options) in SOURCES.items():
        relative = f"fixtures/docling/{fixture_id}.docling.json"
        target = REPOSITORY / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(convert(REPOSITORY / source, input_format, options))
        written.add(relative)
        print(f"wrote {relative}")
    update_generated_hashes(REPOSITORY, written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
