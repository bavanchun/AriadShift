"""Original arithmetic fixtures and separately opted-in real Docker integrations."""

from copy import deepcopy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
import time

import psutil
import pytest

from ariad_bench.report import main, render

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location(
    "competitors", ROOT / "bench/competitors/__init__.py",
    submodule_search_locations=[str(ROOT / "bench/competitors")],
)
package = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = package
spec.loader.exec_module(package)
from competitors import report, run  # noqa: E402


def digest(value):
    return hashlib.sha256(value.encode()).hexdigest()


def original_evidence():
    """Hand-authored numbers for report arithmetic only, never benchmark evidence."""
    evidence = {
        "schema": report.SCHEMA, "kind": "corpus", "repeat": 3,
        "generated_at": "2000-01-01T00:00:00Z", "routes": list(report.ROUTES),
        "ashift_sha256": digest("original test binary"),
        "config_sha256": digest("original test config"),
        "image": {"id": "sha256:" + digest("original test image"), "reference": run.load_config(ROOT / "bench/competitors/stirling.toml")["image"]},
        "environment": {"purpose": "original arithmetic test only"},
        "cleanup": "completed",
        "fixtures": [{"id": "original", "sources": {
            fmt: {"path": f"original.{fmt}", "sha256": digest(f"original {fmt} bytes"),
                  "truth": "original.truth.json", "truth_sha256": digest("independent original truth")}
            for fmt in ("pdf", "docx")
        }}], "samples": [],
    }
    for route in report.ROUTES:
        for product in report.PRODUCTS:
            for attempt in range(1, 4):
                source = evidence["fixtures"][0]["sources"][route.split("->")[0]]
                evidence["samples"].append({
                    "fixture": "original", "route": route, "product": product, "attempt": attempt,
                    "sha256": source["sha256"], "truth_sha256": source["truth_sha256"],
                    "output_sha256": digest(f"original {product} {route} {attempt}"),
                    "status": "ok", "extraction_ms": 1.0, "conversion_ms": 20.0, "evaluation_ms": 4.0,
                    "score": {"metric_version": "original-report-test/2", "text_cer": 0.02 if product == "ariadshift" else 0.03,
                              "table_f1": 0.9 if product == "ariadshift" else 0.8,
                              "style_accuracy": 0.88 if product == "ariadshift" else 0.8, "layout_iou": 0.5,
                              "coverage": {name: {"matched": 3, "reference": 4} for name in report.COMPONENTS}, "unavailable_reasons": []},
                })
    return evidence


@pytest.mark.parametrize("before,after", [
    ("3.3.0@sha256:", "latest@sha256:"),
    ("3.3.0@sha256:", "3.3.0-rc.1@sha256:"),
    ("@sha256:", "@md5:"),
    ("127.0.0.1:18711", "0.0.0.0:18711"),
    ("127.0.0.1:18711", "example.org:18711"),
    ("repeat = 3", "repeat = 0"),
    ("total_timeout_s = 570", "total_timeout_s = 600"),
    ("/api/v1/convert/pdf/word", "https://example.org/convert"),
    ('outputFormat = "docx"', 'outputFormat = "doc"'),
])
def test_competitor_config_requires_stable_tag_and_immutable_digest(tmp_path, before, after):
    original = ROOT / "bench/competitors/stirling.toml"
    assert run.load_config(original)["repeat"] == 3
    path = tmp_path / "invalid.toml"
    path.write_text(original.read_text().replace(before, after))
    with pytest.raises(ValueError):
        run.load_config(path)


def test_competitor_evidence_requires_both_routes_and_source_hashes():
    evidence = original_evidence()
    report.validate_evidence(evidence)
    missing = deepcopy(evidence)
    missing["samples"].pop()
    with pytest.raises(ValueError, match="Missing paired attempts"):
        report.validate_evidence(missing)
    changed = deepcopy(evidence)
    changed["samples"][0]["sha256"] = digest("different source bytes")
    with pytest.raises(ValueError, match="exact same source"):
        report.validate_evidence(changed)
    evidence["routes"] = ["pdf->docx"]
    with pytest.raises(ValueError, match="Both document routes"):
        report.validate_evidence(evidence)


