"""Real table, RapidOCR and Tesseract comparisons for the native x86_64 packs."""

from __future__ import annotations

import json
import os
import subprocess
import tempfile
from pathlib import Path

FIXTURES = (
    ("table", "pdf/en-map-room.pdf", {"ocr": "off"}),
    ("rapidocr", "scan/en-alice-p3.pdf", {"ocr": "rapidocr", "lang": ["en"]}),
    ("tesseract", "scan/vi-garden-300dpi.pdf", {"ocr": "tesseract", "lang": ["vi", "en"]}),
)


def capture(python: Path, repository: Path) -> dict:
    results = {}
    env = {"PATH": os.environ.get("PATH", "")}
    if os.name == "nt":
        env["SYSTEMROOT"] = os.environ.get("SYSTEMROOT", r"C:\Windows")
    for name, fixture, options in FIXTURES:
        with tempfile.TemporaryDirectory(prefix="docling-comparison-") as directory:
            root = Path(directory)
            (root / "out").mkdir()
            (root / "tmp").mkdir()
            request = {
                "protocol": "ariad-engine/1",
                "op": "convert",
                "job": name,
                "input": {"path": str(repository / "fixtures" / fixture), "format": "pdf"},
                "output": {"dir": str(root / "out"), "format": "docling+json"},
                "work_dir": str(root / "tmp"),
                "options": options,
                "limits": {"max_nesting_depth": 64},
            }
            print(f"Converting {fixture} ({name}) for the pack comparison", flush=True)
            completed = subprocess.run(
                [str(python), "-I", "-B", "-m", "ariad_docling"],
                input=json.dumps(request).encode() + b"\n",
                capture_output=True,
                env=env,
                timeout=600,
                check=False,
            )
            events = [json.loads(line) for line in completed.stdout.splitlines()]
            if completed.returncode or not events or not events[-1].get("ok"):
                raise SystemExit(
                    f"{name} comparison conversion failed: {events[-1:]}\n"
                    f"{completed.stderr.decode(errors='replace')[-8000:]}"
                )
            artifact = next(event for event in events if event["type"] == "artifact")
            document = json.loads(Path(artifact["path"]).read_text("utf-8"))
            texts = [item["text"] for item in document.get("texts", [])]
            tables = [item["data"] for item in document.get("tables", [])]
            if not texts or not any(text.strip() for text in texts):
                raise SystemExit(f"{name} comparison conversion produced no text")
            if name == "table" and not tables:
                raise SystemExit("TableFormer comparison produced no table")
            if name == "tesseract" and "Khu vườn đọc sách" not in "\n".join(texts):
                raise SystemExit("Tesseract comparison lost the Vietnamese fixture heading")
            results[name] = {
                "texts": texts,
                "tables": tables,
                "layout": [
                    item.get("prov", [])
                    for item in [*document.get("texts", []), *document.get("tables", [])]
                ],
            }
    return results


def compare(before: dict, after: dict) -> dict:
    return {
        name: {
            "text_equal": before[name]["texts"] == value["texts"],
            "tables_equal": before[name]["tables"] == value["tables"],
            "layout_equal": before[name]["layout"] == value["layout"],
            "before": before[name],
            "after": value,
        }
        for name, value in after.items()
    }
