"""Cache-key isolation and fail-closed verification for CPU torch source builds."""

from __future__ import annotations

import hashlib
import json
import re
import shutil
import subprocess
import sys
import zipfile
from argparse import Namespace
from pathlib import Path

import pytest
import yaml
from conftest import REPOSITORY

sys.path.insert(0, str(REPOSITORY / "engines/docling/pack"))

import fetch
import torch_build

LINUX_TARGET = "x86_64-unknown-linux-gnu"
WINDOWS_TARGET = "x86_64-pc-windows-msvc"
BOTH_TARGETS = (LINUX_TARGET, WINDOWS_TARGET)

PACK_FILES = (
    "torch-build.toml",
    "torch_build.py",
    "native_build.py",
    "vcomp-compat.cpp",
    "vcomp-probe.cpp",
    "vcomp-import.cpp",
    "pins.toml",
)


@pytest.fixture
def isolated_inputs(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> dict[str, Path]:
    """Mirror every file read by torch_build.key() in an isolated temp tree."""
    repo = tmp_path / "repo"
    pack = repo / "engines/docling/pack"
    workflows = repo / ".github/workflows"
    pack.mkdir(parents=True)
    workflows.mkdir(parents=True)

    shutil.copyfile(REPOSITORY / "uv.lock", repo / "uv.lock")
    shutil.copyfile(
        REPOSITORY / "engines/docling/pyproject.toml",
        repo / "engines/docling/pyproject.toml",
    )
    shutil.copyfile(
        REPOSITORY / ".github/workflows/torch-docling.yml",
        workflows / "torch-docling.yml",
    )
    for name in PACK_FILES:
        shutil.copyfile(REPOSITORY / "engines/docling/pack" / name, pack / name)

    real_load_pins = fetch.load_pins
    pins_path = pack / "pins.toml"
    monkeypatch.setattr(torch_build, "REPOSITORY", repo)
    monkeypatch.setattr(torch_build, "PACK", pack)
    monkeypatch.setattr(torch_build, "CONFIG", pack / "torch-build.toml")
    monkeypatch.setattr(torch_build, "__file__", str(pack / "torch_build.py"))
    monkeypatch.setattr(fetch, "load_pins", lambda path=pins_path: real_load_pins(path))

    return {
        "repo": repo,
        "pack": pack,
        "pins": pins_path,
        "workflow": workflows / "torch-docling.yml",
        "pyproject": repo / "engines/docling/pyproject.toml",
        "uv_lock": repo / "uv.lock",
    }


def _sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _write_synthetic_wheel(
    path: Path,
    *,
    extra_members: dict[str, bytes] | None = None,
    payload: bytes = b"# synthetic torch\n",
) -> None:
    members = {
        "torch/__init__.py": payload,
        "torch-2.14.1+cpu.dist-info/METADATA": (
            b"Metadata-Version: 2.1\nName: torch\nVersion: 2.14.1+cpu\n"
        ),
    }
    if extra_members:
        members.update(extra_members)
    with zipfile.ZipFile(path, "w") as archive:
        for name, content in members.items():
            archive.writestr(name, content)


def _create_cached_out(
    out: Path,
    target: str,
    *,
    extra_members: dict[str, bytes] | None = None,
) -> tuple[Path, dict[str, dict[str, object]]]:
    out.mkdir(parents=True, exist_ok=True)
    wheel_name = (
        "torch-2.14.1+cpu-cp314-cp314-win_amd64.whl"
        if target == WINDOWS_TARGET
        else "torch-2.14.1+cpu-cp314-cp314-manylinux_2_28_x86_64.whl"
    )
    wheel = out / wheel_name
    _write_synthetic_wheel(wheel, extra_members=extra_members)

    (out / "OpenBLAS-LICENSE.txt").write_bytes(b"BSD-3-Clause\n")
    if target == WINDOWS_TARGET:
        (out / "LLVM-OpenMP-LICENSE.txt").write_bytes(b"Apache-2.0 WITH LLVM-exception\n")
        (out / "libiomp5md.dll").write_bytes(b"MZ" + b"\x00" * 32)

    files = {}
    for artifact in sorted(out.iterdir()):
        if artifact.is_file():
            raw = artifact.read_bytes()
            files[artifact.name] = {"sha256": _sha256_bytes(raw), "size": len(raw)}

    record = {
        "schema": 1,
        "target": target,
        "build_key": torch_build.key(target),
        "files": files,
    }
    (out / "build.json").write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
    return wheel, files


def test_key_returns_sha256_hex_and_separates_targets(isolated_inputs: dict[str, Path]) -> None:
    linux_key = torch_build.key(LINUX_TARGET)
    windows_key = torch_build.key(WINDOWS_TARGET)
    assert re.fullmatch(r"[0-9a-f]{64}", linux_key)
    assert re.fullmatch(r"[0-9a-f]{64}", windows_key)
    assert linux_key != windows_key


@pytest.mark.parametrize("target", BOTH_TARGETS)
def test_torch_docling_workflow_bump_does_not_invalidate_key(
    isolated_inputs: dict[str, Path], target: str
) -> None:
    before = torch_build.key(target)
    workflow = isolated_inputs["workflow"]
    original = workflow.read_text(encoding="utf-8")
    bumped = original.replace(
        "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1",
        "actions/checkout@0000000000000000000000000000000000000000 # v7.0.2",
    )
    assert bumped != original
    workflow.write_text(bumped, encoding="utf-8")

    assert torch_build.key(target) == before


def test_pack_docling_native_cache_key_drops_workflow_and_includes_pyproject() -> None:
    workflow = yaml.safe_load(
        (REPOSITORY / ".github/workflows/pack-docling.yml").read_text("utf-8")
    )
    step = next(
        item
        for item in workflow["jobs"]["pack"]["steps"]
        if item.get("name") == "Cache native source builds"
    )
    cache_key = step["with"]["key"]
    assert "torch-docling.yml" not in cache_key
    match = re.search(r"hashFiles\(([^)]+)\)", cache_key)
    assert match is not None
    hashed = [item.strip().strip("'\"") for item in match.group(1).split(",")]
    assert hashed == [
        "engines/docling/pack/native_build.py",
        "engines/docling/pack/torch_build.py",
        "engines/docling/pack/torch-build.toml",
        "engines/docling/pack/vcomp-*.cpp",
        "engines/docling/pack/pins.toml",
        "engines/docling/pyproject.toml",
        "uv.lock",
    ]


@pytest.mark.parametrize(
    "input_name",
    [
        "target",
        "torch-build.toml",
        "torch_build.py",
        "native_build.py",
        "vcomp-compat.cpp",
        "vcomp-probe.cpp",
        "vcomp-import.cpp",
        "pyproject.toml",
        "uv.lock",
        "pins.llvm",
        "pins.manylinux",
    ],
)
def test_mutating_each_real_build_input_changes_key(
    isolated_inputs: dict[str, Path], input_name: str
) -> None:
    baseline = torch_build.key(LINUX_TARGET)
    pack = isolated_inputs["pack"]

    if input_name == "target":
        assert torch_build.key(WINDOWS_TARGET) != baseline
        return

    if input_name in {"pins.llvm", "pins.manylinux"}:
        section = input_name.split(".", 1)[1]
        pins_path = isolated_inputs["pins"]
        text = pins_path.read_text(encoding="utf-8")
        if section == "llvm":
            updated = text.replace('version = "23.1.4"', 'version = "23.1.5"', 1)
        else:
            updated = text.replace(
                "f6adcfd450f0927a05c66ded9b4c3bb538fbab625b1ca3f35d71ac093b24b57f",
                "0" * 64,
                1,
            )
        assert updated != text
        pins_path.write_text(updated, encoding="utf-8")
    elif input_name == "torch-build.toml":
        config_path = pack / input_name
        text = config_path.read_text(encoding="utf-8")
        updated = text.replace('USE_NNPACK = "0"', 'USE_NNPACK = "1"', 1)
        assert updated != text
        config_path.write_text(updated, encoding="utf-8")
    elif input_name == "pyproject.toml":
        pyproject = isolated_inputs["pyproject"]
        text = pyproject.read_text(encoding="utf-8")
        updated = text.replace('"cmake==4.4.4"', '"cmake==4.4.5"', 1)
        assert updated != text
        pyproject.write_text(updated, encoding="utf-8")
    elif input_name == "uv.lock":
        isolated_inputs["uv_lock"].write_text(
            isolated_inputs["uv_lock"].read_text(encoding="utf-8") + "\n# lock bump\n",
            encoding="utf-8",
        )
    elif input_name.endswith(".py"):
        path = pack / input_name
        path.write_text(
            path.read_text(encoding="utf-8") + "\n# build input change\n",
            encoding="utf-8",
        )
    else:
        path = pack / input_name
        path.write_bytes(path.read_bytes() + b"\n// build input change\n")

    assert torch_build.key(LINUX_TARGET) != baseline


@pytest.mark.parametrize("target", BOTH_TARGETS)
def test_unrelated_pins_do_not_change_key(isolated_inputs: dict[str, Path], target: str) -> None:
    baseline = torch_build.key(target)
    pins_path = isolated_inputs["pins"]
    text = pins_path.read_text(encoding="utf-8")
    updated = text.replace(
        'release = "toolchain-wheels-v0.2.0"',
        'release = "toolchain-wheels-v0.2.9"',
        1,
    )
    updated = updated.replace('version = "5.0.0.93"', 'version = "5.0.0.99"', 1)
    updated = updated.replace('revision = "v3.10.0"', 'revision = "v3.10.1"', 1)
    updated = updated.replace('version = "3.14.9"', 'version = "3.14.10"', 1)
    updated += (
        "\n[toolchain.targets.x86_64-unknown-linux-gnu.torch]\n"
        f'build_key = "{baseline}"\n'
        'files = [{ name = "torch.whl", url = "https://example.invalid/torch.whl", '
        f'sha256 = "{"a" * 64}", size = 123 }}]\n'
    )
    assert updated != text
    pins_path.write_text(updated, encoding="utf-8")

    assert torch_build.key(target) == baseline


def test_framing_prevents_adjacent_input_collisions(isolated_inputs: dict[str, Path]) -> None:
    pack = isolated_inputs["pack"]
    compat = pack / "vcomp-compat.cpp"
    probe_cpp = pack / "vcomp-probe.cpp"
    compat_bytes = compat.read_bytes()
    probe_bytes = probe_cpp.read_bytes()

    compat.write_bytes(compat_bytes + b"X")
    probe_cpp.write_bytes(probe_bytes)
    left_shift = torch_build.key(LINUX_TARGET)

    compat.write_bytes(compat_bytes)
    probe_cpp.write_bytes(b"X" + probe_bytes)
    right_shift = torch_build.key(LINUX_TARGET)

    assert left_shift != right_shift

    config = pack / "torch-build.toml"
    config_bytes = config.read_bytes()
    left_target = torch_build.key(LINUX_TARGET + "#A\n")
    config.write_bytes(b"#A\n" + config_bytes)
    right_config = torch_build.key(LINUX_TARGET)
    assert left_target != right_config


def test_key_still_fails_when_torch_version_diverges_from_uv_lock(
    isolated_inputs: dict[str, Path],
) -> None:
    config = isolated_inputs["pack"] / "torch-build.toml"
    config.write_text(
        config.read_text(encoding="utf-8").replace(
            'version = "2.14.1+cpu"', 'version = "2.14.9+cpu"'
        ),
        encoding="utf-8",
    )
    with pytest.raises(SystemExit, match=r"do not match uv\.lock"):
        torch_build.key(LINUX_TARGET)


@pytest.mark.parametrize("target", BOTH_TARGETS)
def test_verify_cache_hit_passes_and_runs_probes(
    isolated_inputs: dict[str, Path],
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    target: str,
) -> None:
    out = tmp_path / f"torch-out-{target}"
    wheel, _ = _create_cached_out(out, target)
    site = tmp_path / "site"
    (site / "torch/lib").mkdir(parents=True)

    runs: list[list[str]] = []
    probes: list[str] = []
    monkeypatch.setattr(torch_build, "run", lambda cmd, **_: runs.append([str(c) for c in cmd]))
    monkeypatch.setattr(
        torch_build,
        "probe",
        lambda _py, code, **_: probes.append(code) or subprocess.CompletedProcess([], 0),
    )
    monkeypatch.setattr(subprocess, "check_output", lambda *_, **__: f"{site}\n")

    torch_build.verify(Namespace(target=target, out=out))

    assert len(runs) == 1
    assert "--reinstall" in runs[0]
    assert str(wheel) in runs[0]
    assert probes == [torch_build.TORCH_PROBE, torch_build.VISION_PROBE]
    if target == WINDOWS_TARGET:
        assert (site / "torch/lib/libiomp5md.dll").read_bytes() == (
            out / "libiomp5md.dll"
        ).read_bytes()


@pytest.mark.parametrize(
    ("mutation", "pattern"),
    [
        ("build_key", "different inputs"),
        ("target", "different inputs"),
        ("extra_file", "missing or unexpected files"),
        ("missing_file", "missing or unexpected files"),
        ("tamper_wheel_same_size", "changed"),
        ("tamper_sidecar_same_size", "changed"),
        ("size_mismatch", "changed"),
        ("audit_vendor_mkl", "vendor library check failed"),
        ("audit_hook", "audit failure"),
    ],
)
def test_verify_cache_hit_fails_closed(
    isolated_inputs: dict[str, Path],
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    mutation: str,
    pattern: str,
) -> None:
    out = tmp_path / "torch-out"
    extra_members = (
        {"torch/lib/libmkl_rt.so": b"\x7fELFsynthetic mkl"}
        if mutation == "audit_vendor_mkl"
        else None
    )
    wheel, _ = _create_cached_out(out, LINUX_TARGET, extra_members=extra_members)
    record_path = out / "build.json"
    record = json.loads(record_path.read_text(encoding="utf-8"))

    if mutation == "build_key":
        record["build_key"] = "0" * 64
        record_path.write_text(json.dumps(record), encoding="utf-8")
    elif mutation == "target":
        record["target"] = WINDOWS_TARGET
        record_path.write_text(json.dumps(record), encoding="utf-8")
    elif mutation == "extra_file":
        (out / "unexpected.txt").write_bytes(b"rogue")
    elif mutation == "missing_file":
        (out / "OpenBLAS-LICENSE.txt").unlink()
    elif mutation == "tamper_wheel_same_size":
        original = wheel.read_bytes()
        tampered = original[:-1] + (b"B" if original[-1:] != b"B" else b"C")
        assert len(tampered) == len(original)
        wheel.write_bytes(tampered)
    elif mutation == "tamper_sidecar_same_size":
        sidecar = out / "OpenBLAS-LICENSE.txt"
        original = sidecar.read_bytes()
        tampered = b"X" + original[1:]
        assert len(tampered) == len(original)
        sidecar.write_bytes(tampered)
    elif mutation == "size_mismatch":
        record["files"]["OpenBLAS-LICENSE.txt"]["size"] += 1
        record_path.write_text(json.dumps(record), encoding="utf-8")
    elif mutation == "audit_hook":

        def fail_audit(*_args, **_kwargs):
            raise SystemExit("audit failure")

        monkeypatch.setattr(torch_build, "audit_artifacts", fail_audit)

    runs: list[object] = []
    probes: list[object] = []
    monkeypatch.setattr(torch_build, "run", lambda *args, **kwargs: runs.append((args, kwargs)))
    monkeypatch.setattr(torch_build, "probe", lambda *args, **kwargs: probes.append((args, kwargs)))

    with pytest.raises(SystemExit, match=pattern):
        torch_build.verify(Namespace(target=LINUX_TARGET, out=out))

    assert not runs
    assert not probes
