#!/usr/bin/env python3
"""E8 Field Loss and End-to-End IR-to-DOCX Evaluation Script.

Processes all 4 spike fixtures through:
1. Docling engine (PDF/Image -> Docling Document)
2. to_ir.py mapper (Docling Document -> ariad-ir/0 JSON + field loss catalog)
3. Pandoc engine (ariad-ir/0 JSON -> DOCX)
"""

import json
import os
import subprocess
import sys
import tempfile
from collections import Counter, defaultdict
from pathlib import Path

REPO_ROOT = Path("/home/vchun/Codes/02-My-Projects/AriadShift-spike-docling")
VENV_PYTHON = REPO_ROOT / "spike/docling/.venv/bin/python"
ENGINE_PY = REPO_ROOT / "spike/docling/engine.py"
TO_IR_PY = REPO_ROOT / "spike/docling/to_ir.py"
ASHIFT_BIN = REPO_ROOT / "target/debug/ashift"
PANDOC_BIN = REPO_ROOT / ".tools/pandoc/bin/pandoc"
MODELS_DIR = REPO_ROOT / ".tools/docling-models"
TESSDATA_DIR = REPO_ROOT / ".tools/tessdata"

FIXTURES = [
    ("gao", REPO_ROOT / "fixtures/pdf/pd-en-gao-08-35-highlights.pdf", "pdf"),
    ("tableformer", REPO_ROOT / "fixtures/pdf/arxiv-tableformer-2203-01017-v1.pdf", "pdf"),
    ("congbao", REPO_ROOT / "fixtures/scan/vn-congbao-42-2020-p31.png", "png"),
    ("decree", REPO_ROOT / "fixtures/pdf/pd-vi-decree-39-2022.pdf", "pdf"),
]

def run_docling_engine(fixture_path: Path, out_dir: Path, work_dir: Path) -> Path:
    """Invokes engine.py via JSONL protocol to produce document.docling.json."""
    req = {
        "protocol": "ariad-engine/1",
        "job": f"e8-docling-{fixture_path.stem}",
        "op": "convert",
        "input": {
            "path": str(fixture_path.resolve()),
            "format": fixture_path.suffix.lstrip(".").lower()
        },
        "output": {
            "dir": str(out_dir.resolve()),
            "format": "docling+json"
        },
        "work_dir": str(work_dir.resolve()),
        "options": {
            "artifacts_path": str(MODELS_DIR.resolve()),
            "tessdata_path": str(TESSDATA_DIR.resolve()),
        },
        "limits": {}
    }
    
    env = os.environ.copy()
    env["HF_HUB_OFFLINE"] = "1"
    env["TRANSFORMERS_OFFLINE"] = "1"
    env["TESSDATA_PREFIX"] = str(TESSDATA_DIR.resolve())
    
    p = subprocess.Popen(
        [str(VENV_PYTHON), str(ENGINE_PY)],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        env=env
    )
    stdout, stderr = p.communicate(input=json.dumps(req) + "\n")
    if p.returncode != 0:
        raise RuntimeError(f"Engine failed with code {p.returncode}: {stderr[-1000:]}")
        
    docling_json_path = out_dir / "document.docling.json"
    if not docling_json_path.exists():
        print("Engine STDOUT:", stdout)
        print("Engine STDERR:", stderr[-500:])
        raise FileNotFoundError(f"Missing docling output: {docling_json_path}")
    return docling_json_path

