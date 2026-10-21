"""Diff subcommand generating Markdown tables of capability metric changes."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import subprocess
import sys
from typing import Any


def format_delta(current: float, baseline: float | None, precision: int = 3, suffix: str = "") -> str:
    """Format a metric value alongside its delta from baseline."""
    if baseline is None:
        return f"{current:.{precision}f}{suffix}"
    delta = current - baseline
    threshold = 0.5 * (10 ** (-precision))
    if abs(delta) < threshold:
        return f"{current:.{precision}f}{suffix}"
    sign = "+" if delta > 0 else ""
    return f"{current:.{precision}f}{suffix} ({sign}{delta:.{precision}f}{suffix})"


def generate_diff_table(
    current_data: dict[str, Any],
    baseline_data: dict[str, Any] | None,
) -> str:
    """Produce a Markdown table comparing current capabilities to baseline capabilities."""
    baseline_edges: dict[tuple[str, str, str], dict[str, Any]] = {}
    if baseline_data:
        for edge in baseline_data.get("edges", []):
            key = (edge.get("from", ""), edge.get("to", ""), edge.get("engine", ""))
            baseline_edges[key] = edge.get("metrics") or {}

    rows: list[str] = [
        "| Edge | Engine | Fidelity | Editability | p50 Latency | Peak Memory | Samples |",
        "|---|---|---|---|---|---|---|",
    ]

    for edge in current_data.get("edges", []):
        from_fmt = edge.get("from", "")
        to_fmt = edge.get("to", "")
        engine = edge.get("engine", "")
        metrics = edge.get("metrics")
        edge_key = (from_fmt, to_fmt, engine)
        base_metrics = baseline_edges.get(edge_key, {})

        edge_label = f"`{from_fmt} → {to_fmt}`"
        if metrics is None:
            rows.append(f"| {edge_label} | {engine} | *unmeasured* | *unmeasured* | - | - | 0 |")
            continue

        c_fid = metrics.get("fidelity", 0.0)
        b_fid = base_metrics.get("fidelity")
        fid_str = format_delta(c_fid, b_fid, 3)

        c_edit = metrics.get("editability", 0.0)
        b_edit = base_metrics.get("editability")
        edit_str = format_delta(c_edit, b_edit, 3)

        c_p50 = metrics.get("p50_ms", 0.0)
        b_p50 = base_metrics.get("p50_ms")
        p50_str = format_delta(c_p50, b_p50, 0, " ms")

        c_mem = metrics.get("peak_mem_mb", 0.0)
        b_mem = base_metrics.get("peak_mem_mb")
        mem_str = format_delta(c_mem, b_mem, 0, " MB")

        samples = metrics.get("samples", 0)
        rows.append(f"| {edge_label} | {engine} | {fid_str} | {edit_str} | {p50_str} | {mem_str} | {samples} |")

    return "\n".join(rows) + "\n"


def build_parser(parser: argparse.ArgumentParser | None = None) -> argparse.ArgumentParser:
    """Configure CLI parser for the diff subcommand."""
    if parser is None:
        parser = argparse.ArgumentParser(description="Generate Markdown diff table for capabilities metrics")
    parser.add_argument(
        "--new",
        type=Path,
        default=Path("crates/ariad-core/data/capabilities.json"),
        help="Path to current/new capabilities.json",
    )
    parser.add_argument(
        "--baseline",
        type=str,
        default="HEAD",
        help="Baseline file path or 'HEAD' to read from git",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)

    if not args.new.is_file():
        print(f"Error: file not found: {args.new}", file=sys.stderr)
        return 1

    current_data = json.loads(args.new.read_text(encoding="utf-8"))
    baseline_data: dict[str, Any] | None = None

    if args.baseline == "HEAD":
        try:
            cmd = ["git", "show", "HEAD:crates/ariad-core/data/capabilities.json"]
            proc = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", check=False)
            if proc.returncode == 0 and proc.stdout.strip():
                baseline_data = json.loads(proc.stdout)
        except Exception:
            pass
    else:
        baseline_path = Path(args.baseline)
        if not baseline_path.is_file():
            print(f"Error: baseline file not found: {args.baseline}", file=sys.stderr)
            return 1
        baseline_data = json.loads(baseline_path.read_text(encoding="utf-8"))

    diff_table = generate_diff_table(current_data, baseline_data)
    print(diff_table)
    return 0


if __name__ == "__main__":
    sys.exit(main())
