"""Pack CI omissions remain visible and cannot silently narrow a release."""

from __future__ import annotations

import base64
import csv
import hashlib
import importlib
import importlib.util
import io
import json
import os
import pathlib
import runpy
import shlex
import subprocess
import sys
import tarfile
import tomllib
import zipfile
from dataclasses import asdict
from types import SimpleNamespace

import pytest
import yaml
from conftest import REPOSITORY

sys.path.insert(0, str(REPOSITORY / "engines/docling/pack"))

import build
import fetch
import native_build

LINUX = ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"]
MACOS = "aarch64-apple-darwin"


@pytest.fixture
def pack_workflow():
    return yaml.safe_load((REPOSITORY / ".github/workflows/pack-docling.yml").read_text("utf-8"))


def workflow_condition(expression, **context):
    """Evaluate the workflow's boolean conditions with explicit Actions contexts."""
    expression = expression.removeprefix("${{").removesuffix("}}").strip()
    expression = expression.replace("&&", " and ").replace("||", " or ")
    # The only input is the checked-in workflow, already executable repository code.
    return eval(  # noqa: S307
        f"({expression})",
        {"__builtins__": {}},
        {"always": lambda: True, "startsWith": str.startswith, **context},
    )


@pytest.mark.parametrize(
    "event,ref,expected",
    [
        ("pull_request", "refs/pull/271/merge", True),
        ("workflow_dispatch", "refs/heads/feat/web-tool-suite", True),
        ("workflow_dispatch", "refs/heads/dev", False),
        ("workflow_dispatch", "refs/heads/main", False),
        ("workflow_dispatch", "refs/tags/pack-docling-v0.2.0", False),
        ("push", "refs/tags/pack-docling-v0.2.0", False),
    ],
)
def test_pack_supersession_preserves_trusted_branches_and_tags(pack_workflow, event, ref, expected):
    condition = pack_workflow["concurrency"]["cancel-in-progress"]
    github = SimpleNamespace(event_name=event, ref=ref)
    assert workflow_condition(condition, github=github) is expected


@pytest.mark.parametrize(
    "event,ref,expected",
    [
        ("push", "refs/tags/pack-docling-v0.2.0", True),
        ("pull_request", "refs/pull/271/merge", False),
        ("workflow_dispatch", "refs/tags/pack-docling-v0.2.0", False),
        ("push", "refs/tags/v0.6.0", False),
        ("push", "refs/tags/pack-pdfium-v0.1.0", False),
    ],
)
def test_release_job_condition_selects_docling_tag_pushes_only(pack_workflow, event, ref, expected):
    release = pack_workflow["jobs"]["release"]
    github = SimpleNamespace(event_name=event, ref=ref)
    assert bool(workflow_condition(release["if"], github=github)) is expected


def test_release_job_configuration_and_steps(pack_workflow):
    release = pack_workflow["jobs"]["release"]
    assert release["needs"] == ["engine", "conformance", "manifest"]
    assert release["environment"] == "release"
    assert release["permissions"] == {"contents": "write"}

    steps = {step["name"]: step for step in release["steps"]}

    checkout = steps["Check out source"]
    assert checkout["uses"].startswith("actions/checkout@")
    assert checkout["with"]["fetch-depth"] == 0
    assert checkout["with"]["persist-credentials"] is False

    ancestry = steps["Require stable branch ancestry"]
    assert 'git merge-base --is-ancestor "$GITHUB_SHA" refs/remotes/origin/main' in ancestry["run"]

    checksums = steps["Verify component checksums"]
    assert "sha256sum --check SHA256SUMS" in checksums["run"]

    signing = steps["Sign the manifest"]
    assert "scripts/pack/sign-pack.sh" in signing["run"]
    assert signing.get("env", {}).get("MINISIGN_SECRET_KEY") == "${{ secrets.PACK_MINISIGN_KEY }}"

    for job_name, job in pack_workflow["jobs"].items():
        assert "PACK_MINISIGN_KEY" not in str(job.get("env", {}))
        for step in job.get("steps", []):
            if job_name == "release" and step.get("name") == "Sign the manifest":
                continue
            assert "PACK_MINISIGN_KEY" not in str(step.get("env", {}))

    publish = steps["Publish the release"]
    assert 'gh release create "$TAG" --verify-tag' in publish["run"]


