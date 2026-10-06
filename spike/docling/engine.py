#!/usr/bin/env python3
"""Docling engine adapter for AriadShift engine protocol ariad-engine/1.

Speaks JSON Lines over stdin/stdout.
Immediately moves fd 1 to stderr (fd 2) so that any third-party logging to stdout
(e.g., TableFormer, tqdm) does not corrupt protocol framing on the caller's end.
Protocol messages are written exclusively to a duplicated fd pointing to original stdout.
"""

from __future__ import annotations

import json
import logging
import os
import re
import subprocess
import sys
import time
from pathlib import Path
from typing import Any, Dict, Optional

# Step 1: Duplicate stdout fd and redirect fd 1 to stderr
_ORIG_STDOUT_FD = os.dup(1)
os.dup2(2, 1)

# Protocol output stream
_protocol_stream = os.fdopen(_ORIG_STDOUT_FD, "w", encoding="utf-8", buffering=1)


def send_event(event: Dict[str, Any]) -> None:
    """Emits an NDJSON protocol event line to the dedicated protocol stream."""
    line = json.dumps(event, ensure_ascii=False)
    _protocol_stream.write(line + "\n")
    _protocol_stream.flush()


def run_describe(request: Dict[str, Any]) -> None:
    """Handles op == 'describe' by inspecting local tools, versions, and models."""
    import docling

    tess_ver = None
    try:
        res = subprocess.run(
            ["tesseract", "--version"],
            capture_output=True,
            text=True,
            check=False,
        )
        if res.returncode == 0:
            first_line = res.stdout.splitlines()[0] if res.stdout else ""
            m = re.search(r"tesseract\s+([0-9.]+)", first_line)
            tess_ver = m.group(1) if m else first_line
    except Exception:
        pass

    options = request.get("options", {})
    artifacts_path = options.get("artifacts_path")
    models_available = []
    if artifacts_path and Path(artifacts_path).exists():
        art_p = Path(artifacts_path)
        for p in art_p.iterdir():
            if p.is_dir():
                models_available.append(p.name)

    metrics = {
        "engine": "docling",
        "version": getattr(docling, "__version__", "2.134.0"),
        "python_version": sys.version.split()[0],
        "tesseract_version": tess_ver,
        "models_available": models_available,
        "routes": [
            {"input": "pdf", "output": "docling+json"},
            {"input": "pdf", "output": "ariad-ir+json"},
            {"input": "png", "output": "docling+json"},
            {"input": "png", "output": "ariad-ir+json"},
            {"input": "jpeg", "output": "docling+json"},
            {"input": "jpeg", "output": "ariad-ir+json"},
        ],
        "license": "MIT",
    }

    send_event({
        "type": "result",
        "ok": True,
        "metrics": metrics,
    })


