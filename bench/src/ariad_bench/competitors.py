"""Run the local, pinned Stirling harness with the integrated fidelity scorer."""
from __future__ import annotations

import importlib.util
import os
from pathlib import Path
import sys

from ariad_bench.run import find_binary

ROOT = Path(__file__).resolve().parents[3]


def score(reference: dict, hypothesis: dict) -> dict:
    """Translate the accepted report shape without changing any metric or denominator."""
    from ariad_bench.fidelity import compare

    result = compare(reference, hypothesis)
    table = result.get("structure_by_kind", {}).get("table", {})
    result["table_f1"] = table.get("f1")
    coverage = result["coverage"]
    for target, source in (("text_cer", "text"), ("style_accuracy", "style"), ("layout_iou", "layout")):
        coverage[target] = dict(coverage.get(source, {"matched": 0, "reference": 0}))
    coverage["table_f1"] = {key: table.get(key, 0) for key in ("matched", "reference", "hypothesis")}
    if result["table_f1"] is None:
        result["unavailable_reasons"]["table"] = "no_table_observations"
    return result


def main(argv: list[str] | None = None) -> int:
    spec = importlib.util.spec_from_file_location(
        "ariad_local_competitors", ROOT / "bench/competitors/__init__.py",
        submodule_search_locations=[str(ROOT / "bench/competitors")],
    )
    package = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = package
    spec.loader.exec_module(package)
    from ariad_local_competitors.run import main as run

    binary = find_binary("ashift", os.environ.get("ASHIFT_BIN"), [
        ROOT / "target/release/ashift",
        Path.home() / ".cache/ariadshift-target/release/ashift",
    ])
    return run([
        "--config", str(ROOT / "bench/competitors/stirling.toml"),
        "--ashift", str(binary), "--out", str(ROOT / "bench/results/competitors"),
        "--result-name", "stirling-v3.3.0.json",
        *(sys.argv[1:] if argv is None else argv),
    ])


if __name__ == "__main__":
    raise SystemExit(main())