@pytest.mark.parametrize(
    "hit,outcome,cleanup,restore_compiler,save_compiler,save_native",
    [
        ("true", "success", "success", False, False, False),
        ("true", "failure", "success", False, False, False),
        ("false", "success", "success", True, False, True),
        ("false", "failure", "success", True, True, False),
        ("false", "cancelled", "success", True, True, False),
        ("false", "cancelled", "failure", True, False, False),
        ("false", "skipped", "skipped", True, False, False),
        ("false", "", "skipped", True, False, False),
    ],
)
@pytest.mark.parametrize("target", [*LINUX, "x86_64-pc-windows-msvc"])
def test_pack_caches_wheels_or_stopped_compiler_checkpoints(
    pack_workflow, hit, outcome, cleanup, restore_compiler, save_compiler, save_native, target
):
    steps = {step["name"]: step for step in pack_workflow["jobs"]["pack"]["steps"]}
    context = {
        "matrix": SimpleNamespace(target=target),
        "steps": SimpleNamespace(
            native=SimpleNamespace(outputs={"cache-hit": hit}),
            build=SimpleNamespace(outcome=outcome),
            cleanup=SimpleNamespace(outcome=cleanup),
        ),
    }
    for name, expected in (
        ("Restore native compiler work", restore_compiler),
        ("Save native compiler work", save_compiler),
        ("Save native source builds", save_native),
    ):
        # Actions permits hyphens in property names; Python uses subscription here.
        condition = steps[name]["if"].replace("outputs.cache-hit", "outputs['cache-hit']")
        assert bool(workflow_condition(condition, **context)) is (
            expected and target.startswith("x86_64-")
        ), name


def test_pack_checkpoints_follow_bounded_build_and_cleanup(pack_workflow):
    pack = pack_workflow["jobs"]["pack"]
    steps = {step["name"]: step for step in pack["steps"]}
    names = list(steps)
    assert steps["Build the pack"]["timeout-minutes"] == 310 < pack["timeout-minutes"]
    cleanup = names.index("Stop compiler processes before saving their cache")
    assert names.index("Build the pack") < cleanup
    assert cleanup < names.index("Save native source builds")
    assert cleanup < names.index("Save native compiler work")
    assert names.index("Save native source builds") < names.index(
        "Rebuild and compare component hashes"
    )
    assert "cache/restore@" in steps["Cache native source builds"]["uses"]
    assert steps["Save native source builds"]["with"]["key"] == (
        "${{ steps.native.outputs.cache-primary-key }}"
    )
    assert "TORCH_BUILD_CONTAINER" in pack["env"]


@pytest.mark.parametrize("present,stop_fails", [(True, False), (False, False), (True, True)])
def test_pack_cleanup_stops_only_its_named_container(
    pack_workflow, monkeypatch, tmp_path, present, stop_fails
):
    step = next(
        step for step in pack_workflow["jobs"]["pack"]["steps"] if step.get("id") == "cleanup"
    )
    monkeypatch.setenv("TORCH_BUILD_CONTAINER", "ashift-pack-torch-test")
    monkeypatch.setenv("TARGET", LINUX[0])
    calls = []

    def run(command, **kwargs):
        calls.append(command)
        assert kwargs["timeout"] <= 60
        if command[:3] == ["docker", "container", "inspect"]:
            return subprocess.CompletedProcess(command, 0 if present else 1)
        if command[:2] == ["docker", "stop"] and stop_fails:
            raise subprocess.CalledProcessError(1, command)
        return subprocess.CompletedProcess(command, 0)

    monkeypatch.setattr(subprocess, "run", run)
    script = tmp_path / "cleanup.py"
    script.write_text(step["run"], "utf-8")
    if stop_fails:
        with pytest.raises(subprocess.CalledProcessError):
            runpy.run_path(str(script))
    else:
        runpy.run_path(str(script))
    assert calls[0] == ["docker", "container", "inspect", "ashift-pack-torch-test"]
    if present:
        assert calls[1] == ["docker", "stop", "--time", "30", "ashift-pack-torch-test"]
    if not stop_fails:
        assert calls[-1] == ["sccache", "--stop-server"]


def test_toolchain_linux_opencv_step_runs_inside_uv_project_environment(tmp_path, monkeypatch):
    workflow = yaml.safe_load(
        (REPOSITORY / ".github/workflows/toolchain-wheels.yml").read_text("utf-8")
    )
    step = next(
        step
        for step in workflow["jobs"]["build"]["steps"]
        if step.get("name")
        == "Build IPP-free OpenCV wheel, collate toolchain artifacts and audit wheels"
    )
    commands = []

    def run(command, **kwargs):
        commands.append(command)
        return subprocess.CompletedProcess(command, 0)

    monkeypatch.setattr(subprocess, "run", run)
    script = tmp_path / "step.py"
    script.write_text(step["run"], "utf-8")
    runpy.run_path(str(script))
    assert len(commands) == 1
    command = commands[0]
    assert command[:8] == [
        "uv",
        "run",
        "--package",
        "ariad-docling",
        "--group",
        "pack-build",
        "--frozen",
        "python",
    ]
    assert command[8] == "-c"
    compile(command[9], "<toolchain-linux-step>", "exec")
    for fragment in ("uv", "pip", "sys.executable", "licenses.check_wheel_files"):
        assert fragment in command[9]


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


