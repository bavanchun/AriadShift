"""Run subcommand executing empirical benchmarks for reader and writer edges."""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib
from typing import Any

from ariad_bench.canonical import CanonicalDoc, normalize
from ariad_bench.metrics import (
    SingleFixtureScore,
    aggregate_edge_metrics,
    measure_execution,
    score_fixture,
)
from ariad_bench.references import get_reference_for_fixture


def find_binary(name: str, explicit_path: Path | str | None, candidates: list[Path]) -> Path:
    """Locate an executable binary by explicit path, search candidates, or system PATH."""
    exe_suffix = ".exe" if sys.platform == "win32" else ""
    if explicit_path:
        p = Path(explicit_path).resolve()
        if p.is_file() and os.access(p, os.X_OK):
            return p
        if exe_suffix and not str(p).endswith(exe_suffix):
            p_exe = Path(f"{p}{exe_suffix}").resolve()
            if p_exe.is_file() and os.access(p_exe, os.X_OK):
                return p_exe
        raise FileNotFoundError(f"Specified {name} binary not found or not executable: {explicit_path}")

    for cand in candidates:
        if cand.is_file() and os.access(cand, os.X_OK):
            return cand.resolve()
        if exe_suffix and not str(cand).endswith(exe_suffix):
            cand_exe = Path(f"{cand}{exe_suffix}")
            if cand_exe.is_file() and os.access(cand_exe, os.X_OK):
                return cand_exe.resolve()

    which_name = f"{name}{exe_suffix}" if exe_suffix and not name.endswith(exe_suffix) else name
    which_path = shutil.which(which_name) or shutil.which(name)
    if which_path:
        return Path(which_path).resolve()

    raise FileNotFoundError(f"Could not locate {name} executable. Please provide --{name}.")


def get_binary_version(bin_path: Path) -> str:
    """Extract version string from a binary."""
    try:
        proc = subprocess.run(
            [str(bin_path), "--version"],
            capture_output=True,
            text=True,
            encoding="utf-8",
            check=False,
        )
        first_line = proc.stdout.strip().splitlines()[0] if proc.stdout.strip() else ""
        return first_line.split()[-1] if first_line else "unknown"
    except Exception:
        return "unknown"


def match_format(format_name: str, target: str) -> bool:
    """Match format synonyms (e.g. md/markdown)."""
    norm_from = "markdown" if format_name in ("md", "markdown") else format_name.lower()
    norm_target = "markdown" if target in ("md", "markdown") else target.lower()
    return norm_from == norm_target