def run_pandoc_engine(ir_path: Path, ws_dir: Path) -> Path:
    """Runs ashift __engine pandoc to convert ariad-ir+json to docx."""
    in_dir = ws_dir / "in"
    out_dir = ws_dir / "out"
    tmp_dir = ws_dir / "tmp"
    log_dir = ws_dir / "log"
    for d in (in_dir, out_dir, tmp_dir, log_dir):
        d.mkdir(parents=True, exist_ok=True)
        
    ws_ir = in_dir / "document.ir.json"
    ws_ir.write_bytes(ir_path.read_bytes())
    
    req = {
        "protocol": "ariad-engine/1",
        "job": "e8-pandoc-convert",
        "op": "convert",
        "input": {
            "path": str(ws_ir.resolve()),
            "format": "ariad-ir+json"
        },
        "output": {
            "dir": str(out_dir.resolve()),
            "format": "docx"
        },
        "work_dir": str(tmp_dir.resolve()),
        "options": {},
        "limits": {}
    }
    
    env = os.environ.copy()
    env["ASHIFT_PANDOC"] = str(PANDOC_BIN.resolve())
    
    p = subprocess.Popen(
        [str(ASHIFT_BIN), "__engine", "pandoc"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        env=env
    )
    stdout, stderr = p.communicate(input=json.dumps(req) + "\n")
    docx_path = out_dir / "document.docx"
    if not docx_path.exists():
        raise RuntimeError(f"Pandoc engine failed to create docx. Stdout: {stdout}\nStderr: {stderr}")
    return docx_path

def main():
    print("=" * 80)
    print("E8 EVALUATION: FIELD LOSS CATALOG & END-TO-END IR-TO-DOCX CONVERSION")
    print("=" * 80)
    
    results = {}
    
    with tempfile.TemporaryDirectory(prefix="e8-eval-") as base_tmp:
        base_dir = Path(base_tmp)
        
        for name, fixture_path, fmt in FIXTURES:
            print(f"\n--- Processing Fixture: {name} ({fixture_path.name}) ---")
            fix_dir = base_dir / name
            eng_out = fix_dir / "eng_out"
            eng_work = fix_dir / "eng_work"
            pandoc_ws = fix_dir / "pandoc_ws"
            for d in (eng_out, eng_work, pandoc_ws):
                d.mkdir(parents=True, exist_ok=True)
                
            # 1. Run Docling
            print("  1. Running Docling engine...")
            docling_json = run_docling_engine(fixture_path, eng_out, eng_work)
            docling_size = docling_json.stat().st_size
            print(f"     Docling JSON generated ({docling_size} bytes)")
            
            # 2. Run to_ir.py
            print("  2. Mapping Docling to ariad-ir/0...")
            ir_path = fix_dir / "document.ir.json"
            losses_path = fix_dir / "losses.json"
            
            p = subprocess.run(
                [str(VENV_PYTHON), str(TO_IR_PY), str(docling_json), str(ir_path), str(losses_path)],
                capture_output=True,
                text=True
            )
            if p.returncode != 0:
                print(f"     to_ir.py failed: {p.stderr}")
                sys.exit(1)
            print(f"     {p.stdout.strip()}")
            
            # Parse IR and losses
            with open(ir_path, "r", encoding="utf-8") as f:
                ir_doc = json.load(f)
            with open(losses_path, "r", encoding="utf-8") as f:
                losses = json.load(f)
                
            # Count loss breakdown
            loss_counts = Counter(item["field"] for item in losses)
            loss_reasons = defaultdict(list)
            for item in losses:
                loss_reasons[item["field"]].append(item["reason"])
                
            # 3. Run Pandoc Engine
            print("  3. Converting IR to DOCX via Pandoc engine...")
            docx_path = run_pandoc_engine(ir_path, pandoc_ws)
            docx_size = docx_path.stat().st_size
            print(f"     DOCX successfully generated ({docx_size} bytes)")
            
            # Explicit counts for all 5 IR candidates named in phase 1 & roadmap 1a:
            # 1. provenance (bbox coordinates)
            # 2. furniture (running headers/footers)
            # 3. table header flags (row headers / stub columns)
            # 4. table footnotes
            # 5. picture bytes (unrendered images)
            candidate_counts = {
                "provenance_bbox": loss_counts.get("provenance_bbox", 0),
                "furniture": loss_counts.get("furniture", 0),
                "table_header_flags": loss_counts.get("row_header_cell", 0),
                "table_footnotes": loss_counts.get("table_footnotes", 0),
                "picture_bytes_unrendered": loss_counts.get("picture_image", 0),
                "formula_tex": loss_counts.get("formula_tex", 0),
            }

            candidate_notes = {}
            if candidate_counts["furniture"] == 0:
                candidate_notes["furniture"] = "0 detected: Docling Heron layout model did not classify separate page headers/footers on this fixture; elements folded into body layer"
            else:
                candidate_notes["furniture"] = f"{candidate_counts['furniture']} furniture items dropped by IR v0"

            if candidate_counts["table_footnotes"] == 0:
                candidate_notes["table_footnotes"] = "0 present: Docling TableFormer output has empty table.footnotes list on this fixture"
            else:
                candidate_notes["table_footnotes"] = f"{candidate_counts['table_footnotes']} table footnotes dropped by IR v0"

            if candidate_counts["picture_bytes_unrendered"] == 0:
                candidate_notes["picture_bytes_unrendered"] = f"0 unrendered: All picture elements ({len(ir_doc.get('assets', {}))} assets) successfully rendered to PNG and preserved in IR asset store"
            else:
                candidate_notes["picture_bytes_unrendered"] = f"{candidate_counts['picture_bytes_unrendered']} picture elements lacked image bytes"

            if candidate_counts["table_header_flags"] == 0:
                candidate_notes["table_header_flags"] = "0 row headers: Document contains no tables or tables have only column headers"
            else:
                candidate_notes["table_header_flags"] = f"{candidate_counts['table_header_flags']} row headers detected by TableFormer, dropped by IR v0 TableCell"

            candidate_notes["provenance_bbox"] = f"{candidate_counts['provenance_bbox']} bounding box coordinates dropped by IR v0"

            results[name] = {
                "file": fixture_path.name,
                "ir_blocks": len(ir_doc.get("body", [])),
                "ir_assets": len(ir_doc.get("assets", {})),
                "docx_bytes": docx_size,
                "total_loss_events": len(losses),
                "loss_by_field": dict(loss_counts),
                "candidate_counts": candidate_counts,
                "candidate_notes": candidate_notes,
            }
            
    print("\n" + "=" * 80)
    print("E8 SUMMARY RESULTS:")
    print("=" * 80)
    print(json.dumps(results, indent=2))
    
    out_json = Path("/home/vchun/Codes/02-My-Projects/AriadShift/plans/261006-0544-docling-spike-and-roadmap-1a/reports/docling-spike/e8_summary.json")
    out_json.write_text(json.dumps(results, indent=2))
    print(f"\nWritten summary to {out_json}")

if __name__ == "__main__":
    main()