TORCH_WHEEL = "torch-2.14.1+cpu-cp314-cp314-manylinux_2_27_x86_64.manylinux_2_28_x86_64.whl"
TORCH_DIST_INFO = "torch-2.14.1+cpu.dist-info"
TORCH_NOTICES = {
    "notices/pytorch-2.14.1-native-notices.txt": "pytorch-2.14.1-native-notices.txt",
    "notices/llvm-openmp-notice.txt": "llvm-openmp-notice.txt",
    "notices/ittapi.txt": "ittapi.txt",
}


def toolchain_build_script(monkeypatch, tmp_path):
    """The audited inner program of the Linux toolchain job, as the runner would receive it."""
    workflow = yaml.safe_load(
        (REPOSITORY / ".github/workflows/toolchain-wheels.yml").read_text("utf-8")
    )
    step = next(
        step
        for step in workflow["jobs"]["build"]["steps"]
        if step.get("name")
        == "Build IPP-free OpenCV wheel, collate toolchain artifacts and audit wheels"
    )
    commands = []
    with monkeypatch.context() as patch:
        patch.setattr(
            subprocess, "run", lambda command, **_: commands.append(command) or SimpleNamespace()
        )
        outer = tmp_path / "outer-step.py"
        outer.write_text(step["run"], "utf-8")
        runpy.run_path(str(outer))
    return commands[0][9]


def unpack_wheel_without_wheel_package(command):
    """Unpack a wheel and verify its RECORD as ``python -m wheel unpack`` does."""
    dest = pathlib.Path(command[command.index("--dest") + 1])
    wheel = pathlib.Path(command[-1])

    def fail(reason):
        raise subprocess.CalledProcessError(1, command, stderr=f"{wheel.name}: {reason}")

    with zipfile.ZipFile(wheel) as archive:
        members = {info.filename: info for info in archive.infolist() if not info.is_dir()}
        records = [name for name in members if name.endswith(".dist-info/RECORD")]
        if len(records) != 1:
            fail("expected exactly one RECORD")
        hashed = set()
        for name, checksum, size in csv.reader(io.StringIO(archive.read(records[0]).decode())):
            if name == records[0]:
                continue
            if name not in members:
                fail(f"RECORD lists a missing member: {name}")
            data = archive.read(name)
            algorithm, _, expected = checksum.partition("=")
            digest = base64.urlsafe_b64encode(hashlib.new(algorithm, data).digest())
            if algorithm != "sha256" or digest.decode().rstrip("=") != expected:
                fail(f"RECORD digest mismatch: {name}")
            if size and int(size) != len(data):
                fail(f"RECORD size mismatch: {name}")
            hashed.add(name)
        if unhashed := set(members) - hashed - {records[0]}:
            fail(f"members without a RECORD hash: {sorted(unhashed)}")
        archive.extractall(dest / wheel.stem)  # noqa: S202 - the test's own synthetic wheel


def synthetic_torch_wheel(path):
    members = {
        "torch/__init__.py": b"# synthetic torch\n",
        "torch/lib/data.bin": bytes(range(256)) * 64,
        f"{TORCH_DIST_INFO}/METADATA": b"Metadata-Version: 2.1\nName: torch\nVersion: 2.14.1+cpu\n",
        f"{TORCH_DIST_INFO}/WHEEL": (
            b"Wheel-Version: 1.0\nGenerator: test\nRoot-Is-Purelib: false\n"
            b"Tag: cp314-cp314-manylinux_2_27_x86_64\nTag: cp314-cp314-manylinux_2_28_x86_64\n"
        ),
    }
    record = "".join(f"{name},,\n" for name in members) + f"{TORCH_DIST_INFO}/RECORD,,\n"
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as archive:
        for name, data in members.items():
            info = zipfile.ZipInfo(name, date_time=(2026, 1, 2, 3, 4, 6))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o100755 << 16
            archive.writestr(info, data)
        archive.writestr(f"{TORCH_DIST_INFO}/RECORD", record)