def main() -> None:
    start_time = time.monotonic()

    # Read request line from stdin
    raw_line = sys.stdin.readline()
    if not raw_line:
        send_event({
            "type": "result",
            "ok": False,
            "error": {
                "code": "invalid_request",
                "message": "Empty stdin; expected JSON request line",
            },
        })
        sys.exit(0)

    try:
        request = json.loads(raw_line)
    except Exception as e:
        send_event({
            "type": "result",
            "ok": False,
            "error": {
                "code": "invalid_request",
                "message": f"Malformed request JSON: {e}",
            },
        })
        sys.exit(0)

    protocol = request.get("protocol")
    if protocol != "ariad-engine/1":
        send_event({
            "type": "result",
            "ok": False,
            "error": {
                "code": "invalid_request",
                "message": f"Unsupported protocol '{protocol}'; expected 'ariad-engine/1'",
            },
        })
        sys.exit(0)

    op = request.get("op", "convert")

    # Extract options and set environment for offline operation before imports
    options = request.get("options", {})
    work_dir = request.get("work_dir")
    if work_dir:
        os.environ["TMPDIR"] = str(work_dir)
        os.environ["TEMP"] = str(work_dir)
        os.environ["TMP"] = str(work_dir)
        try:
            pid_path = Path(work_dir) / "engine.pid"
            pid_path.write_text(f"{os.getpid()}\n{os.getpgrp()}\n", encoding="utf-8")
        except Exception:
            pass

    os.environ["HF_HUB_OFFLINE"] = "1"
    os.environ["TRANSFORMERS_OFFLINE"] = "1"

    tessdata_path = options.get("tessdata_path")
    if tessdata_path:
        os.environ["TESSDATA_PREFIX"] = str(tessdata_path)

    artifacts_path = options.get("artifacts_path")
    if artifacts_path:
        os.environ["DOCLING_ARTIFACTS_PATH"] = str(artifacts_path)

    # Disable remote services and external plugins
    os.environ["DOCLING_ENABLE_REMOTE_SERVICES"] = "0"
    os.environ["DOCLING_ALLOW_EXTERNAL_PLUGINS"] = "0"

    if op == "describe":
        run_describe(request)
        sys.exit(0)

    if op != "convert":
        send_event({
            "type": "result",
            "ok": False,
            "error": {
                "code": "unsupported_route",
                "message": f"Unsupported op '{op}'",
            },
        })
        sys.exit(0)

    # Lazy import Docling modules
    try:
        from docling.document_converter import (
            DocumentConverter,
            PdfFormatOption,
            ImageFormatOption,
        )
        from docling.datamodel.pipeline_options import (
            PdfPipelineOptions,
            AcceleratorOptions,
            TesseractCliOcrOptions,
            OcrMode,
        )
        from docling.datamodel.base_models import InputFormat
        import to_ir
    except Exception as e:
        send_event({
            "type": "result",
            "ok": False,
            "error": {
                "code": "engine_failure",
                "message": f"Failed to import docling: {e}",
            },
        })
        sys.exit(0)

    input_info = request.get("input", {})
    input_path = input_info.get("path")
    input_format = input_info.get("format", "").lower()

    output_info = request.get("output", {})
    output_dir = output_info.get("dir")
    output_format = output_info.get("format", "").lower()

    if not input_path or not Path(input_path).exists():
        send_event({
            "type": "result",
            "ok": False,
            "error": {
                "code": "invalid_request",
                "message": f"Input path does not exist: {input_path}",
            },
        })
        sys.exit(0)

    if not output_dir:
        send_event({
            "type": "result",
            "ok": False,
            "error": {
                "code": "invalid_request",
                "message": "Missing output.dir in request",
            },
        })
        sys.exit(0)

    out_dir_path = Path(output_dir)
    out_dir_path.mkdir(parents=True, exist_ok=True)

    # Validate route
    supported_in = {"pdf", "png", "jpeg", "jpg"}
    supported_out = {"docling+json", "ariad-ir+json"}

    if input_format not in supported_in:
        send_event({
            "type": "result",
            "ok": False,
            "error": {
                "code": "unsupported_route",
                "message": f"Unsupported input format: {input_format}",
            },
        })
        sys.exit(0)

    if output_format not in supported_out:
        send_event({
            "type": "result",
            "ok": False,
            "error": {
                "code": "unsupported_route",
                "message": f"Unsupported output format: {output_format}",
            },
        })
        sys.exit(0)

    # Configure OCR
    ocr_lang = options.get("ocr_lang", ["vie", "eng"])
    do_ocr = bool(options.get("do_ocr", input_format in {"png", "jpeg", "jpg"}))
    ocr_mode_str = options.get("ocr_mode", "auto")

    ocr_options = None
    if do_ocr:
        mode = OcrMode.FULL_PAGE if (input_format in {"png", "jpeg", "jpg"} or ocr_mode_str == "full_page") else OcrMode.PDF_AWARE_LAYOUT_REGIONS
        ocr_options = TesseractCliOcrOptions(
            tesseract_cmd="tesseract",
            path=str(tessdata_path) if tessdata_path else None,
            lang=ocr_lang,
            mode=mode,
        )

    pipeline_kwargs: Dict[str, Any] = {
        "artifacts_path": Path(artifacts_path) if artifacts_path else None,
        "do_ocr": do_ocr,
        "do_table_structure": True,
        "generate_picture_images": True,
        "accelerator_options": AcceleratorOptions(num_threads=4),
    }
    if ocr_options is not None:
        pipeline_kwargs["ocr_options"] = ocr_options

    pipeline_options = PdfPipelineOptions(**pipeline_kwargs)

    # Hook progress via logging handler
    total_pages_detected = [1]
    completed_pages = set()

    class ProgressHandler(logging.Handler):
        def emit(self, record):
            msg = record.getMessage()
            if "PIPELINE_PROFILING Stage assemble:" in msg:
                # Extract page numbers from pages=[...]
                m = re.search(r"pages=\[([0-9, ]+)\]", msg)
                if m:
                    pgs = [int(p.strip()) for p in m.group(1).split(",") if p.strip()]
                    for p in pgs:
                        completed_pages.add(p)
                    send_event({
                        "type": "progress",
                        "stage": "convert",
                        "done": len(completed_pages),
                        "total": max(len(completed_pages), total_pages_detected[0]),
                    })

    prog_handler = ProgressHandler()
    pipeline_logger = logging.getLogger("docling.pipeline.standard_pdf_pipeline")
    pipeline_logger.addHandler(prog_handler)
    pipeline_logger.setLevel(logging.DEBUG)

    # Build converter
    format_options = {}
    if input_format == "pdf":
        format_options[InputFormat.PDF] = PdfFormatOption(pipeline_options=pipeline_options)
    elif input_format in {"png", "jpeg", "jpg"}:
        format_options[InputFormat.IMAGE] = ImageFormatOption(pipeline_options=pipeline_options)

    converter = DocumentConverter(format_options=format_options)

    try:
        conv_res = converter.convert(input_path)
    except Exception as e:
        pipeline_logger.removeHandler(prog_handler)
        send_event({
            "type": "result",
            "ok": False,
            "error": {
                "code": "engine_failure",
                "message": f"Conversion error: {e}",
            },
        })
        sys.exit(0)

    pipeline_logger.removeHandler(prog_handler)

    num_pages = len(conv_res.pages) if conv_res.pages else 1
    artifacts = []

    # Emit final progress
    send_event({
        "type": "progress",
        "stage": "convert",
        "done": num_pages,
        "total": num_pages,
    })

    if output_format == "docling+json":
        out_file = out_dir_path / "document.docling.json"
        conv_res.document.save_as_json(out_file)
        send_event({
            "type": "artifact",
            "path": str(out_file),
            "format": "docling+json",
        })
    elif output_format == "ariad-ir+json":
        out_file = out_dir_path / "document.ir.json"
        ir_doc, lost = to_ir.docling_to_ir(conv_res.document, source_format=input_format)
        with open(out_file, "w", encoding="utf-8") as f:
            json.dump(ir_doc, f, indent=2, ensure_ascii=False)
        send_event({
            "type": "artifact",
            "path": str(out_file),
            "format": "ariad-ir+json",
        })

    elapsed_ms = int((time.monotonic() - start_time) * 1000)
    send_event({
        "type": "result",
        "ok": True,
        "metrics": {
            "pages": num_pages,
            "elapsed_ms": elapsed_ms,
        },
    })


if __name__ == "__main__":
    main()
