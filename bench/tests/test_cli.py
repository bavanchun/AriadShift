"""Tests for CLI subcommands: check, diff, and run argument validation."""

from __future__ import annotations

import json
from pathlib import Path
import pytest

from ariad_bench.check import validate_capabilities, main as check_main
from ariad_bench.diff import format_delta, generate_diff_table, main as diff_main
from ariad_bench.run import find_binary, run_benchmark


def test_format_delta_threshold_and_signs() -> None:
    # Baseline None
    assert format_delta(0.95, None, 3) == "0.950"

    # Delta below half-LSB threshold avoids -0.000
    assert format_delta(0.9501, 0.9503, 3) == "0.950"

    # Positive delta has +
    assert format_delta(0.96, 0.95, 3) == "0.960 (+0.010)"

    # Negative delta
    assert format_delta(0.94, 0.95, 3) == "0.940 (-0.010)"

    # Suffix test
    assert format_delta(50.0, 40.0, 0, " ms") == "50 ms (+10 ms)"


def test_generate_diff_table() -> None:
    current = {
        "edges": [
            {
                "from": "html",
                "to": "ariad-ir+json",
                "engine": "ariad-reader-html",
                "metrics": {
                    "fidelity": 0.985,
                    "editability": 0.950,
                    "p50_ms": 20.0,
                    "peak_mem_mb": 12.0,
                    "samples": 3,
                },
            }
        ]
    }
    baseline = {
        "edges": [
            {
                "from": "html",
                "to": "ariad-ir+json",
                "engine": "ariad-reader-html",
                "metrics": {
                    "fidelity": 0.980,
                    "editability": 0.950,
                    "p50_ms": 30.0,
                    "peak_mem_mb": 15.0,
                    "samples": 3,
                },
            }
        ]
    }

    table = generate_diff_table(current, baseline)
    assert "| `html → ariad-ir+json` | ariad-reader-html |" in table
    assert "0.985 (+0.005)" in table
    assert "20 ms (-10 ms)" in table


def test_check_capabilities_valid() -> None:
    root = Path(__file__).resolve().parents[2]
    schema_path = root / "schemas" / "capabilities.v0.json"
    cap_path = root / "crates" / "ariad-core" / "data" / "capabilities.json"

    assert schema_path.is_file()
    assert cap_path.is_file()

    cap_data = json.loads(cap_path.read_text(encoding="utf-8"))
    schema_data = json.loads(schema_path.read_text(encoding="utf-8"))

    errors = validate_capabilities(cap_data, schema_data)
    assert errors == []
    assert check_main(["--file", str(cap_path), "--schema", str(schema_path)]) == 0


def test_check_capabilities_invalid_schema() -> None:
    root = Path(__file__).resolve().parents[2]
    schema_path = root / "schemas" / "capabilities.v0.json"
    schema_data = json.loads(schema_path.read_text(encoding="utf-8"))

    errors = validate_capabilities({"invalid": "data"}, schema_data)
    assert len(errors) > 0


def test_diff_missing_baseline(tmp_path: Path) -> None:
    cap = tmp_path / "cap.json"
    cap.write_text(json.dumps({"edges": []}), encoding="utf-8")

    rc = diff_main(["--new", str(cap), "--baseline", str(tmp_path / "nonexistent.json")])
    assert rc == 1


def test_run_validation(tmp_path: Path) -> None:
    with pytest.raises(ValueError, match="repeat_count must be at least 1"):
        run_benchmark(
            root=tmp_path,
            ashift_bin=tmp_path / "ashift",
            pandoc_bin=tmp_path / "pandoc",
            repeat_count=0,
        )

    with pytest.raises(FileNotFoundError):
        find_binary("nonexistent_binary_xyz", None, [tmp_path / "nowhere"])


def test_partial_run_guard(tmp_path: Path) -> None:
    data_dir = tmp_path / "crates" / "ariad-core" / "data"
    data_dir.mkdir(parents=True, exist_ok=True)
    cap_file = data_dir / "capabilities.json"
    cap_file.write_text(json.dumps({"edges": []}), encoding="utf-8")

    with pytest.raises(ValueError, match="cannot overwrite committed capabilities.json"):
        run_benchmark(
            root=tmp_path,
            ashift_bin=tmp_path / "ashift",
            pandoc_bin=tmp_path / "pandoc",
            edges_filter=["html->ariad-ir+json"],
            out_file=cap_file,
        )


