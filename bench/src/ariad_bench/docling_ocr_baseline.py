"""Generate a Docling OCR transcript for comparison with an authored scan truth."""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--models", type=Path, required=True)
    parser.add_argument("--tessdata", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--language", action="append", required=True)
    args = parser.parse_args()
    from docling_core.types.doc import DoclingDocument

    with tempfile.TemporaryDirectory(prefix="ariad-docling-baseline-") as temporary:
        root = Path(temporary)
        for name in ("in", "out", "tmp"):
            (root / name).mkdir()
        source = root / "in" / args.input.name
        shutil.copyfile(args.input, source)
        suffix = source.suffix.lower().lstrip(".")
        formats = {"pdf": "pdf", "png": "png", "jpg": "jpeg", "jpeg": "jpeg"}
        if suffix not in formats:
            raise ValueError(f"unsupported fixture format: {suffix}")
        request = {
            "protocol": "ariad-engine/1", "job": "ocr-baseline", "op": "convert",
            "input": {"path": str(source), "format": formats[suffix]},
            "output": {"dir": str(root / "out"), "format": "docling+json"},
            "work_dir": str(root / "tmp"),
            "options": {"ocr": "tesseract", "lang": args.language,
                        "artifacts_path": str(args.models.resolve()), "tessdata_path": str(args.tessdata.resolve())},
            "limits": {"timeout_s": 300},
        }
        result = subprocess.run([sys.executable, "-I", "-m", "ariad_docling"], input=json.dumps(request) + "\n", text=True, capture_output=True, timeout=300, check=True)
        events = [json.loads(line) for line in result.stdout.splitlines()]
        if not events or events[-1].get("type") != "result" or not events[-1].get("ok"):
            raise RuntimeError(f"Docling conversion failed: {events}")
        artifacts = [event for event in events if event.get("type") == "artifact"]
        if len(artifacts) != 1:
            raise RuntimeError("Docling did not return one artifact")
        artifact = Path(artifacts[0]["path"]).resolve()
        if not artifact.is_relative_to((root / "out").resolve()) or not artifact.is_file():
            raise RuntimeError("Docling returned an unsafe artifact")
        document = DoclingDocument.model_validate_json(artifact.read_bytes())
        args.output.write_text(document.export_to_text(traverse_pictures=True), encoding="utf-8")


if __name__ == "__main__":
    main()