def test_competitor_report_obeys_cer_direction_and_style_gate():
    evidence = original_evidence()
    result = report.compare(evidence, "pdf->docx")
    assert result["status"] == "pass"
    assert result["style_delta_pp"] == pytest.approx(8)
    assert result["style_relative_percent"] == pytest.approx(10)
    for sample in evidence["samples"]:
        if sample["product"] == "ariadshift":
            sample["score"].update(text_cer=1.2, style_accuracy=0.87)
    report.validate_evidence(evidence)  # Raw CER above one is valid.
    result = report.compare(evidence, "pdf->docx")
    assert result["gates"] == {"text_cer": False, "table_f1": True, "style_accuracy": False}
    assert result["status"] == "fail"
    for sample in evidence["samples"]:
        sample["score"]["style_accuracy"] = 0
    result = report.compare(evidence, "pdf->docx")
    assert result["style_relative_percent"] is None
    assert result["style_delta_pp"] == 0
    assert result["gates"]["style_accuracy"] is True
    assert "zero/zero tie" in report.render(evidence)


def test_competitor_report_preserves_failures_and_missing_coverage():
    evidence = original_evidence()
    failed = evidence["samples"][0]
    failed.update(status="unsupported", reason="optional backend unavailable")
    failed.pop("score")
    missing = evidence["samples"][1]["score"]
    missing.update(layout_iou=None, unavailable_reasons=["reference layout missing"])
    text = report.render(evidence)
    assert "unsupported: 1 attempts" in text
    assert "optional backend unavailable" in text
    assert "reference layout missing" in text
    assert "0.0200 (2/3)" in text
    assert "incomplete" in text
    assert report.compare(evidence, "pdf->docx")["status"] == "incomplete"
    evidence["kind"] = "preflight"
    with pytest.raises(ValueError, match="Only versioned corpus"):
        report.render(evidence)


def test_competitor_report_is_reproducible_with_base_sections(tmp_path):
    capabilities = json.loads((ROOT / "crates/ariad-core/data/capabilities.json").read_text())
    heavy = json.loads((ROOT / "bench/results/heavy-routes.json").read_text())
    base = render(capabilities, heavy, None)
    evidence = original_evidence()
    combined = render(capabilities, heavy, evidence)
    assert combined.startswith(base)
    assert combined == render(capabilities, heavy, evidence)
    assert combined.count("## Local Stirling comparison") == 1
    path = tmp_path / "results.json"
    output = tmp_path / "report.md"
    path.write_text(json.dumps(evidence))
    args = ["--competitors", str(path), "--out", str(output)]
    assert main(args) == 0
    assert output.read_text() == combined
    assert main([*args, "--check"]) == 0
    output.write_text(combined + "stale")
    with pytest.raises(SystemExit) as failure:
        main([*args, "--check"])
    assert failure.value.code == 1
    with pytest.raises(ValueError, match="different runs"):
        render(capabilities, {**heavy, "generated_at": "different"}, evidence)


def test_competitor_timeout_cleans_owned_resources(tmp_path):
    """A real child and grandchild exit; an unrelated real process survives."""
    import subprocess

    pid_file = tmp_path / "grandchild.pid"
    script = (
        "import subprocess,sys,time; from pathlib import Path; "
        "p=subprocess.Popen([sys.executable,'-c','import signal,time; signal.signal(signal.SIGTERM,signal.SIG_IGN); time.sleep(60)']); "
        "Path(sys.argv[1]).write_text(str(p.pid)); time.sleep(60)"
    )
    unrelated = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"])
    try:
        with pytest.raises(run.HarnessError, match="Timed out"):
            run.Runner(5).run([sys.executable, "-c", script, str(pid_file)], timeout=0.5)
        grandchild = int(pid_file.read_text())
        deadline = time.monotonic() + 2
        while psutil.pid_exists(grandchild) and psutil.Process(grandchild).status() != psutil.STATUS_ZOMBIE and time.monotonic() < deadline:
            time.sleep(0.01)
        assert not psutil.pid_exists(grandchild) or psutil.Process(grandchild).status() == psutil.STATUS_ZOMBIE
        assert unrelated.poll() is None
    finally:
        unrelated.terminate()
        unrelated.wait(timeout=3)


def test_competitor_rejects_invalid_output_and_unavailable_scorer(tmp_path):
    path = tmp_path / "invalid.pdf"
    path.write_bytes(b"<html>not a converted PDF</html>")
    with pytest.raises(run.HarnessError, match="did not return a PDF"):
        run.validate_artifact(path, "pdf")
    with pytest.raises(run.HarnessError, match="unavailable"):
        run.load_scorer("ariad_bench.no_such_scorer:score")