def test_validate_capabilities_rejects_insufficient_samples() -> None:
    schema = {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": {
            "edges": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "from": {"type": "string"},
                        "to": {"type": "string"},
                        "engine": {"type": "string"},
                        "metrics": {
                            "type": ["object", "null"],
                            "properties": {
                                "samples": {"type": "integer"}
                            }
                        }
                    }
                }
            }
        }
    }
    cap_one_sample = {
        "edges": [
            {
                "from": "markdown",
                "to": "ariad-ir+json",
                "engine": "ariad-core",
                "metrics": {"samples": 1}
            }
        ]
    }
    errors = validate_capabilities(cap_one_sample, schema)
    assert any("minimum 2 required" in err for err in errors)

    cap_two_samples = {
        "edges": [
            {
                "from": "markdown",
                "to": "ariad-ir+json",
                "engine": "ariad-core",
                "metrics": {"samples": 2}
            }
        ]
    }
    errors2 = validate_capabilities(cap_two_samples, schema)
    assert not errors2


def test_unselected_edges_metrics_reset_to_none(tmp_path: Path) -> None:
    """Verify that unmeasured edges have their metrics reset to None."""
    data_dir = tmp_path / "crates" / "ariad-core" / "data"
    data_dir.mkdir(parents=True, exist_ok=True)
    cap_file = data_dir / "capabilities.json"
    cap_file.write_text(
        json.dumps({
            "edges": [
                {
                    "from": "html",
                    "to": "ariad-ir+json",
                    "engine": "ariad-reader-html",
                    "metrics": {"fidelity": 0.9, "samples": 3},
                },
                {
                    "from": "markdown",
                    "to": "ariad-ir+json",
                    "engine": "ariad-core",
                    "metrics": {"fidelity": 0.95, "samples": 5},
                },
            ]
        }),
        encoding="utf-8",
    )

    fixtures_dir = tmp_path / "fixtures"
    fixtures_dir.mkdir(parents=True, exist_ok=True)
    (fixtures_dir / "manifest.toml").write_text("", encoding="utf-8")

    out_file = tmp_path / "out_capabilities.json"
    # Filter to an edge that won't match any fixtures, so nothing runs
    result = run_benchmark(
        root=tmp_path,
        ashift_bin=tmp_path / "ashift",
        pandoc_bin=tmp_path / "pandoc",
        edges_filter=["docx->ariad-ir+json"],
        out_file=out_file,
    )

    # Both edges in the output should have metrics reset to None
    for edge in result["edges"]:
        assert edge["metrics"] is None


def test_failure_score_fidelity_and_editability_are_zero() -> None:
    """Verify that a failure score records zero fidelity and editability, not 1.0."""
    from ariad_bench.metrics import SingleFixtureScore

    failed_score = SingleFixtureScore(
        text_cer=1.0,
        heading_ted=1.0,
        teds=0.0,
        fidelity=0.0,
        editability=0.0,
        wall_ms=10.0,
        peak_rss_bytes=1000,
    )
    assert failed_score.fidelity == 0.0
    assert failed_score.editability == 0.0


