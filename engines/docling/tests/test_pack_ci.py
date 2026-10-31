"""Pack CI omissions remain visible and cannot silently narrow a release."""

from __future__ import annotations

import hashlib
import importlib
import io
import json
import os
import shlex
import sys
import tarfile
import tomllib
from dataclasses import asdict

import pytest
from conftest import REPOSITORY

sys.path.insert(0, str(REPOSITORY / "engines/docling/pack"))

import build
import fetch
import native_build

LINUX = ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"]
MACOS = "aarch64-apple-darwin"


@pytest.fixture
def planner():
    return importlib.import_module("ci_plan")


def macos_pin():
    pins = fetch.load_pins()
    name = "opencv_python_headless-5.0.0.93-cp314-cp314-macosx_14_0_arm64.whl"
    release = pins["toolchain"]["release"]
    pins["toolchain"]["targets"] = {
        MACOS: {
            "opencv": {
                "build_key": native_build.cache_key("opencv", MACOS, pins),
                "files": [
                    {
                        "name": name,
                        "url": f"https://github.com/bavanchun/AriadShift/releases/download/{release}/{name}",
                        "sha256": "a" * 64,
                        "size": 123,
                    }
                ],
            }
        }
    }
    return pins


def test_unpinned_pull_request_keeps_both_linux_targets_and_explains_macos(planner):
    result = planner.plan(fetch.load_pins(), "pull_request")
    assert result["targets"] == LINUX
    assert result["skipped"] == {MACOS: planner.MISSING_MACOS_PIN}
    assert "pin missing" in result["skipped"][MACOS]


@pytest.mark.parametrize("event", ["push", "workflow_dispatch"])
def test_release_without_an_explicit_macos_omission_fails(planner, event):
    with pytest.raises(SystemExit, match=r"explicit.*omission"):
        planner.plan(fetch.load_pins(), event, "pack-docling-v0.2.0")


def test_cloud_ci_dispatch_explicitly_omits_only_macos_without_relaxing_release(planner):
    workflow = (REPOSITORY / ".github/workflows/cloud-e2e.yml").read_text(encoding="utf-8")
    commands = [
        shlex.split(line.strip())
        for line in workflow.replace("\\\n", " ").splitlines()
        if line.strip().startswith("gh workflow run pack-docling.yml ")
    ]
    assert len(commands) == 1
    command = commands[0]
    assert command[command.index("--ref") + 1] == "$PACK_REF"
    omissions = command[command.index("--field") + 1]
    assert omissions == f"allow_missing_targets={MACOS}"
    pins = fetch.load_pins()
    result = planner.plan(pins, "workflow_dispatch", allowed_skips=omissions.split("=", 1)[1])
    assert result["targets"] == LINUX
    assert result["allowed_skips"] == [MACOS]
    assert result["release"] is True
    with pytest.raises(SystemExit, match="release requires an explicit macOS omission"):
        planner.plan(pins, "workflow_dispatch")


def test_tag_omission_is_bound_to_the_exact_release_tag(planner):
    pins = fetch.load_pins()
    pins["pack_release_skips"] = {"pack-docling-v0.2.0": [MACOS]}
    assert planner.plan(pins, "push", "pack-docling-v0.2.0")["targets"] == LINUX
    with pytest.raises(SystemExit, match=r"explicit.*omission"):
        planner.plan(pins, "push", "pack-docling-v0.2.1")


def test_manual_release_can_explicitly_allow_only_the_missing_macos_target(planner):
    result = planner.plan(fetch.load_pins(), "workflow_dispatch", allowed_skips=MACOS)
    assert result["targets"] == LINUX
    assert result["allowed_skips"] == [MACOS]


@pytest.mark.parametrize("target", [*LINUX, "x86_64-pc-windows-msvc", "unknown"])
def test_release_omissions_never_drop_linux_or_accept_unknown_targets(planner, target):
    with pytest.raises(SystemExit, match=r"only.*aarch64-apple-darwin"):
        planner.plan(fetch.load_pins(), "workflow_dispatch", allowed_skips=target)


@pytest.mark.parametrize("event", ["pull_request", "push", "workflow_dispatch"])
def test_current_authoritative_pin_selects_macos_on_every_run(planner, event):
    result = planner.plan(macos_pin(), event, "pack-docling-v0.2.0")
    assert result["targets"] == [*LINUX, MACOS]
    assert not result["skipped"]


@pytest.mark.parametrize("field,value", [("build_key", "stale"), ("files", [])])
def test_invalid_configured_pin_is_an_error_instead_of_a_pr_skip(planner, field, value):
    pins = macos_pin()
    pins["toolchain"]["targets"][MACOS]["opencv"][field] = value
    with pytest.raises(SystemExit):
        planner.plan(pins, "pull_request")