def test_competitor_signal_interrupts_attempt_and_releases_owned_scope(tmp_path):
    """Interrupt an actual sleeping process, without simulating engine success."""
    sleeper = tmp_path / "sleeping-process"
    sleeper.write_text("#!/bin/sh\nsleep 60\n")
    sleeper.chmod(0o700)
    cleaned = tmp_path / "cleaned"
    with pytest.raises(BaseException, match="Interrupted by signal"):
        try:
            with run.interrupt_cleanup(0.1):
                run.paired_attempt(run.Runner(5), None, sleeper, ROOT / "fixtures/heavy/en-map.pdf", "docx", tmp_path / "output.docx")
        finally:
            cleaned.touch()
    assert cleaned.exists()


def test_competitor_cannot_claim_pass_when_other_route_is_incomplete():
    evidence = original_evidence()
    evidence["status"] = "pass"
    row = next(s for s in evidence["samples"] if s["route"] == "docx->pdf")
    row.update(status="unavailable", reason="read-back unavailable")
    row.pop("score")
    with pytest.raises(ValueError, match="acceptance"):
        report.validate_evidence(evidence)


def test_competitor_conversion_timing_survives_evaluation_failure():
    evidence = original_evidence()
    for sample in evidence["samples"][1:3]:
        sample.update(status="unavailable", reason="read-back unavailable", conversion_ms=1000)
        sample.pop("score")
    text = report.render(evidence)
    assert "| 1000 | incomplete |" in text


def test_competitor_reused_output_keeps_attempts_in_separate_runs(tmp_path):
    first = run.artifact_directory(tmp_path)
    run.write_json(first / "attempt-0001.json", {"original": "first"})
    second = run.artifact_directory(tmp_path)
    run.write_json(second / "attempt-0001.json", {"original": "second"})
    assert first != second
    assert json.loads((first / "attempt-0001.json").read_text()) == {"original": "first"}
    assert json.loads((second / "attempt-0001.json").read_text()) == {"original": "second"}
    assert len(list(second.iterdir())) == 1


def test_competitor_acceptance_requires_cleanup_and_docker_image_hash():
    evidence = original_evidence()
    evidence["cleanup"] = "failed"
    with pytest.raises(ValueError, match="cleanup"):
        report.validate_evidence(evidence)
    evidence["cleanup"] = "completed"
    evidence["image"]["id"] = "not-a-docker-image-hash"
    with pytest.raises(ValueError, match="image"):
        report.validate_evidence(evidence)


def test_competitor_public_evidence_redacts_paths_without_changing_measurements():
    binary = Path.home() / "original-test-bin/ashift"
    original = {"worktree": str(ROOT), "command": [str(binary), str(ROOT / "fixtures/heavy/en-map.pdf")],
                "sha256": digest("original bytes"), "conversion_ms": 12.25, "status": "converted"}
    public = run.public_evidence(original, binary)
    assert str(Path.home()) not in json.dumps(public)
    assert public["worktree"] == "${WORKTREE}"
    assert public["command"] == ["${ASHIFT_BIN}", "${WORKTREE}/fixtures/heavy/en-map.pdf"]
    assert (public["sha256"], public["conversion_ms"], public["status"]) == (original["sha256"], 12.25, "converted")
    assert original["worktree"] == str(ROOT)


@pytest.mark.skipif(os.environ.get("ARIAD_BENCH_REAL_COMPETITORS") != "1", reason="Real Docker integration is a separate required gate; run with ARIAD_BENCH_REAL_COMPETITORS=1")
def test_stirling_real_local_pdf_docx_roundtrips(tmp_path):
    config = run.load_config(ROOT / "bench/competitors/stirling.toml")
    binary = Path(os.environ.get("ASHIFT_BIN", str(Path.home() / ".cache/ariadshift-target/debug/ashift")))
    runner = run.Runner(config["total_timeout_s"])
    with run.interrupt_cleanup(config["total_timeout_s"]), tempfile.TemporaryDirectory(dir=tmp_path) as temporary:
        with run.LocalStirling(config, runner) as service:
            samples = run.probe_routes(runner, service, binary, Path(temporary))
            image = dict(service.metadata)
    run.write_json(tmp_path / "integration.json", run.public_evidence({
        "schema": report.SCHEMA, "kind": "integration", "samples": samples,
        "image": image, "cleanup": "completed", "commands": runner.commands,
        "ashift_sha256": run.sha256(binary), "config_sha256": run.sha256(ROOT / "bench/competitors/stirling.toml"),
    }, binary))
    assert len(samples) == 4
    failures = [s for s in samples if s["status"] != "converted"]
    assert not failures, json.dumps(failures, indent=2)
    commands = [entry["command"][:2] for entry in runner.commands]
    assert ["docker", "stop"] in commands
    assert ["docker", "rm"] in commands
    assert any(entry["command"][:3] == ["docker", "network", "rm"] for entry in runner.commands)