def test_run_benchmark_with_failing_binary(tmp_path: Path) -> None:
    """Verify run_benchmark handles failing binary by recording 0.0 fidelity and editability at run level."""
    import os
    import sys

    if sys.platform == "win32":
        pytest.skip("Unix shell script binary mock")

    # Set up template capabilities
    data_dir = tmp_path / "crates" / "ariad-core" / "data"
    data_dir.mkdir(parents=True, exist_ok=True)
    cap_file = data_dir / "capabilities.json"
    cap_file.write_text(
        json.dumps({
            "version": "ariad-capabilities/0",
            "generated_at": "2026-10-06T00:00:00Z",
            "bench": {"ashift_version": "0.1.0", "fixture_count": 0, "pandoc_version": None},
            "edges": [
                {
                    "from": "markdown",
                    "to": "ariad-ir+json",
                    "engine": "ariad-core",
                    "runtime": ["local"],
                    "license": "Apache-2.0",
                    "metrics": None,
                }
            ],
        }),
        encoding="utf-8",
    )

    # Set up fixtures and manifest
    fixtures_dir = tmp_path / "fixtures"
    fixtures_dir.mkdir(parents=True, exist_ok=True)
    (fixtures_dir / "test.md").write_text("# Test\nContent", encoding="utf-8")
    (fixtures_dir / "test.truth.json").write_text(
        json.dumps({
            "version": "ariad-truth/0",
            "ir": {
                "version": "ariad-ir/0",
                "meta": {"title": "Test"},
                "body": [
                    {"type": "heading", "level": 1, "content": [{"type": "text", "text": "Test"}]},
                    {"type": "paragraph", "content": [{"type": "text", "text": "Content"}]},
                ],
            },
        }),
        encoding="utf-8",
    )

    manifest_toml = """
[[fixture]]
id = "test-doc"
format = "markdown"
path = "fixtures/test.md"
routes = ["md->ir"]
languages = ["en"]
tags = ["test"]
[[fixture.companions]]
kind = "scan-truth"
path = "fixtures/test.truth.json"
sha256 = "abc"
"""
    (fixtures_dir / "manifest.toml").write_text(manifest_toml, encoding="utf-8")

    # Create failing ashift script
    fake_ashift = tmp_path / "fake-ashift"
    fake_ashift.write_text("#!/bin/sh\nsleep 0.01\necho 'Fatal error' >&2\nexit 1\n", encoding="utf-8")
    os.chmod(fake_ashift, 0o755)

    out_file = tmp_path / "out_capabilities.json"
    result = run_benchmark(
        root=tmp_path,
        ashift_bin=fake_ashift,
        pandoc_bin=fake_ashift,
        edges_filter=["markdown->ariad-ir+json"],
        repeat_count=1,
        out_file=out_file,
    )

    metrics = result["edges"][0]["metrics"]
    assert metrics is not None
    assert metrics["fidelity"] == 0.0
    assert metrics["editability"] == 0.0
    assert metrics["samples"] == 1