@pytest.mark.skipif(
    sys.platform != "linux" or sys.version_info[:2] != (3, 14),
    reason="the synthetic wheel carries the Linux x86_64 cp314 tags the toolchain job produces",
)
def test_toolchain_job_repacks_the_torch_wheel_with_reviewed_notices(tmp_path, monkeypatch):
    import licenses

    script = toolchain_build_script(monkeypatch, tmp_path)
    work = tmp_path / "work"
    (work / "torch-out").mkdir(parents=True)
    (work / "engines").symlink_to(REPOSITORY / "engines")
    wheel = work / "torch-out" / TORCH_WHEEL
    synthetic_torch_wheel(wheel)
    openblas = work / "torch-out/OpenBLAS-LICENSE.txt"
    openblas.write_bytes(b"OpenBLAS licence\n")
    target = "x86_64-unknown-linux-gnu"
    build_record = {
        "schema": 1,
        "target": target,
        "build_key": "synthetic-key",
        "files": {
            TORCH_WHEEL: {"sha256": fetch._sha256(wheel), "size": wheel.stat().st_size},
            openblas.name: {"sha256": fetch._sha256(openblas), "size": openblas.stat().st_size},
        },
    }
    build_json = work / "torch-out/build.json"
    build_json.write_text(json.dumps(build_record, indent=2) + "\n", "utf-8")
    original = {
        path.name: hashlib.sha256(path.read_bytes()).hexdigest()
        for path in (wheel, openblas, build_json)
    }

    audits = []
    installs = []
    real_run = subprocess.run

    def run(command, **kwargs):
        # Only the OpenCV reinstall into the build environment is replaced; the virtual-environment
        # install of the repacked wheel runs for real. The unpack runs through the ``wheel`` package
        # when it is installed (``pack-build`` group) and through an equivalent RECORD-verifying
        # unpack otherwise, as in the plain ``just docling-test`` environment.
        if "--reinstall" in command:
            installs.append(command)
            return subprocess.CompletedProcess(command, 0)
        if (
            len(command) >= 6
            and command[1:4] == ["-m", "wheel", "unpack"]
            and importlib.util.find_spec("wheel") is None
        ):
            unpack_wheel_without_wheel_package(command)
            return subprocess.CompletedProcess(command, 0)
        return real_run(command, **kwargs)

    monkeypatch.chdir(work)
    monkeypatch.setenv("TARGET", target)
    monkeypatch.setattr(subprocess, "run", run)
    monkeypatch.setattr(native_build, "opencv", lambda *_: work / "opencv.whl")
    monkeypatch.setattr(native_build, "verify_opencv", lambda *_, **__: None)
    monkeypatch.setattr(native_build, "export_opencv", lambda *_: None)
    monkeypatch.setattr(
        licenses,
        "check_wheel_files",
        lambda artifacts, **options: audits.append((list(artifacts), options)),
    )
    exec(compile(script, "<toolchain-linux-step>", "exec"), {"__name__": "__main__"})  # noqa: S102

    assert len(installs) == 1
    # The cache-keyed inputs are byte-for-byte what the build produced.
    assert {
        path.name: hashlib.sha256(path.read_bytes()).hexdigest()
        for path in (wheel, openblas, build_json)
    } == original

    released = work / "wheels-out" / TORCH_WHEEL
    reviewed = {
        notice["path"]: notice["sha256"]
        for notice in licenses._NATIVE_COMPONENTS["torch-native"]["notices"]
    }
    with zipfile.ZipFile(released) as archive:
        for member, source in TORCH_NOTICES.items():
            data = archive.read(member)
            assert data == (REPOSITORY / "engines/docling/pack/licenses" / source).read_bytes()
            assert hashlib.sha256(data).hexdigest() == reviewed[source]
        rows = list(csv.reader(io.StringIO(archive.read(f"{TORCH_DIST_INFO}/RECORD").decode())))
        assert rows[-1] == [f"{TORCH_DIST_INFO}/RECORD", "", ""]
        assert {row[0] for row in rows} == set(archive.namelist())
        for name, checksum, size in rows[:-1]:
            data = archive.read(name)
            digest = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).decode().rstrip("=")
            assert (checksum, size) == (f"sha256={digest}", str(len(data)))
        with zipfile.ZipFile(wheel) as source:
            for info in source.infolist():
                if info.filename.endswith("/RECORD"):
                    continue
                copied = archive.getinfo(info.filename)
                assert (copied.date_time, copied.external_attr, copied.compress_type) == (
                    info.date_time,
                    info.external_attr,
                    info.compress_type,
                )
                assert archive.read(info.filename) == source.read(info.filename)

    record = json.loads((work / "wheels-out" / f"torch-build-{target}.json").read_text("utf-8"))
    entry = record["files"][TORCH_WHEEL]
    assert entry["sha256"] == hashlib.sha256(released.read_bytes()).hexdigest()
    assert entry["sha256"] != build_record["files"][TORCH_WHEEL]["sha256"]
    assert entry["size"] == released.stat().st_size != build_record["files"][TORCH_WHEEL]["size"]
    sidecar = record["files"][openblas.name]
    assert sidecar["asset"] == f"OpenBLAS-LICENSE-{target}.txt"
    assert (sidecar["sha256"], sidecar["size"]) == (
        build_record["files"][openblas.name]["sha256"],
        build_record["files"][openblas.name]["size"],
    )
    assert (work / "wheels-out" / sidecar["asset"]).read_bytes() == openblas.read_bytes()
    assert len(audits) == 1
    artifacts, options = audits[0]
    assert pathlib.Path("wheels-out", TORCH_WHEEL) in artifacts and options["target"] == target
