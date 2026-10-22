"""The ``describe`` operation: versions, Tesseract status, installed assets and routes.

It imports neither Docling nor torch, so it answers within the host's describe timeout.
"""

from __future__ import annotations

import re
import subprocess
from importlib import metadata
from pathlib import Path
from typing import Any

from ariad_docling.environment import Assets, enforces_memory_limit

INPUT_FORMATS = ("pdf", "docx", "pptx", "xlsx", "png", "jpeg", "tiff", "webp")
OUTPUT_FORMAT = "docling+json"
TESSERACT_MIN_MAJOR = 5
TOOL_TIMEOUT_S = 10

# Model directories under ``artifacts_path``, as Docling names them.
_MODELS = {
    "docling-layout-heron": "docling-project--docling-layout-heron",
    "tableformer": "docling-project--docling-models/model_artifacts/tableformer",
    "rapidocr": "RapidOcr",
}


def docling_version() -> str:
    for distribution in ("docling-slim", "docling"):
        try:
            return metadata.version(distribution)
        except metadata.PackageNotFoundError:
            continue
    return "unknown"


def tesseract_status(command: str = "tesseract") -> dict[str, Any]:
    """Finds Tesseract on PATH without exposing where it was found."""
    try:
        completed = subprocess.run(
            [command, "--version"],
            capture_output=True,
            text=True,
            timeout=TOOL_TIMEOUT_S,
            check=False,
        )
    except OSError, subprocess.SubprocessError:
        return {"name": "tesseract", "status": "missing"}
    output = f"{completed.stdout}\n{completed.stderr}"
    match = re.search(r"tesseract\s+v?(\d+)\.(\d+)(?:\.(\d+))?", output)
    if completed.returncode != 0 or match is None:
        return {"name": "tesseract", "status": "missing"}
    version = ".".join(part for part in match.groups() if part is not None)
    status = "found" if int(match.group(1)) >= TESSERACT_MIN_MAJOR else "wrong_version"
    return {"name": "tesseract", "version": version, "status": status}


def tessdata_languages(tessdata: Path | None) -> frozenset[str]:
    if tessdata is None:
        return frozenset()
    return frozenset(path.stem for path in tessdata.glob("*.traineddata") if path.is_file())


def installed_models(assets: Assets) -> list[str]:
    models: list[str] = []
    if assets.artifacts is not None:
        for name, relative in _MODELS.items():
            directory = assets.artifacts / relative
            if directory.is_dir() and any(directory.iterdir()):
                models.append(name)
    models.extend(
        f"tessdata-{language}" for language in sorted(tessdata_languages(assets.tessdata))
    )
    return models


def capabilities(assets: Assets) -> dict[str, Any]:
    return {
        "type": "capabilities",
        "engine": "docling",
        "version": docling_version(),
        "tool": tesseract_status(),
        "license": "MIT",
        "routes": [{"input": source, "output": OUTPUT_FORMAT} for source in INPUT_FORMATS],
        "enforces_memory_limit": enforces_memory_limit(),
        "models": installed_models(assets),
        "supports_session": True,
    }
