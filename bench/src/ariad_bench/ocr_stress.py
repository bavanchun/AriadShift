"""Measure a real 50-page OCR job using a licensed one-page scan."""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile

from ariad_bench.metrics import measure_execution
from ariad_bench.ocr_eval import evidence


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ashift", type=Path, required=True)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--timeout", type=int, default=600)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="ariad-ocr-stress-") as scratch:
        work = Path(scratch)
        source = work / "fifty-pages.pdf"
        pages = [item for _ in range(50) for item in (str(args.fixture.resolve()), "1")]
        subprocess.run(["qpdf", "--empty", "--pages", *pages, "--", str(source)], check=True)
        command = [str(args.ashift.resolve()), "tool", "pdf.ocr", str(source), "-o", str(work / "ocr.pdf")]
        code, stdout, stderr, wall_ms, peak_rss = measure_execution(
            command, env=os.environ.copy(), timeout_s=args.timeout
        )
        if code or peak_rss is None:
            raise RuntimeError(f"OCR failed ({code}), peak RSS {peak_rss}: {stderr[-2000:]} {stdout[-500:]}")
        args.output.write_text(json.dumps({
            "fixture": evidence(args.fixture),
            "pages": 50,
            "command": ["ashift", "tool", "pdf.ocr", "fifty-pages.pdf", "-o", "ocr.pdf"],
            "wall_ms": wall_ms,
            "peak_process_tree_rss_bytes": peak_rss,
            "timeout_s": args.timeout,
            "exit_code": code,
        }, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
