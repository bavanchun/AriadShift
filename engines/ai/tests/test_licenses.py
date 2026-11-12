"""Licence receipts and development pack assembly checks."""

from __future__ import annotations

import hashlib
import importlib.util
import json
import os
import shutil
import subprocess
import sys
import tarfile
import tomllib
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
NAME = ROOT.name
spec = importlib.util.spec_from_file_location(NAME + "_pack_licenses", ROOT / "pack/licenses.py")
assert spec and spec.loader
licenses = importlib.util.module_from_spec(spec)
spec.loader.exec_module(licenses)


def test_licence_inventory_and_notices():
    assert licenses.check() == []
    pins = tomllib.loads((ROOT / "pack/pins.toml").read_text())
    reference = tomllib.loads((ROOT.parent / "sign/pack/pins.toml").read_text())
    assert pins["python"] == reference["python"]
    assert not list(ROOT.glob("**/*.gguf"))
    assert not list(ROOT.glob("**/*.safetensors"))
    if NAME == "inpaint":
        assert set(pins["policy"]["excluded_weights"]) == {"big-lama", "Places365"}
        assert not list(ROOT.glob("**/*lama*.pt"))


def test_licence_gate_rejects_changed_notice(tmp_path):
    pack = tmp_path / "pack"
    shutil.copytree(ROOT / "pack", pack)
    notice = next((pack / "notices").glob("*.txt"))
    notice.write_text("Changed notice")
    assert any("digest mismatch" in error for error in licenses.check(pack))


def test_licence_gate_rejects_unreviewed_license(tmp_path):
    pack = tmp_path / "pack"
    shutil.copytree(ROOT / "pack", pack)
    pins = pack / "pins.toml"
    pins.write_text(pins.read_text().replace('license = "BSD-3-Clause"', 'license = "AGPL-3.0"'))
    assert any("unapproved licence" in error for error in licenses.check(pack))


def test_development_pack_build_and_entrypoint(tmp_path):
    root = tmp_path / (NAME + " pack")
    result = subprocess.run(
        [sys.executable, str(ROOT / "pack/build.py"), "build", "--out", str(root)],
        capture_output=True,
        text=True,
        check=False,
        timeout=60,
    )
    assert result.returncode == 0, result.stdout + result.stderr
    manifest = tomllib.loads((root / "pack.toml").read_text())
    assert manifest["schema"] == "ariad-pack/1"
    assert manifest["name"] == NAME
    assert manifest["engine_protocol"] == "ariad-engine/2"
    assert manifest["license"] == "Apache-2.0 AND MIT"
    build = manifest["targets"][0]
    program = (root / build["entrypoint"][0]).resolve()
    assert program.is_relative_to(root)
    component = build["components"][0]
    archive = root / component["url"]
    assert hashlib.sha256(archive.read_bytes()).hexdigest() == component["sha256"]
    assert archive.stat().st_size == component["size"]
    with tarfile.open(archive) as package:
        assert len(package.getmembers()) == component["entries"]
        assert sum(info.size for info in package.getmembers()) == component["unpacked_size"]
    assert all((root / notice).stat().st_size for notice in manifest["notices"])
    # Mirror host argument expansion after moving the pack away from its build path.
    moved = tmp_path / (NAME + " relocated pack")
    root.rename(moved)
    root = moved
    program = root / build["entrypoint"][0]
    # Disable automatic site discovery to exercise the launcher's explicit import paths.
    args = [
        str(program),
        "-S",
        *(arg.replace("{root}", str(root)) for arg in build["entrypoint"][1:]),
    ]
    work = tmp_path / "job"
    work.mkdir()
    child = subprocess.run(
        args,
        cwd=work,
        env={key: value for key, value in os.environ.items() if not key.startswith("PYTHON")},
        input=json.dumps({"protocol": "ariad-engine/2", "job": "pack-test", "op": "describe"})
        + "\n",
        capture_output=True,
        text=True,
        check=True,
        timeout=15,
    )
    events = [json.loads(line) for line in child.stdout.splitlines()]
    assert events[0]["engine"] == NAME and events[-1]["ok"]

    with (root / "pack.toml").open("a") as manifest_file:
        manifest_file.write(
            '\n[targets.options]\nmodel_path = "{root}/model.gguf"\nbackend = "python"\n'
        )
    config = importlib.import_module("ariad_" + NAME + ".config")
    defaults = config.pack_options({}, root / "python-env" / "module.py")
    assert defaults["model_path"] == str(root / "model.gguf")
    assert defaults["backend"] == "python"
    assert (
        config.pack_options({"backend": "auto"}, root / "python-env" / "module.py")["backend"]
        == "auto"
    )

    host = os.environ.get("ASHIFT_TEST_BIN")
    if host:
        # Exercise the real host resolver and sandbox using the assembled runtime.
        # These IP socket creations generate no packets, even if isolation regresses.
        if sys.platform == "linux":
            module = next(root.glob("python-env/lib/python*/site-packages/ariad_*/__main__.py"))
            guard = """import errno
import socket
for family in (socket.AF_INET, socket.AF_INET6):
    for kind in (socket.SOCK_STREAM, socket.SOCK_DGRAM):
        try:
            probe = socket.socket(family, kind)
        except OSError as error:
            if error.errno not in (errno.EPERM, errno.EACCES):
                raise
        else:
            probe.close()
            raise SystemExit("sandbox allowed an IP socket")
"""
            module.write_text(guard + module.read_text())
        host_result = subprocess.run(
            [host, "engines", "--json"],
            capture_output=True,
            text=True,
            timeout=60,
            check=True,
            env={**os.environ, "ASHIFT_PACK_" + NAME.upper(): str(root)},
        )
        status = next(item for item in json.loads(host_result.stdout) if item["id"] == NAME)
        assert status["status"] == "ready", status
        assert status["pack"]["source"] == "override"


@pytest.mark.parametrize(
    "selection,expected",
    [
        ("odd", {1, 3, 5}),
        ("even", {2, 4}),
        ("last", {5}),
        ("last-1", {4}),
        ("2-last-1", {2, 3, 4}),
        ("-3", {1, 2, 3}),
        ("4-", {4, 5}),
        ("5-3", {3, 4, 5}),
    ],
)
def test_host_page_range_grammar(selection, expected):
    pages = importlib.import_module("ariad_" + NAME + ".pages")
    assert pages.selected_pages(selection, 5) == expected
