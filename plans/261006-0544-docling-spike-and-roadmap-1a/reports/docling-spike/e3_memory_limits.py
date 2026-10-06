#!/usr/bin/env python3
"""E3: Resource limit evaluation for Docling under RLIMIT_AS and RLIMIT_DATA on Linux.

Runs a memory ladder (1, 2, 3, 4, 6, 8 GiB) for both RLIMIT_AS and RLIMIT_DATA
applied to REAL conversions of gao (1 page) and tableformer (10 pages) fixtures
through the engine.py adapter.

Records per step:
- limit_type: RLIMIT_AS or RLIMIT_DATA
- limit_mb: cap in MiB
- fixture: gao or tableformer
- exit_code: process return code
- peak_rss_mb: peak resident set size in MiB
- wall_time_s: total conversion duration in seconds
- cleanliness: clean_ok, clean_err, or abort_sig
- failure_detail: error message or termination signal details

Outputs raw results to e3_results.txt and prints a formatted summary table.
"""

from __future__ import annotations

import json
import os
import resource
import subprocess
import sys
import time
from pathlib import Path
from typing import Any, Dict, List, Tuple

REPO_ROOT = Path("/home/vchun/Codes/02-My-Projects/AriadShift-spike-docling")
VENV_PYTHON = REPO_ROOT / "spike/docling/.venv/bin/python"
ENGINE_PY = REPO_ROOT / "spike/docling/engine.py"
MODELS_DIR = REPO_ROOT / ".tools/docling-models"
TESSDATA_DIR = REPO_ROOT / ".tools/tessdata"
RESULTS_FILE = Path("/home/vchun/Codes/02-My-Projects/AriadShift/plans/261006-0544-docling-spike-and-roadmap-1a/reports/docling-spike/e3_results.txt")

FIXTURES = {
    "gao": REPO_ROOT / "fixtures/pdf/pd-en-gao-08-35-highlights.pdf",
    "tableformer": REPO_ROOT / "fixtures/pdf/arxiv-tableformer-2203-01017-v1.pdf",
}

LADDER_MB = [1024, 2048, 3072, 4096, 6144, 8192]


def run_single_step(limit_type: str, limit_mb: int, fixture_key: str, fixture_path: Path) -> Dict[str, Any]:
    """Runs a single conversion step under a child process configured with setrlimit."""
    runner_code = """
import json, os, resource, subprocess, sys, time

limit_type = sys.argv[1]
limit_mb = int(sys.argv[2])
fixture_path = sys.argv[3]
engine_py = sys.argv[4]
models_dir = sys.argv[5]
tessdata_dir = sys.argv[6]
out_dir = sys.argv[7]
work_dir = sys.argv[8]

res_const = resource.RLIMIT_AS if limit_type == "RLIMIT_AS" else resource.RLIMIT_DATA
limit_bytes = limit_mb * 1024 * 1024

try:
    resource.setrlimit(res_const, (limit_bytes, limit_bytes))
except Exception as e:
    print(json.dumps({"error": f"setrlimit failed: {e}", "rc": 99}))
    sys.exit(99)

req = {
    "protocol": "ariad-engine/1",
    "job": f"e3-{limit_type}-{limit_mb}",
    "op": "convert",
    "input": {"path": fixture_path, "format": "pdf"},
    "output": {"dir": out_dir, "format": "docling+json"},
    "work_dir": work_dir,
    "options": {
        "artifacts_path": models_dir,
        "tessdata_path": tessdata_dir,
    },
    "limits": {},
}

start = time.monotonic()
p = subprocess.Popen(
    [sys.executable, engine_py],
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    text=True,
)
stdout, stderr = p.communicate(input=json.dumps(req) + "\\n")
wall_time = time.monotonic() - start
ru = resource.getrusage(resource.RUSAGE_CHILDREN)

print(json.dumps({
    "rc": p.returncode,
    "wall_time": wall_time,
    "maxrss_kb": ru.ru_maxrss,
    "stdout": stdout,
    "stderr_tail": stderr[-1000:] if stderr else "",
}))
"""
    tmp_base = Path(f"/tmp/e3_ladder/{limit_type}_{limit_mb}_{fixture_key}")
    out_dir = tmp_base / "out"
    work_dir = tmp_base / "work"
    out_dir.mkdir(parents=True, exist_ok=True)
    work_dir.mkdir(parents=True, exist_ok=True)

    res = subprocess.run(
        [
            str(VENV_PYTHON),
            "-c",
            runner_code,
            limit_type,
            str(limit_mb),
            str(fixture_path),
            str(ENGINE_PY),
            str(MODELS_DIR),
            str(TESSDATA_DIR),
            str(out_dir),
            str(work_dir),
        ],
        capture_output=True,
        text=True,
    )

    if res.returncode != 0 and not res.stdout.strip():
        return {
            "limit_type": limit_type,
            "limit_mb": limit_mb,
            "fixture": fixture_key,
            "rc": res.returncode,
            "peak_rss_mb": 0.0,
            "wall_time_s": 0.0,
            "cleanliness": "abort_sig",
            "detail": f"Wrapper killed: {res.stderr.strip()[:200]}",
        }

    try:
        raw_info = json.loads(res.stdout.strip().splitlines()[-1])
    except Exception as e:
        return {
            "limit_type": limit_type,
            "limit_mb": limit_mb,
            "fixture": fixture_key,
            "rc": res.returncode,
            "peak_rss_mb": 0.0,
            "wall_time_s": 0.0,
            "cleanliness": "abort_sig",
            "detail": f"Failed to parse wrapper output: {res.stdout[:200]} (err: {e})",
        }

    rc = raw_info["rc"]
    wall_time = round(raw_info["wall_time"], 2)
    peak_rss_mb = round(raw_info["maxrss_kb"] / 1024.0, 1)
    stdout = raw_info.get("stdout", "")
    stderr_tail = raw_info.get("stderr_tail", "")

    # Analyze cleanliness
    cleanliness = "unknown"
    detail = ""

    # Parse stdout for protocol result event
    result_event = None
    for line in stdout.splitlines():
        try:
            ev = json.loads(line)
            if ev.get("type") == "result":
                result_event = ev
                break
        except Exception:
            pass

    if rc == 0:
        if result_event and result_event.get("ok"):
            cleanliness = "clean_ok"
            detail = f"Success ({result_event.get('metrics', {}).get('pages', 1)} pages, {result_event.get('metrics', {}).get('elapsed_ms', 0)}ms)"
        elif result_event and not result_event.get("ok"):
            cleanliness = "clean_err"
            err_msg = result_event.get("error", {}).get("message", "Unknown error")
            detail = f"Protocol error event: {err_msg[:120]}"
        else:
            cleanliness = "clean_ok" if not stderr_tail else "clean_err"
            detail = f"RC 0 without result event: {stdout[:100]}"
    elif rc < 0:
        cleanliness = "abort_sig"
        sig_num = -rc
        detail = f"Killed by signal {sig_num} (e.g. SIGABRT/SIGSEGV). Stderr: {stderr_tail.replace(chr(10), ' ')[:140]}"
    else:
        cleanliness = "clean_err" if ("MemoryError" in stderr_tail or "MemoryError" in stdout) else "abort_sig"
        detail = f"Exit code {rc}. Stderr: {stderr_tail.replace(chr(10), ' ')[:140]}"

    return {
        "limit_type": limit_type,
        "limit_mb": limit_mb,
        "fixture": fixture_key,
        "rc": rc,
        "peak_rss_mb": peak_rss_mb,
        "wall_time_s": wall_time,
        "cleanliness": cleanliness,
        "detail": detail,
    }


