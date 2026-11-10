"""Assemble an unpacked local development pack; never download model weights."""

from __future__ import annotations

import argparse
import hashlib
import importlib.metadata
import json
import os
import platform
import shutil
import subprocess
import sys
import sysconfig
import tarfile
import tomllib
import venv
from pathlib import Path

import licenses

ROOT = Path(__file__).resolve().parents[1]
PACK_NAME = "inpaint"
MODULE = "ariad_" + PACK_NAME


def host_triple() -> str:
    machine = platform.machine().lower()
    arch = "aarch64" if machine in {"aarch64", "arm64"} else "x86_64"
    if sys.platform == "darwin":
        return arch + "-apple-darwin"
    if sys.platform == "win32":
        return "x86_64-pc-windows-msvc"
    return arch + "-unknown-linux-gnu"


def build(out: Path) -> Path:
    """Copy the active Python interpreter and reviewed pure Python dependencies.

    The development pack copies the active interpreter and its standard library.
    Release builds must use the hash-pinned standalone Python archive instead.
    """
    errors = licenses.check()
    if errors:
        raise ValueError("\n".join(errors))
    distribution = importlib.metadata.distribution("pypdf")
    pins = tomllib.loads((ROOT / "pack/pins.toml").read_text())
    if distribution.version != pins["pypdf"]["version"]:
        raise ValueError("install the pypdf version locked in this engine's uv.lock")
    out = out.resolve()
    if out.exists() and any(out.iterdir()):
        raise ValueError("output directory must be empty")
    out.mkdir(parents=True, exist_ok=True)
    runtime = out / "python-env"
    venv.EnvBuilder(with_pip=False, symlinks=False).create(runtime)
    executable = runtime / ("Scripts/python.exe" if sys.platform == "win32" else "bin/python")
    site = runtime / (
        "Lib/site-packages"
        if sys.platform == "win32"
        else f"lib/python{sys.version_info.major}.{sys.version_info.minor}/site-packages"
    )
    # A sandbox cannot read a managed interpreter under the user's home directory.
    # Copy the standard library beside the interpreter and remove the venv redirect.
    stdlib = Path(sysconfig.get_path("stdlib"))
    bundled_stdlib = runtime / (
        "Lib"
        if sys.platform == "win32"
        else f"lib/python{sys.version_info.major}.{sys.version_info.minor}"
    )
    shutil.copytree(
        stdlib,
        bundled_stdlib,
        dirs_exist_ok=True,
        ignore=shutil.ignore_patterns(
            "site-packages",
            "__pycache__",
            "*.pyc",
            "test",
            "tests",
            "ensurepip",
            "idlelib",
            "tkinter",
        ),
    )
    # Some distributions keep extension modules outside the pure standard library.
    dynload = Path(sysconfig.get_config_var("DESTSHARED") or stdlib / "lib-dynload")
    if sys.platform != "win32" and dynload.is_dir():
        shutil.copytree(dynload, bundled_stdlib / "lib-dynload", dirs_exist_ok=True)
    (runtime / "pyvenv.cfg").unlink()
    base = Path(sys.base_prefix)
    for candidate in (base / "lib").glob("libpython*"):
        if candidate.is_file():
            shutil.copyfile(candidate, runtime / "lib" / candidate.name)
    if sys.platform == "win32":
        # The venv launcher requires pyvenv.cfg; use the actual interpreter instead.
        shutil.copyfile(base / "python.exe", executable)
        for candidate in base.glob("*.dll"):
            shutil.copyfile(candidate, runtime / "Scripts" / candidate.name)
        if (base / "DLLs").is_dir():
            shutil.copytree(base / "DLLs", runtime / "DLLs", dirs_exist_ok=True)
    site.mkdir(parents=True, exist_ok=True)
    shutil.copytree(
        ROOT / "src" / MODULE, site / MODULE, ignore=shutil.ignore_patterns("__pycache__", "*.pyc")
    )
    # Validate installed RECORD receipts before copying unmodified dependency sources.
    receipt_count = 0
    for member in distribution.files or []:
        if not member.parts or member.parts[0] != "pypdf" or str(member).endswith(".pyc"):
            continue
        source = Path(distribution.locate_file(member))
        if not source.is_file():
            continue
        import base64

        receipt = member.hash
        digest = (
            base64.urlsafe_b64encode(hashlib.sha256(source.read_bytes()).digest())
            .decode()
            .rstrip("=")
        )
        if (
            not receipt
            or receipt.mode != "sha256"
            or receipt.value != digest
            or member.size != source.stat().st_size
        ):
            raise ValueError(
                "pypdf installed RECORD hash or size differs from reviewed distribution"
            )
        if source.suffix not in {".py", ".typed"} and source.name != "py.typed":
            raise ValueError("unexpected file in pure Python pypdf dependency")
        destination = site / str(member)
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, destination)
        receipt_count += 1
    if not receipt_count:
        raise ValueError("pypdf has no installed RECORD receipts")
    # Isolated Python does not consistently discover site-packages without a venv
    # redirect (for example with Debian or macOS framework builds).
    launcher = runtime / "run-engine.py"
    launcher.write_text(
        "import pathlib, runpy, sys\n"
        "root = pathlib.Path(__file__).resolve().parent\n"
        "stdlib = root / ('Lib' if sys.platform == 'win32' else "
        "f'lib/python{sys.version_info.major}.{sys.version_info.minor}')\n"
        "sys.path[:0] = [str(stdlib / 'site-packages'), str(stdlib), "
        "str(root / 'DLLs' if sys.platform == 'win32' else stdlib / 'lib-dynload')]\n"
        f"runpy.run_module({MODULE!r}, run_name='__main__', alter_sys=True)\n",
        encoding="utf-8",
    )
    shutil.copytree(ROOT / "pack/notices", out / "notices")
    for name in ("LICENSE", "NOTICE"):
        source = ROOT.parents[1] / name
        if source.is_file():
            shutil.copyfile(source, out / "notices" / ("AriadShift-" + name))
    if PACK_NAME == "inpaint":
        (out / "config").mkdir()
        shutil.copyfile(ROOT / "pack/florence-config.json", out / "config/florence.json")
    archive = out / "runtime.tar.gz"
    files = sorted(p for p in out.rglob("*") if p.is_file() and not p.is_symlink())
    with tarfile.open(archive, "w:gz") as tar:
        for path in files:
            tar.add(path, arcname=path.relative_to(out).as_posix(), recursive=False)
    notices = sorted(p.relative_to(out).as_posix() for p in (out / "notices").iterdir())

    def quote(value):
        return json.dumps(value, ensure_ascii=True)

    relative_program = executable.relative_to(out).as_posix()
    entrypoint = [relative_program, "-I", "-B", "{root}/python-env/run-engine.py"]
    manifest = [
        'schema = "ariad-pack/1"',
        f'name = "{PACK_NAME}"',
        'version = "0.1.0"',
        'description = "Offline document tools (unpacked development pack)"',
        'license = "Apache-2.0 AND MIT"',
        'requires_ashift = ">=0.2.0, <2.0.0"',
        'engine_protocol = "ariad-engine/2"',
        "notices = " + quote(notices),
        "",
        "[[targets]]",
        "triple = " + quote(host_triple()),
        "entrypoint = " + quote(entrypoint),
        "executables = " + quote([relative_program]),
        "",
        "[[targets.components]]",
        'name = "runtime"',
        'url = "runtime.tar.gz"',
        'sha256 = "' + hashlib.sha256(archive.read_bytes()).hexdigest() + '"',
        f"size = {archive.stat().st_size}",
        'unpack = "tar.gz"',
        'dest = "."',
        f"unpacked_size = {sum(p.stat().st_size for p in files)}",
        f"entries = {len(files)}",
    ]
    path = out / "pack.toml"
    path.write_text("\n".join(manifest) + "\n", encoding="utf-8")
    # Explicit assertion of the sandbox-required host resolution invariant.
    if not executable.resolve().is_relative_to(out) or not os.access(executable, os.X_OK):
        raise ValueError("development pack entrypoint escapes its root or is not executable")
    result = subprocess.run(
        [str(executable), *(arg.replace("{root}", str(out)) for arg in entrypoint[1:])],
        input=json.dumps({"protocol": "ariad-engine/2", "job": "self-check", "op": "describe"})
        + "\n",
        cwd=out.parent,
        capture_output=True,
        text=True,
        check=False,
        timeout=15,
    )
    if result.returncode:
        raise ValueError("development pack self-check failed: " + result.stdout + result.stderr)
    events = [json.loads(line) for line in result.stdout.splitlines()]
    if not events or events[0].get("engine") != PACK_NAME or events[-1].get("ok") is not True:
        raise ValueError("development pack self-check did not report success")
    return path


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    command = sub.add_parser("build")
    command.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    print(build(args.out))


if __name__ == "__main__":
    main()
