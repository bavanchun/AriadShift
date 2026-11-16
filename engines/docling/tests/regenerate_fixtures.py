"""Regenerate the DoclingDocument JSON fixtures under ``fixtures/docling``.

The Rust mapper's snapshot tests read these files, so CI needs no engine or models. Run it with
the development environment after ``just docling-dev``::

    .tools/docling/python-env/bin/python engines/docling/tests/regenerate_fixtures.py

It builds the source-less formula fixture directly and converts each source fixture through the
engine exactly as the host does (a cleared environment, a fresh workspace) and refreshes the
SHA-256 values in ``fixtures/manifest.toml``.
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


FORMULA_CELLS_ID = "en-formula-cells"

# Cell rows of the formula-injection fixture: a table of spreadsheet-formula lookalikes and a
# plain header-only table. There is no source document, so the file is built directly.
FORMULA_CELLS_TABLES = (
    (
        (False, ("=SUM(A1:A5)", "+12345")),
        (False, ("-999", "@formula")),
    ),
    ((True, ("Item", "Count")),),
)


def formula_cells_document() -> bytes:
    """Build the DoclingDocument JSON for the formula-injection fixture deterministically."""
    tables = []
    for index, rows in enumerate(FORMULA_CELLS_TABLES):
        cells = [
            {
                "row_span": 1,
                "col_span": 1,
                "start_row_offset_idx": row,
                "end_row_offset_idx": row + 1,
                "start_col_offset_idx": column,
                "end_col_offset_idx": column + 1,
                "text": text,
                "column_header": header,
                "row_header": False,
                "row_section": False,
                "fillable": False,
            }
            for row, (header, texts) in enumerate(rows)
            for column, text in enumerate(texts)
        ]
        tables.append(
            {
                "self_ref": f"#/tables/{index}",
                "parent": {"$ref": "#/body"},
                "children": [],
                "content_layer": "body",
                "label": "table",
                "prov": [],
                "captions": [],
                "references": [],
                "footnotes": [],
                "data": {"table_cells": cells},
            }
        )
    document = {
        "schema_name": "DoclingDocument",
        "version": "1.10.0",
        "name": FORMULA_CELLS_ID,
        "origin": {
            "mimetype": "application/pdf",
            "binary_hash": 1234567890123456789,
            "filename": f"{FORMULA_CELLS_ID}.pdf",
        },
        "furniture": {
            "self_ref": "#/furniture",
            "children": [],
            "content_layer": "furniture",
            "name": "_root_",
            "label": "unspecified",
        },
        "body": {
            "self_ref": "#/body",
            "children": [{"$ref": f"#/tables/{index}"} for index in range(len(tables))],
            "content_layer": "body",
            "name": "_root_",
            "label": "unspecified",
        },
        "groups": [],
        "texts": [],
        "pictures": [],
        "tables": tables,
    }
    return (json.dumps(document, indent=2, ensure_ascii=False) + "\n").encode()


def main() -> int:
    written = set()
    for fixture_id, (source, input_format, options) in SOURCES.items():
        relative = f"fixtures/docling/{fixture_id}.docling.json"
        target = REPOSITORY / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(convert(REPOSITORY / source, input_format, options))
        written.add(relative)
        print(f"wrote {relative}")
    formula_relative = f"fixtures/docling/{FORMULA_CELLS_ID}.docling.json"
    (REPOSITORY / formula_relative).write_bytes(formula_cells_document())
    written.add(formula_relative)
    print(f"wrote {formula_relative}")
    update_generated_hashes(REPOSITORY, written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