def test_pr_skip_emits_warning_summary_and_filtered_matrix(planner, tmp_path, monkeypatch, capsys):
    output = tmp_path / "output"
    summary = tmp_path / "summary"
    monkeypatch.setenv("GITHUB_EVENT_NAME", "pull_request")
    monkeypatch.setenv("GITHUB_OUTPUT", str(output))
    monkeypatch.setenv("GITHUB_STEP_SUMMARY", str(summary))
    assert planner.main(["plan"]) == 0
    assert "::warning" in capsys.readouterr().out
    assert "SKIPPED" in summary.read_text("utf-8")
    values = dict(line.split("=", 1) for line in output.read_text("utf-8").splitlines())
    assert [r["target"] for r in json.loads(values["matrix"])] == LINUX


def records(out, targets):
    for target in targets:
        record = build.TargetBuild(
            target,
            "0.2.0",
            f"docling-0.2.0-{target}-runtime.tar.zst",
            "docling-0.2.0-models.tar.zst",
            "docling-0.2.0-tessdata.tar.zst",
            "Apache-2.0",
        )
        (out / f"build-{target}.json").write_text(json.dumps(asdict(record)), encoding="utf-8")
        for filename in record.files.values():
            with tarfile.open(out / filename, "w:zst") as archive:
                item = tarfile.TarInfo("payload")
                item.size = 1
                archive.addfile(item, io.BytesIO(b"x"))


@pytest.mark.parametrize(
    "problem", ["missing", "extra", "wrong-triple", "missing-component", "undeclared-skip"]
)
def test_assembly_refuses_incomplete_or_inconsistent_records(planner, tmp_path, problem):
    selected = planner.plan(fetch.load_pins(), "pull_request")
    records(tmp_path, LINUX)
    if problem == "missing":
        (tmp_path / f"build-{LINUX[1]}.json").unlink()
    elif problem == "extra":
        records(tmp_path, [MACOS])
    elif problem == "wrong-triple":
        path = tmp_path / f"build-{LINUX[0]}.json"
        value = json.loads(path.read_text("utf-8"))
        value["triple"] = MACOS
        path.write_text(json.dumps(value), encoding="utf-8")
    elif problem == "missing-component":
        (tmp_path / f"docling-0.2.0-{LINUX[0]}-runtime.tar.zst").unlink()
    else:
        selected["release"] = True
    with pytest.raises(SystemExit):
        planner.assemble(tmp_path, selected, "must-not-execute")


def test_manifest_rechecks_the_release_omission_before_writing(planner, tmp_path, monkeypatch):
    monkeypatch.setenv("GITHUB_EVENT_NAME", "workflow_dispatch")
    monkeypatch.setenv("PACK_CI_PLAN", json.dumps(planner.plan(fetch.load_pins(), "pull_request")))
    with pytest.raises(SystemExit, match="explicit macOS omission"):
        planner.main(["manifest", "--out", str(tmp_path)])
    assert not (tmp_path / "pack.toml").exists()


def test_manifest_rejects_a_plan_that_differs_from_scope(planner, tmp_path, monkeypatch):
    monkeypatch.setenv("GITHUB_EVENT_NAME", "pull_request")
    monkeypatch.setenv("PACK_CI_PLAN", "{}")
    with pytest.raises(SystemExit, match="differs from the current CI policy"):
        planner.main(["manifest", "--out", str(tmp_path)])
    assert not (tmp_path / "pack.toml").exists()


@pytest.mark.skipif(not os.environ.get("ASHIFT_BIN"), reason="requires the pack gate's built CLI")
def test_real_manifest_and_checksums_contain_only_built_targets(planner, tmp_path):
    selected = planner.plan(fetch.load_pins(), "pull_request")
    records(tmp_path, LINUX)
    stale = tmp_path / f"docling-0.2.0-{MACOS}-runtime.tar.zst"
    stale.write_bytes(b"not selected")
    planner.assemble(tmp_path, selected, os.environ["ASHIFT_BIN"])
    manifest = tomllib.loads((tmp_path / "pack.toml").read_text("utf-8"))
    assert {t["triple"] for t in manifest["targets"]} == set(LINUX)
    checksums = (tmp_path / "SHA256SUMS").read_text("utf-8").splitlines()
    expected = {"pack.toml", "docling-0.2.0-models.tar.zst", "docling-0.2.0-tessdata.tar.zst"}
    expected.update(f"docling-0.2.0-{t}-runtime.tar.zst" for t in LINUX)
    assert {line.split("  ", 1)[1] for line in checksums} == expected
    for line in checksums:
        digest, filename = line.split("  ", 1)
        assert hashlib.sha256((tmp_path / filename).read_bytes()).hexdigest() == digest