def main():
    print("=" * 80)
    print("E3 EXPERIMENT: SYSTEMATIC MEMORY LIMIT LADDER (RLIMIT_AS vs RLIMIT_DATA)")
    print("=" * 80)

    results: List[Dict[str, Any]] = []

    # Run ladder for GAO fixture first
    for limit_type in ["RLIMIT_AS", "RLIMIT_DATA"]:
        print(f"\n--- Testing {limit_type} on fixture: gao (1 page) ---")
        for limit_mb in LADDER_MB:
            res = run_single_step(limit_type, limit_mb, "gao", FIXTURES["gao"])
            results.append(res)
            print(f"  {limit_type} {limit_mb:4d} MB: exit={res['rc']:3d}, peak_rss={res['peak_rss_mb']:6.1f} MB, wall={res['wall_time_s']:5.2f}s, cleanliness={res['cleanliness']:9s} | {res['detail']}")

    # Run ladder for TableFormer fixture
    for limit_type in ["RLIMIT_AS", "RLIMIT_DATA"]:
        print(f"\n--- Testing {limit_type} on fixture: tableformer (10 pages) ---")
        for limit_mb in LADDER_MB:
            res = run_single_step(limit_type, limit_mb, "tableformer", FIXTURES["tableformer"])
            results.append(res)
            print(f"  {limit_type} {limit_mb:4d} MB: exit={res['rc']:3d}, peak_rss={res['peak_rss_mb']:6.1f} MB, wall={res['wall_time_s']:5.2f}s, cleanliness={res['cleanliness']:9s} | {res['detail']}")

    # Format raw report text
    lines = []
    lines.append("# E3 Memory Limits Ladder Empirical Results")
    lines.append(f"Generated at: {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}")
    lines.append("Environment: Linux x86_64, Python 3.14, Docling 2.134.0, PyTorch 2.14.1+cpu")
    lines.append("")
    lines.append("## Results Table")
    lines.append("| Limit Type | Limit (MiB) | Fixture | Exit Code | Peak RSS (MiB) | Wall Time (s) | Cleanliness | Detail / Diagnostic |")
    lines.append("|---|---|---|---|---|---|---|---|")
    for r in results:
        lines.append(f"| {r['limit_type']} | {r['limit_mb']} | {r['fixture']} | {r['rc']} | {r['peak_rss_mb']} | {r['wall_time_s']} | {r['cleanliness']} | {r['detail']} |")

    # Compute smallest workable caps
    lines.append("")
    lines.append("## Smallest Workable Caps")
    for limit_type in ["RLIMIT_AS", "RLIMIT_DATA"]:
        for fix in ["gao", "tableformer"]:
            matching = [r for r in results if r["limit_type"] == limit_type and r["fixture"] == fix and r["cleanliness"] == "clean_ok"]
            if matching:
                smallest = min(matching, key=lambda x: x["limit_mb"])
                lines.append(f"- **{limit_type} ({fix})**: Smallest workable cap = {smallest['limit_mb']} MiB ({smallest['limit_mb'] / 1024:.1f} GiB), Peak RSS = {smallest['peak_rss_mb']} MiB, Time = {smallest['wall_time_s']}s")
            else:
                lines.append(f"- **{limit_type} ({fix})**: No tested limit succeeded up to 8192 MiB")

    content = "\n".join(lines) + "\n"
    RESULTS_FILE.parent.mkdir(parents=True, exist_ok=True)
    RESULTS_FILE.write_text(content, encoding="utf-8")
    print("\n" + "=" * 80)
    print(f"Results successfully saved to {RESULTS_FILE}")
    print("=" * 80)


if __name__ == "__main__":
    main()
