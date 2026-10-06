#!/usr/bin/env python3
"""E6: Benchmark comparing separate processes per conversion vs single process session mode.

Converts 3 documents in two modes:
1. Isolated: separate process spawned for each document (current runner architecture).
2. Session: single warm process converting requests in a loop.

Outputs timing results and writes a CSV report to reports/docling-spike/e6_session_timings.csv.
"""

from __future__ import annotations

import csv
import json
import os
import subprocess
import sys
import time
from pathlib import Path


def run_single_process(python_bin: Path, engine_script: Path, req: dict) -> tuple[float, float]:
    """Runs a single request in an isolated process. Returns (total_wall_s, engine_elapsed_s)."""
    t0 = time.monotonic()
    p = subprocess.run(
        [str(python_bin), str(engine_script)],
        input=json.dumps(req) + "\n",
        capture_output=True,
        text=True,
    )
    total_wall = time.monotonic() - t0
    engine_elapsed = 0.0
    for line in p.stdout.splitlines():
        try:
            ev = json.loads(line)
            if ev.get("type") == "result" and ev.get("ok"):
                engine_elapsed = ev.get("metrics", {}).get("elapsed_ms", 0) / 1000.0
        except Exception:
            pass
    return total_wall, engine_elapsed


def main():
    spike_worktree = Path("/home/vchun/Codes/02-My-Projects/AriadShift-spike-docling")
    python_bin = spike_worktree / "spike/docling/.venv/bin/python"
    engine_script = spike_worktree / "spike/docling/engine.py"
    models_dir = spike_worktree / ".tools/docling-models"
    tessdata_dir = spike_worktree / ".tools/tessdata"

    fixtures = [
        ("gao-08-35", spike_worktree / "fixtures/pdf/pd-en-gao-08-35-highlights.pdf", "pdf", False),
        ("vn-congbao", spike_worktree / "fixtures/scan/vn-congbao-42-2020-p31.png", "png", True),
        ("arxiv-tableformer", spike_worktree / "fixtures/pdf/arxiv-tableformer-2203-01017-v1.pdf", "pdf", False),
    ]

    out_base = Path("/tmp/e6_bench_out")
    out_base.mkdir(parents=True, exist_ok=True)

    print("=== Mode 1: Isolated Process per Document ===")
    isolated_times = []
    for name, path, fmt, do_ocr in fixtures:
        req = {
            "protocol": "ariad-engine/1",
            "job": f"e6-iso-{name}",
            "op": "convert",
            "input": {"path": str(path), "format": fmt},
            "output": {"dir": str(out_base / f"iso_{name}"), "format": "docling+json"},
            "work_dir": str(out_base / f"tmp_iso_{name}"),
            "options": {
                "artifacts_path": str(models_dir),
                "tessdata_path": str(tessdata_dir),
                "do_ocr": do_ocr,
                "ocr_mode": "full_page" if do_ocr else "auto",
            },
            "limits": {"max_pages": None, "timeout_s": None, "max_memory_mb": None, "max_nesting_depth": 64},
        }
        print(f"Running isolated {name}...")
        wall_s, eng_s = run_single_process(python_bin, engine_script, req)
        print(f"  {name}: wall={wall_s:.2f}s, engine={eng_s:.2f}s")
        isolated_times.append((name, wall_s, eng_s))

    total_isolated_wall = sum(t[1] for t in isolated_times)
    print(f"Total Isolated Wall Time: {total_isolated_wall:.2f}s")

    print("\n=== Mode 2: In-Process Session Mode (Loop) ===")
    session_script = f"""
import sys, time, json
from pathlib import Path
from docling.document_converter import DocumentConverter, PdfFormatOption, ImageFormatOption
from docling.datamodel.pipeline_options import PdfPipelineOptions, AcceleratorOptions, TesseractCliOcrOptions, OcrMode
from docling.datamodel.base_models import InputFormat
import os

t0_init = time.monotonic()
models_dir = Path('{models_dir}')
tessdata_dir = Path('{tessdata_dir}')

os.environ["TESSDATA_PREFIX"] = str(tessdata_dir)
os.environ["HF_HUB_OFFLINE"] = "1"
os.environ["TRANSFORMERS_OFFLINE"] = "1"

ocr_opts = TesseractCliOcrOptions(
    tesseract_cmd="tesseract",
    path=str(tessdata_dir),
    lang=["vie", "eng"],
    mode=OcrMode.FULL_PAGE,
)

pdf_opts = PdfPipelineOptions(
    artifacts_path=models_dir,
    do_ocr=False,
    do_table_structure=True,
    accelerator_options=AcceleratorOptions(num_threads=4),
)

img_opts = PdfPipelineOptions(
    artifacts_path=models_dir,
    do_ocr=True,
    ocr_options=ocr_opts,
    do_table_structure=True,
    accelerator_options=AcceleratorOptions(num_threads=4),
)

converter = DocumentConverter(
    format_options={{
        InputFormat.PDF: PdfFormatOption(pipeline_options=pdf_opts),
        InputFormat.IMAGE: ImageFormatOption(pipeline_options=img_opts),
    }}
)
init_time = time.monotonic() - t0_init
print(json.dumps({{"type": "init", "time_s": init_time}}))

fixtures = {json.dumps([(name, str(path)) for name, path, _, _ in fixtures])}
for name, p in fixtures:
    t0 = time.monotonic()
    conv_res = converter.convert(p)
    elapsed = time.monotonic() - t0
    pages = len(conv_res.pages) if conv_res.pages else 1
    print(json.dumps({{"type": "doc", "name": name, "pages": pages, "time_s": elapsed}}))
"""
    p_sess = subprocess.run(
        [str(python_bin), "-c", session_script],
        capture_output=True,
        text=True,
    )

    session_init_time = 0.0
    session_doc_times = []
    for line in p_sess.stdout.splitlines():
        try:
            ev = json.loads(line)
            if ev.get("type") == "init":
                session_init_time = ev.get("time_s", 0.0)
            elif ev.get("type") == "doc":
                session_doc_times.append((ev.get("name"), ev.get("pages"), ev.get("time_s", 0.0)))
        except Exception:
            pass

    print(f"Session Model Init Time: {session_init_time:.2f}s")
    for name, pages, t_s in session_doc_times:
        print(f"  {name} ({pages} pages): {t_s:.2f}s")

    total_session_wall = session_init_time + sum(t[2] for t in session_doc_times)
    print(f"Total Session Time (including init): {total_session_wall:.2f}s")
    print(f"Speedup from Session Mode: {total_isolated_wall / total_session_wall:.2f}x")

    # Write CSV
    csv_path = Path("/home/vchun/Codes/02-My-Projects/AriadShift/plans/261006-0544-docling-spike-and-roadmap-1a/reports/docling-spike/e6_session_timings.csv")
    with open(csv_path, "w", newline="", encoding="utf-8") as f:
        writer = csv.writer(f)
        writer.writerow(["mode", "document", "pages", "init_time_s", "convert_time_s", "total_wall_s"])
        for (name, wall_s, eng_s) in isolated_times:
            writer.writerow(["isolated", name, 1 if "gao" in name or "congbao" in name else 10, f"{wall_s - eng_s:.2f}", f"{eng_s:.2f}", f"{wall_s:.2f}"])
        writer.writerow(["isolated_summary", "all_3_docs", 12, "-", f"{sum(t[2] for t in isolated_times):.2f}", f"{total_isolated_wall:.2f}"])
        for (name, pages, t_s) in session_doc_times:
            writer.writerow(["session", name, pages, "-", f"{t_s:.2f}", f"{t_s:.2f}"])
        writer.writerow(["session_init", "startup_warmup", 0, f"{session_init_time:.2f}", "-", f"{session_init_time:.2f}"])
        writer.writerow(["session_summary", "all_3_docs_with_init", 12, f"{session_init_time:.2f}", f"{sum(t[2] for t in session_doc_times):.2f}", f"{total_session_wall:.2f}"])

    print(f"\nWritten timing results to {csv_path}")


if __name__ == "__main__":
    main()