def run_benchmark(
    root: Path,
    ashift_bin: Path,
    pandoc_bin: Path,
    edges_filter: list[str] | None = None,
    fixtures_filter: list[str] | None = None,
    repeat_count: int = 3,
    out_file: Path | None = None,
) -> dict[str, Any]:
    """Execute complete benchmark suite and return populated capabilities dictionary."""
    if repeat_count < 1:
        raise ValueError(f"repeat_count must be at least 1, got {repeat_count}")

    is_filtered = bool(edges_filter or fixtures_filter)
    committed_path = (root / "crates" / "ariad-core" / "data" / "capabilities.json").resolve()
    target_out = (out_file if out_file is not None else committed_path).resolve()

    if is_filtered and target_out == committed_path:
        raise ValueError(
            "Partial run with --edges or --fixtures cannot overwrite committed capabilities.json. "
            "Specify an explicit non-default --out path for partial runs."
        )

    capabilities_template_path = root / "crates" / "ariad-core" / "data" / "capabilities.json"
    capabilities_data = json.loads(capabilities_template_path.read_text(encoding="utf-8"))

    # Reset all edge metrics in template to None so unmeasured edges are not populated
    for edge in capabilities_data.get("edges", []):
        edge["metrics"] = None

    manifest_path = root / "fixtures" / "manifest.toml"
    manifest_records = tomllib.loads(manifest_path.read_text(encoding="utf-8")).get("fixture", [])

    env = os.environ.copy()
    env["ASHIFT_PANDOC"] = str(pandoc_bin)

    scored_fixture_ids: set[str] = set()
    edge_failures: dict[str, list[tuple[str, str]]] = {}
    all_miss_edges: list[str] = []
    unscored_fixtures: list[tuple[str, str]] = []
    total_tables_skipped = 0

    with tempfile.TemporaryDirectory(prefix="ashift-bench-") as tmp_dir:
        tmp_path = Path(tmp_dir)

        for edge in capabilities_data.get("edges", []):
            from_fmt = edge.get("from", "")
            to_fmt = edge.get("to", "")
            engine = edge.get("engine", "")
            edge_key = f"{from_fmt}->{to_fmt}"

            if edges_filter and edge_key not in edges_filter:
                continue

            print(f"Benchmarking edge: {from_fmt} -> {to_fmt} ({engine})...")
            edge_scores: list[SingleFixtureScore] = []

            # -------------------------------------------------------------
            # Case A: Reader Edge (from_fmt -> ariad-ir+json)
            # -------------------------------------------------------------
            if to_fmt == "ariad-ir+json":
                candidate_fixtures = [
                    f for f in manifest_records
                    if match_format(f.get("format", ""), from_fmt)
                ]
                if fixtures_filter:
                    candidate_fixtures = [f for f in candidate_fixtures if f["id"] in fixtures_filter]

                for f in candidate_fixtures:
                    fid = f["id"]
                    safe_fid = re.sub(r"[^a-zA-Z0-9_-]", "_", fid)
                    ref_doc = get_reference_for_fixture(f, root, pandoc_bin=pandoc_bin)
                    if ref_doc is None:
                        has_unscored_companion = False
                        unscored_reason = ""
                        for comp in f.get("companions", []):
                            if comp.get("path", "").endswith(".truth.json"):
                                p = root / comp.get("path", "")
                                if p.is_file():
                                    data = json.loads(p.read_text(encoding="utf-8"))
                                    if data.get("unscored") is True:
                                        has_unscored_companion = True
                                        unscored_reason = data.get("unscored_reason", "unscored: true")
                                        break
                        if has_unscored_companion:
                            unscored_fixtures.append((fid, f"Truth companion marked unscored ({unscored_reason})"))
                        else:
                            unscored_fixtures.append((fid, "External fixture without authored truth companion"))
                        continue

                    fixture_input = root / f["path"]
                    tmp_ir = tmp_path / f"{safe_fid}.ir.json"

                    wall_times: list[float] = []
                    peak_rss_list: list[int | None] = []

                    failed_fixture = False
                    failure_err = ""
                    for _ in range(repeat_count):
                        cmd = [str(ashift_bin), "__ir", str(fixture_input), "-o", str(tmp_ir), "--overwrite"]
                        rc, stdout, stderr, wall_ms, peak_rss = measure_execution(cmd, env=env)
                        wall_times.append(wall_ms)
                        peak_rss_list.append(peak_rss)
                        if rc != 0:
                            failure_err = stderr.strip() or f"Reader process failed with exit code {rc}"
                            print(f"  [fail] {fid} reader failed: {failure_err}")
                            failed_fixture = True
                            break

                    valid_rss = [r for r in peak_rss_list if r is not None]
                    max_rss: int | None = max(valid_rss) if valid_rss else None

                    if failed_fixture:
                        edge_failures.setdefault(edge_key, []).append((fid, failure_err))
                        score = SingleFixtureScore(
                            text_cer=1.0,
                            heading_ted=1.0,
                            teds=0.0,
                            fidelity=0.0,
                            editability=0.0,
                            wall_ms=sorted(wall_times)[len(wall_times) // 2] if wall_times else 0.0,
                            peak_rss_bytes=max_rss,
                            tables_skipped=0,
                        )
                        edge_scores.append(score)
                        scored_fixture_ids.add(fid)
                        continue

                    try:
                        hyp_doc = normalize(tmp_ir)
                    except Exception as exc:
                        failure_err = f"Hypothesis normalization failed: {exc}"
                        print(f"  [fail] {fid} normalize failed: {failure_err}")
                        edge_failures.setdefault(edge_key, []).append((fid, failure_err))
                        score = SingleFixtureScore(
                            text_cer=1.0,
                            heading_ted=1.0,
                            teds=0.0,
                            fidelity=0.0,
                            editability=0.0,
                            wall_ms=sorted(wall_times)[len(wall_times) // 2],
                            peak_rss_bytes=max_rss,
                            tables_skipped=0,
                        )
                        edge_scores.append(score)
                        scored_fixture_ids.add(fid)
                        continue

                    median_wall = sorted(wall_times)[len(wall_times) // 2]

                    score = score_fixture(ref_doc, hyp_doc, wall_ms=median_wall, peak_rss_bytes=max_rss)
                    edge_scores.append(score)
                    scored_fixture_ids.add(fid)
                    total_tables_skipped += score.tables_skipped

            # -------------------------------------------------------------
            # Case B: Writer Edge (ariad-ir+json -> to_fmt)
            # -------------------------------------------------------------
            elif from_fmt == "ariad-ir+json":
                candidate_fixtures = []
                target_route_suffix = "->md" if to_fmt == "markdown" else f"->{to_fmt}"
                for f in manifest_records:
                    routes = f.get("routes", [])
                    if any(r.endswith(target_route_suffix) for r in routes):
                        candidate_fixtures.append(f)

                if fixtures_filter:
                    candidate_fixtures = [f for f in candidate_fixtures if f["id"] in fixtures_filter]

                if not candidate_fixtures:
                    print(f"  [skip] No candidate fixtures found for writer edge {edge_key}")
                    continue

                for f in candidate_fixtures:
                    fid = f["id"]
                    safe_fid = re.sub(r"[^a-zA-Z0-9_-]", "_", fid)
                    input_ir_path = tmp_path / f"{safe_fid}.ir.json"

                    # 1. Produce input IR
                    truth_companion = None
                    for comp in f.get("companions", []):
                        if comp.get("path", "").endswith(".truth.json"):
                            p = root / comp.get("path", "")
                            if p.is_file():
                                truth_companion = p
                                break

                    if truth_companion is not None:
                        truth_data = json.loads(truth_companion.read_text(encoding="utf-8"))
                        if truth_data.get("unscored") is True:
                            unscored_reason = truth_data.get("unscored_reason", "unscored: true")
                            unscored_fixtures.append((fid, f"Truth companion marked unscored ({unscored_reason})"))
                            continue
                        if "ir" in truth_data:
                            input_ir_path.write_text(json.dumps(truth_data["ir"], indent=2), encoding="utf-8")
                        else:
                            unscored_fixtures.append((fid, "Truth companion missing ir"))
                            continue
                    elif match_format(f.get("format", ""), "markdown"):
                        cmd = [str(ashift_bin), "__ir", str(root / f["path"]), "-o", str(input_ir_path), "--overwrite"]
                        rc, _, stderr, wall_ms, peak_rss = measure_execution(cmd, env=env)
                        if rc != 0:
                            failure_err = f"Input IR generation failed: {stderr.strip() or f'Exit code {rc}'}"
                            print(f"  [fail] {fid} {failure_err}")
                            edge_failures.setdefault(edge_key, []).append((fid, failure_err))
                            score = SingleFixtureScore(
                                text_cer=1.0,
                                heading_ted=1.0,
                                teds=0.0,
                                fidelity=0.0,
                                editability=0.0,
                                wall_ms=wall_ms,
                                peak_rss_bytes=peak_rss,
                                tables_skipped=0,
                            )
                            edge_scores.append(score)
                            scored_fixture_ids.add(fid)
                            continue
                    else:
                        unscored_fixtures.append((fid, "Non-markdown format without authored truth companion"))
                        continue

                    try:
                        ref_doc = normalize(input_ir_path)
                    except Exception as exc:
                        failure_err = f"Reference IR normalization failed: {exc}"
                        print(f"  [fail] {fid} {failure_err}")
                        edge_failures.setdefault(edge_key, []).append((fid, failure_err))
                        score = SingleFixtureScore(
                            text_cer=1.0,
                            heading_ted=1.0,
                            teds=0.0,
                            fidelity=0.0,
                            editability=0.0,
                            wall_ms=0.0,
                            peak_rss_bytes=0,
                            tables_skipped=0,
                        )
                        edge_scores.append(score)
                        scored_fixture_ids.add(fid)
                        continue

                    # 2. Write IR to target format
                    ext_map = {"markdown": "md", "html": "html", "docx": "docx", "epub": "epub"}
                    ext = ext_map.get(to_fmt, to_fmt)
                    tmp_out = tmp_path / f"{safe_fid}.{ext}"

                    wall_times: list[float] = []
                    peak_rss_list: list[int | None] = []
                    write_failed = False
                    write_err = ""

                    for _ in range(repeat_count):
                        cmd = [
                            str(ashift_bin),
                            "__write",
                            str(input_ir_path),
                            "--to",
                            to_fmt,
                            "-o",
                            str(tmp_out),
                            "--overwrite",
                        ]
                        rc, stdout, stderr, wall_ms, peak_rss = measure_execution(cmd, env=env)
                        wall_times.append(wall_ms)
                        peak_rss_list.append(peak_rss)
                        if rc != 0:
                            write_err = stderr.strip() or f"Writer process failed with exit code {rc}"
                            print(f"  [fail] {fid} write failed: {write_err}")
                            write_failed = True
                            break

                    valid_rss = [r for r in peak_rss_list if r is not None]
                    max_rss: int | None = max(valid_rss) if valid_rss else None

                    if write_failed:
                        edge_failures.setdefault(edge_key, []).append((fid, write_err))
                        score = SingleFixtureScore(
                            text_cer=1.0,
                            heading_ted=1.0,
                            teds=0.0,
                            fidelity=0.0,
                            editability=0.0,
                            wall_ms=sorted(wall_times)[len(wall_times) // 2] if wall_times else 0.0,
                            peak_rss_bytes=max_rss,
                            tables_skipped=0,
                        )
                        edge_scores.append(score)
                        scored_fixture_ids.add(fid)
                        continue

                    # 3. Read back to IR
                    tmp_readback = tmp_path / f"{safe_fid}.readback.ir.json"
                    cmd_readback = [
                        str(ashift_bin),
                        "__ir",
                        str(tmp_out),
                        "-o",
                        str(tmp_readback),
                        "--overwrite",
                    ]
                    rc, stdout, stderr, rb_wall, rb_rss = measure_execution(cmd_readback, env=env)
                    if rc != 0:
                        rb_err = stderr.strip() or f"Readback process failed with exit code {rc}"
                        print(f"  [fail] {fid} readback failed: {rb_err}")
                        edge_failures.setdefault(edge_key, []).append((fid, rb_err))
                        score = SingleFixtureScore(
                            text_cer=1.0,
                            heading_ted=1.0,
                            teds=0.0,
                            fidelity=0.0,
                            editability=0.0,
                            wall_ms=sorted(wall_times)[len(wall_times) // 2],
                            peak_rss_bytes=max_rss,
                            tables_skipped=0,
                        )
                        edge_scores.append(score)
                        scored_fixture_ids.add(fid)
                        continue

                    try:
                        hyp_doc = normalize(tmp_readback)
                    except Exception as exc:
                        failure_err = f"Readback normalization failed: {exc}"
                        print(f"  [fail] {fid} {failure_err}")
                        edge_failures.setdefault(edge_key, []).append((fid, failure_err))
                        score = SingleFixtureScore(
                            text_cer=1.0,
                            heading_ted=1.0,
                            teds=0.0,
                            fidelity=0.0,
                            editability=0.0,
                            wall_ms=sorted(wall_times)[len(wall_times) // 2],
                            peak_rss_bytes=max_rss,
                            tables_skipped=0,
                        )
                        edge_scores.append(score)
                        scored_fixture_ids.add(fid)
                        continue

                    median_wall = sorted(wall_times)[len(wall_times) // 2]

                    score = score_fixture(ref_doc, hyp_doc, wall_ms=median_wall, peak_rss_bytes=max_rss)
                    edge_scores.append(score)
                    scored_fixture_ids.add(fid)
                    total_tables_skipped += score.tables_skipped

            # Summarize metrics for this edge
            if edge_scores:
                summary = aggregate_edge_metrics(edge_scores)
                if summary.peak_mem_mb is None:
                    failure_msg = "All memory samples missed for edge; cannot record valid peak_mem_mb"
                    edge_failures.setdefault(edge_key, []).append(("all-fixtures", failure_msg))
                    all_miss_edges.append(edge_key)
                    edge["metrics"] = None
                    print(f"  [fail] Edge {edge_key} memory sampling failed: {failure_msg}")
                else:
                    edge["metrics"] = summary.to_dict()
                    print(
                        f"  -> fidelity={summary.fidelity:.3f}, editability={summary.editability:.3f}, "
                        f"p50={summary.p50_ms:.0f}ms, peak_mem={summary.peak_mem_mb:.0f}MB, samples={summary.samples}"
                    )
                    if summary.memory_misses > 0:
                        print(f"  [warn] {summary.memory_misses} execution(s) finished before memory could be sampled")
            else:
                edge["metrics"] = None
                print("  -> unmeasured (0 samples)")

    # Print overall benchmark summary
    print("\nBenchmark Run Summary:")
    print(f"  Scored fixtures: {len(scored_fixture_ids)}")
    print(f"  Total tables skipped (cells > 500): {total_tables_skipped}")
    if unscored_fixtures:
        unique_unscored = sorted(set(unscored_fixtures))
        print(f"  Unscored fixtures ({len(unique_unscored)}):")
        for ufid, ureason in unique_unscored:
            print(f"    - {ufid}: {ureason}")
    if edge_failures:
        print("  Failures by edge:")
        for ekey, fails in edge_failures.items():
            print(f"    - Edge {ekey} ({len(fails)} failure(s)):")
            for ffid, ferr in fails:
                print(f"        * {ffid}: {ferr}")
    else:
        print("  Failures: None\n")

    # Update metadata
    ashift_ver = get_binary_version(ashift_bin)
    pandoc_ver = get_binary_version(pandoc_bin)

    capabilities_data["version"] = "ariad-capabilities/0"
    capabilities_data["generated_at"] = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    capabilities_data["bench"] = {
        "fixture_count": len(scored_fixture_ids),
        "pandoc_version": pandoc_ver,
        "ashift_version": ashift_ver,
    }

    if out_file:
        out_file.parent.mkdir(parents=True, exist_ok=True)
        out_file.write_text(json.dumps(capabilities_data, indent=2) + "\n", encoding="utf-8")
        print(f"Wrote measured capabilities to {out_file}")

    if all_miss_edges:
        raise RuntimeError(
            f"All memory samples missed for edge(s): {', '.join(all_miss_edges)}"
        )

    return capabilities_data


def build_parser(parser: argparse.ArgumentParser | None = None) -> argparse.ArgumentParser:
    """Configure CLI parser for the run subcommand."""
    if parser is None:
        parser = argparse.ArgumentParser(description="Run AriadShift benchmark harness")
    parser.add_argument("--ashift", type=Path, default=None, help="Path to ashift CLI binary")
    parser.add_argument("--pandoc", type=Path, default=None, help="Path to pandoc binary")
    parser.add_argument("--edges", type=str, nargs="*", default=None, help="Edges to benchmark, e.g. html->ariad-ir+json")
    parser.add_argument("--fixtures", type=str, nargs="*", default=None, help="Fixture IDs to benchmark")
    parser.add_argument("--repeat", type=int, default=3, help="Repeat count for latency measurements")
    parser.add_argument(
        "--out",
        type=Path,
        default=Path("crates/ariad-core/data/capabilities.json"),
        help="Path to write capabilities output JSON",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    if args.repeat < 1:
        parser.error("--repeat must be at least 1")

    root = Path(__file__).resolve().parents[3]
    ashift_bin = find_binary(
        "ashift",
        args.ashift,
        [root / "target" / "release" / "ashift", root / "target" / "debug" / "ashift"],
    )
    pandoc_bin = find_binary(
        "pandoc",
        args.pandoc,
        [root / ".tools" / "pandoc" / "bin" / "pandoc"],
    )

    edges_list = None
    if args.edges:
        edges_list = []
        for e in args.edges:
            edges_list.extend(e.split(","))

    fixtures_list = None
    if args.fixtures:
        fixtures_list = []
        for f in args.fixtures:
            fixtures_list.extend(f.split(","))

    try:
        run_benchmark(
            root=root,
            ashift_bin=ashift_bin,
            pandoc_bin=pandoc_bin,
            edges_filter=edges_list,
            fixtures_filter=fixtures_list,
            repeat_count=args.repeat,
            out_file=args.out,
        )
    except (ValueError, RuntimeError) as exc:
        print(f"Error: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