def test_run_benchmark_per_fixture_median_latency(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    """Verify run_benchmark computes per-fixture median latency across repeat measurements."""
    import ariad_bench.run

    data_dir = tmp_path / "crates" / "ariad-core" / "data"
    data_dir.mkdir(parents=True, exist_ok=True)
    cap_file = data_dir / "capabilities.json"
    cap_file.write_text(
        json.dumps({
            "version": "ariad-capabilities/0",
            "generated_at": "2026-10-06T00:00:00Z",
            "bench": {"ashift_version": "0.1.0", "fixture_count": 0, "pandoc_version": None},
            "edges": [
                {
                    "from": "markdown",
                    "to": "ariad-ir+json",
                    "engine": "ariad-core",
                    "runtime": ["local"],
                    "license": "Apache-2.0",
                    "metrics": None,
                }
            ],
        }),
        encoding="utf-8",
    )

    fixtures_dir = tmp_path / "fixtures"
    fixtures_dir.mkdir(parents=True, exist_ok=True)
    (fixtures_dir / "test.md").write_text("# Test\nContent", encoding="utf-8")
    (fixtures_dir / "test.truth.json").write_text(
        json.dumps({
            "version": "ariad-truth/0",
            "ir": {
                "version": "ariad-ir/0",
                "meta": {"title": "Test"},
                "body": [
                    {"type": "heading", "level": 1, "content": [{"type": "text", "text": "Test"}]},
                    {"type": "paragraph", "content": [{"type": "text", "text": "Content"}]},
                ],
            },
        }),
        encoding="utf-8",
    )

    manifest_toml = """
[[fixture]]
id = "test-doc"
format = "markdown"
path = "fixtures/test.md"
routes = ["md->ir"]
languages = ["en"]
tags = ["test"]
[[fixture.companions]]
kind = "scan-truth"
path = "fixtures/test.truth.json"
sha256 = "abc"
"""
    (fixtures_dir / "manifest.toml").write_text(manifest_toml, encoding="utf-8")

    # Sequence of timings: 10 ms, 50 ms, 120 ms -> median is 50 ms (rounded to 50 ms)
    # If max were used, latency would be 120 ms
    timings = [10.0, 50.0, 120.0]
    call_idx = 0

    def mock_measure_execution(cmd, **kwargs):
        nonlocal call_idx
        wall = timings[call_idx % len(timings)]
        call_idx += 1
        # Write valid IR JSON output to the specified -o file
        if "-o" in cmd:
            out_idx = cmd.index("-o") + 1
            Path(cmd[out_idx]).write_text(
                json.dumps({
                    "version": "ariad-ir/0",
                    "meta": {"title": "Test"},
                    "body": [
                        {"type": "heading", "level": 1, "content": [{"type": "text", "text": "Test"}]},
                        {"type": "paragraph", "content": [{"type": "text", "text": "Content"}]},
                    ],
                }),
                encoding="utf-8",
            )
        return 0, "", "", wall, 10 * 1024 * 1024

    monkeypatch.setattr(ariad_bench.run, "measure_execution", mock_measure_execution)

    fake_ashift = tmp_path / "fake-ashift"
    fake_ashift.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")

    out_file = tmp_path / "out_capabilities.json"
    result = run_benchmark(
        root=tmp_path,
        ashift_bin=fake_ashift,
        pandoc_bin=fake_ashift,
        edges_filter=["markdown->ariad-ir+json"],
        repeat_count=3,
        out_file=out_file,
    )

    metrics = result["edges"][0]["metrics"]
    assert metrics is not None
    # Per-fixture median is 50.0 ms. If max were used, it would be 120.0 ms.
    assert metrics["p50_ms"] == 50.0


def test_validate_capabilities_rejects_null_peak_mem_mb() -> None:
    root = Path(__file__).resolve().parents[2]
    schema_path = root / "schemas" / "capabilities.v0.json"
    schema_data = json.loads(schema_path.read_text(encoding="utf-8"))

    invalid_cap = {
        "version": "ariad-capabilities/0",
        "generated_at": "2026-10-07T00:00:00Z",
        "bench": {"fixture_count": 2, "pandoc_version": "3.1", "ashift_version": "0.1.0"},
        "edges": [
            {
                "from": "html",
                "to": "ariad-ir+json",
                "engine": "ariad-reader-html",
                "metrics": {
                    "fidelity": 0.95,
                    "editability": 0.90,
                    "p50_ms": 10.0,
                    "peak_mem_mb": None,
                    "samples": 2,
                },
            }
        ],
    }

    errors = validate_capabilities(invalid_cap, schema_data)
    assert len(errors) > 0
    assert any("peak_mem_mb" in err for err in errors)


def test_run_benchmark_all_miss_edge_rejected(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    import ariad_bench.run

    cap_json = {
        "version": "ariad-capabilities/0",
        "edges": [
            {
                "from": "markdown",
                "to": "ariad-ir+json",
                "engine": "ariad-reader-markdown",
                "metrics": None,
            }
        ],
    }
    cap_dir = tmp_path / "crates" / "ariad-core" / "data"
    cap_dir.mkdir(parents=True)
    (cap_dir / "capabilities.json").write_text(json.dumps(cap_json), encoding="utf-8")

    fixtures_dir = tmp_path / "fixtures"
    fixtures_dir.mkdir()
    fixture_file = fixtures_dir / "test.md"
    fixture_file.write_text("# Test\n\nContent\n", encoding="utf-8")

    truth_file = fixtures_dir / "test.truth.json"
    truth_doc = {
        "canonical": {"text": "Test\nContent", "headings": [{"level": 1, "text": "Test", "children": []}], "lists": [], "tables": []},
        "ir": {
            "version": "ariad-ir/0",
            "meta": {"title": "Test"},
            "body": [
                {"type": "heading", "level": 1, "content": [{"type": "text", "text": "Test"}]},
                {"type": "paragraph", "content": [{"type": "text", "text": "Content"}]},
            ],
        },
    }
    truth_file.write_text(json.dumps(truth_doc), encoding="utf-8")

    manifest_toml = """
[[fixture]]
id = "test-doc"
format = "markdown"
path = "fixtures/test.md"
routes = ["md->ir"]
languages = ["en"]
tags = ["test"]
[[fixture.companions]]
kind = "scan-truth"
path = "fixtures/test.truth.json"
sha256 = "abc"
"""
    (fixtures_dir / "manifest.toml").write_text(manifest_toml, encoding="utf-8")

    def mock_measure_execution(cmd, **kwargs):
        if "-o" in cmd:
            out_idx = cmd.index("-o") + 1
            Path(cmd[out_idx]).write_text(
                json.dumps({
                    "version": "ariad-ir/0",
                    "meta": {"title": "Test"},
                    "body": [
                        {"type": "heading", "level": 1, "content": [{"type": "text", "text": "Test"}]},
                        {"type": "paragraph", "content": [{"type": "text", "text": "Content"}]},
                    ],
                }),
                encoding="utf-8",
            )
        # All samples miss memory
        return 0, "", "", 15.0, None

    monkeypatch.setattr(ariad_bench.run, "measure_execution", mock_measure_execution)

    fake_ashift = tmp_path / "fake-ashift"
    fake_ashift.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
    fake_ashift.chmod(0o755)

    out_file = tmp_path / "out_capabilities.json"
    with pytest.raises(RuntimeError, match="All memory samples missed for edge"):
        run_benchmark(
            root=tmp_path,
            ashift_bin=fake_ashift,
            pandoc_bin=fake_ashift,
            edges_filter=["markdown->ariad-ir+json"],
            repeat_count=2,
            out_file=out_file,
        )

    # Edge metrics must be None in the written file (valid schema, nothing invalid written)
    written_data = json.loads(out_file.read_text(encoding="utf-8"))
    assert written_data["edges"][0]["metrics"] is None

    # Check CLI main also exits non-zero (rc == 1) when run_benchmark encounters all-miss edge
    def mock_run_benchmark(**kwargs):
        raise RuntimeError("All memory samples missed for edge(s): markdown->ariad-ir+json")

    monkeypatch.setattr(ariad_bench.run, "run_benchmark", mock_run_benchmark)
    rc = ariad_bench.run.main([
        "--ashift", str(fake_ashift),
        "--pandoc", str(fake_ashift),
        "--edges", "markdown->ariad-ir+json",
        "--repeat", "2",
        "--out", str(out_file),
    ])
    assert rc == 1


def test_check_capabilities_fails_when_measured_edge_becomes_null(tmp_path: Path) -> None:
    schema = {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": {
            "edges": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "from": {"type": "string"},
                        "to": {"type": "string"},
                        "engine": {"type": "string"},
                        "metrics": {
                            "type": ["object", "null"],
                            "properties": {
                                "samples": {"type": "integer"},
                                "peak_mem_mb": {"type": ["number", "null"]},
                            },
                        },
                    },
                },
            },
        },
    }
    committed_cap = {
        "edges": [
            {
                "from": "markdown",
                "to": "ariad-ir+json",
                "engine": "ariad-core",
                "metrics": {"samples": 5, "peak_mem_mb": 8.0},
            }
        ]
    }
    # Current capabilities where the edge became null (e.g. from an all-miss run)
    null_cap = {
        "edges": [
            {
                "from": "markdown",
                "to": "ariad-ir+json",
                "engine": "ariad-core",
                "metrics": None,
            }
        ]
    }
    errors = validate_capabilities(null_cap, schema, committed_data=committed_cap)
    assert len(errors) == 1
    assert "has null metrics, but was measured in committed baseline" in errors[0]

    # CLI check_main invocation with --committed file
    cap_file = tmp_path / "capabilities.json"
    cap_file.write_text(json.dumps(null_cap), encoding="utf-8")
    committed_file = tmp_path / "committed.json"
    committed_file.write_text(json.dumps(committed_cap), encoding="utf-8")
    schema_file = tmp_path / "schema.json"
    schema_file.write_text(json.dumps(schema), encoding="utf-8")

    rc = check_main([
        "--file", str(cap_file),
        "--schema", str(schema_file),
        "--committed", str(committed_file),
    ])
    assert rc == 1

