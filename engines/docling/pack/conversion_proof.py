"""Real table, RapidOCR and Tesseract comparisons for the native x86_64 packs."""

from __future__ import annotations

import json
import os
import statistics
import subprocess
import tempfile
import time
from pathlib import Path

FIXTURES = (
    ("table", "pdf/en-map-room.pdf", {"ocr": "off"}),
    ("rapidocr", "scan/en-alice-p3.pdf", {"ocr": "rapidocr", "lang": ["en"]}),
    ("tesseract", "scan/vi-garden-300dpi.pdf", {"ocr": "tesseract", "lang": ["vi", "en"]}),
)
CPU_MARKER = "ASHIFT_ENGINE_CPU_SECONDS="
TIMED_ENGINE = f"""
import runpy, sys, time
start = time.process_time()
try:
    runpy.run_module('ariad_docling', run_name='__main__')
finally:
    print('{CPU_MARKER}' + str(time.process_time() - start), file=sys.stderr)
"""


def capture(python: Path, repository: Path, *, repeats: int = 3) -> dict:
    results = {}
    engine_threads = min(4, os.cpu_count() or 1)
    env = {"PATH": os.environ.get("PATH", "")}
    if os.name == "nt":
        env["SYSTEMROOT"] = os.environ.get("SYSTEMROOT", r"C:\Windows")
    env["OMP_NUM_THREADS"] = str(engine_threads)
    env["OPENBLAS_NUM_THREADS"] = str(engine_threads)
    for name, fixture, options in FIXTURES:
        samples = []
        outputs = []
        # Each sample starts a fresh engine. Discard one warmup so filesystem
        # caching does not unfairly penalize the original wheels.
        for sample in range(repeats + 1):
            output, elapsed, cpu = _convert(python, repository, name, fixture, options, env)
            if sample:
                samples.append({"elapsed_s": elapsed, "engine_cpu_s": cpu})
                outputs.append(output)
        results[name] = {
            **outputs[0],
            "repeat_output_equal": all(output == outputs[0] for output in outputs),
            "timing": {
                "samples": samples,
                "median_elapsed_s": statistics.median(s["elapsed_s"] for s in samples),
                "median_engine_cpu_s": statistics.median(s["engine_cpu_s"] for s in samples),
                "engine_threads": engine_threads,
                "cpu_scope": "engine process and its native threads; excludes OCR child processes",
            },
        }
    return results


def _convert(python, repository, name, fixture, options, env):
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
        start = time.perf_counter()
        completed = subprocess.run(
            [str(python), "-I", "-B", "-X", "faulthandler", "-c", TIMED_ENGINE],
            input=json.dumps(request).encode() + b"\n",
            capture_output=True,
            env=env,
            timeout=600,
            check=False,
        )
        elapsed = time.perf_counter() - start
        events = [json.loads(line) for line in completed.stdout.splitlines()]
        if completed.returncode or not events or not events[-1].get("ok"):
            raise SystemExit(
                f"{name} comparison conversion failed "
                f"(exit {completed.returncode}, 0x{completed.returncode & 0xFFFFFFFF:08X}): "
                f"{events[-1:]}\n"
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
        output = {
            "texts": texts,
            "tables": tables,
            "layout": [
                item.get("prov", [])
                for item in [*document.get("texts", []), *document.get("tables", [])]
            ],
        }
        cpu = next(
            float(line.removeprefix(CPU_MARKER))
            for line in completed.stderr.decode(errors="replace").splitlines()
            if line.startswith(CPU_MARKER)
        )
        return output, elapsed, cpu


def compare(before: dict, after: dict) -> dict:
    return {
        name: {
            "text_equal": before[name]["texts"] == value["texts"],
            "tables_equal": before[name]["tables"] == value["tables"],
            "layout_equal": before[name]["layout"] == value["layout"],
            "elapsed_change_percent": (
                value["timing"]["median_elapsed_s"] / before[name]["timing"]["median_elapsed_s"] - 1
            )
            * 100,
            "engine_cpu_change_percent": (
                value["timing"]["median_engine_cpu_s"]
                / before[name]["timing"]["median_engine_cpu_s"]
                - 1
            )
            * 100,
            "before": before[name],
            "after": value,
        }
        for name, value in after.items()
    }
