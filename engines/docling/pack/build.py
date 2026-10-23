"""Build the docling engine pack for one target.

The pack is laid out as::

    runtime/   python-build-standalone CPython with the locked wheels and ariad_docling
    notices/   licence files of every bundled component
    models/    Docling layout, TableFormer and RapidOCR models (artifacts_path)
    tessdata/  tessdata_best vie, eng and osd with configs/tsv (TESSDATA_PREFIX)

and shipped as three ``tar.zst`` components (``runtime`` per target, ``models`` and
``tessdata`` shared) plus an unsigned ``pack.toml`` written by ``ashift __pack manifest``.
Signing and publishing happen elsewhere (docs/packs.md).

Usage, from the repository root::

    python engines/docling/pack/build.py build --target TRIPLE --out DIR [--ashift PATH]
    python engines/docling/pack/build.py verify --out DIR --input FILE --expect TEXT \\
        [--offline] [--ashift PATH]
    python engines/docling/pack/build.py manifest --out DIR [--ashift PATH] TRIPLE...

``build`` needs ``uv`` on PATH and Python 3.14 (``tarfile`` writes zstd archives there).
``verify`` unpacks the components into a fresh directory and converts a document through
``ashift`` and the unpacked pack. ``manifest`` merges the targets built into ``DIR`` into one
``pack.toml``.
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import shutil
import subprocess
import sys
import tarfile
import tempfile
import tomllib
from dataclasses import asdict, dataclass
from importlib import metadata
from pathlib import Path

import fetch
import licenses

PACK_DIR = Path(__file__).resolve().parent
ENGINE_DIR = PACK_DIR.parent
REPOSITORY = ENGINE_DIR.parents[1]
PYTORCH_INDEX = "https://download.pytorch.org/whl/cpu"
PACK_NAME = "docling"
REQUIRES_ASHIFT = ">=0.2.0, <0.3.0"
DESCRIPTION = "Docling: PDF, scanned documents, Office files and images, with OCR"
NOTICES = (
    "notices/AriadShift-LICENSE",
    "notices/AriadShift-NOTICE",
    "notices/python-distributions.txt",
    "notices/models-and-data.txt",
    "notices/runtime-libraries.txt",
)
MAX_COMPONENT_BYTES = 2 * 1024**3
PYTHON_VERSION = "3.14"
# docling-parse ships macOS wheels for 14.0 and later only; uv would otherwise assume 13.0.
MACOS_DEPLOYMENT_TARGET = "14.0"

# uv's --python-platform name for each pack target.
PLATFORMS = {
    "x86_64-unknown-linux-gnu": "x86_64-manylinux_2_28",
    "aarch64-unknown-linux-gnu": "aarch64-manylinux_2_28",
    "aarch64-apple-darwin": "aarch64-apple-darwin",
    "x86_64-pc-windows-msvc": "x86_64-pc-windows-msvc",
}


@dataclass(frozen=True)
class TargetBuild:
    """The files one target build leaves in the output directory."""

    triple: str
    version: str
    runtime: str
    models: str
    tessdata: str
    license: str

    @property
    def files(self) -> dict[str, str]:
        return {"runtime": self.runtime, "models": self.models, "tessdata": self.tessdata}


def engine_version() -> str:
    with (ENGINE_DIR / "pyproject.toml").open("rb") as handle:
        return tomllib.load(handle)["project"]["version"]


def is_windows(target: str) -> bool:
    return target.endswith("windows-msvc")


def python_executable(runtime: Path, target: str) -> Path:
    return runtime / ("python.exe" if is_windows(target) else "bin/python3")


def library_root(runtime: Path, target: str) -> Path:
    """The standard library directory, which holds ``site-packages``."""
    if is_windows(target):
        return runtime / "Lib"
    return runtime / "lib" / f"python{PYTHON_VERSION}"


def site_packages(runtime: Path, target: str) -> Path:
    return library_root(runtime, target) / "site-packages"


def host_target() -> str | None:
    machine = platform.machine().lower()
    if sys.platform.startswith("linux"):
        return {"x86_64": "x86_64-unknown-linux-gnu", "aarch64": "aarch64-unknown-linux-gnu"}.get(
            machine
        )
    if sys.platform == "darwin" and machine == "arm64":
        return "aarch64-apple-darwin"
    if sys.platform == "win32" and machine in ("amd64", "x86_64"):
        return "x86_64-pc-windows-msvc"
    return None


def run(command: list[str | Path], **kwargs) -> None:
    print("+", " ".join(str(part) for part in command), flush=True)
    subprocess.run([str(part) for part in command], check=True, **kwargs)


def unpack_runtime(archive: Path, root: Path) -> Path:
    with tempfile.TemporaryDirectory(dir=root) as scratch:
        with tarfile.open(archive, "r:gz") as tar:
            tar.extractall(scratch, filter="data")
        runtime = root / "runtime"
        shutil.move(Path(scratch) / "python", runtime)
    return runtime


def prune_runtime(runtime: Path, target: str) -> None:
    """Remove unused GUI components and standard-library tests before installing wheels."""
    library = library_root(runtime, target)
    directories = [runtime / "tcl", library / "tkinter", library / "idlelib"]
    directories.extend(path for path in library.rglob("*") if path.name in {"test", "tests"})
    native_directories = [runtime / "DLLs", runtime, runtime / "bin"]
    # Windows aliases lib to the stdlib's Lib; scanning it would also remove threading.py.
    if not is_windows(target):
        native_directories.append(runtime / "lib")
    for directory in native_directories:
        if directory.is_dir():
            directories.extend(
                path
                for path in directory.iterdir()
                if path.name.lower().startswith(
                    ("tcl", "tk", "itcl", "tix", "thread", "libtcl", "libtk", "_tkinter", "idle")
                )
            )
    directories.extend((library / "lib-dynload").glob("_tkinter*"))
    for path in sorted(set(directories), key=lambda item: len(item.parts), reverse=True):
        if path.is_symlink() or path.is_file():
            path.unlink()
        elif path.is_dir():
            shutil.rmtree(path)


def install_wheels(work: Path, runtime: Path, target: str) -> None:
    requirements = work / "requirements.txt"
    run(
        [
            "uv",
            "export",
            "--package",
            "ariad-docling",
            "--frozen",
            "--no-dev",
            "--no-emit-workspace",
            "--no-header",
            "--format",
            "requirements.txt",
            "--output-file",
            requirements,
        ],
        cwd=REPOSITORY,
    )
    platform_args = ["--python-platform", PLATFORMS[target], "--python-version", PYTHON_VERSION]
    env = dict(os.environ)
    if target.endswith("apple-darwin"):
        env["MACOSX_DEPLOYMENT_TARGET"] = MACOS_DEPLOYMENT_TARGET
    site = site_packages(runtime, target)
    # Every wheel is pinned by hash, so PyPI and the CPU-only PyTorch index can be mixed safely.
    run(
        [
            "uv",
            "pip",
            "install",
            "--target",
            site,
            *platform_args,
            "--require-hashes",
            "--no-deps",
            "--only-binary",
            ":all:",
            "--link-mode",
            "copy",
            "--extra-index-url",
            PYTORCH_INDEX,
            "--index-strategy",
            "unsafe-best-match",
            "--requirements",
            requirements,
        ],
        cwd=REPOSITORY,
        env=env,
    )
    dist = work / "dist"
    run(
        ["uv", "build", "--package", "ariad-docling", "--wheel", "--out-dir", dist],
        cwd=REPOSITORY,
    )
    wheel = next(dist.glob("ariad_docling-*.whl"))
    run(
        [
            "uv",
            "pip",
            "install",
            "--target",
            site,
            *platform_args,
            "--no-deps",
            "--link-mode",
            "copy",
            wheel,
        ],
        cwd=REPOSITORY,
        env=env,
    )
    # `--target` installs console scripts into a bin directory the pack never uses.
    shutil.rmtree(site / "bin", ignore_errors=True)
    drop_build_records(site)


def drop_build_records(site: Path) -> None:
    """Removes the installer's records of where and when each wheel was installed.

    ``direct_url.json`` holds the absolute path of the wheel and ``uv_cache.json`` the time of
    the install. Removed console scripts have interpreter-dependent shebangs, so their RECORD
    entries must go too, even though the scripts themselves are no longer in the pack.
    """
    for info in site.glob("*.dist-info"):
        dropped = [name for name in ("direct_url.json", "uv_cache.json") if (info / name).is_file()]
        for name in dropped:
            (info / name).unlink()
        record = info / "RECORD"
        if record.is_file():
            entries = ("bin/", *(f"{info.name}/{name}," for name in dropped))
            lines = record.read_text(encoding="utf-8").splitlines()
            kept = [line for line in lines if not line.startswith(entries)]
            record.write_text("\n".join(kept) + "\n", encoding="utf-8")


def compile_bytecode(runtime: Path, target: str) -> None:
    """Byte-compiles the standard library and the packages with hash-based pycs.

    The read-only install cannot write caches, so everything is compiled here. The recorded file
    names are relative to the library directory, so the bytecode does not depend on the
    directory the pack was built in. One process compiles the files in a fixed order with a fixed
    hash seed, because marshal output depends on what the process has already interned, and
    ``-B`` keeps the interpreter from adding caches that do record the build directory.
    """
    root = library_root(runtime, target)
    env = {"PYTHONHASHSEED": "0", "PATH": os.environ.get("PATH", "")}
    if is_windows(target):
        env["SYSTEMROOT"] = os.environ.get("SYSTEMROOT", r"C:\Windows")
    run(
        [
            python_executable(runtime, target),
            "-s",
            "-P",
            "-B",
            "-W",
            "ignore",
            "-m",
            "compileall",
            "-q",
            "-j",
            "1",
            "--invalidation-mode",
            "unchecked-hash",
            "-s",
            root,
            "-p",
            "library",
            root,
        ],
        env=env,
    )


def write_notices(root: Path, runtime: Path, target: str, pins: dict) -> str:
    site = site_packages(runtime, target)
    distributions = list(metadata.distributions(path=[str(site)]))
    libraries = licenses.runtime_libraries(pins["python"]["release"], target)
    expression = licenses.pack_expression(
        distributions,
        [
            "Apache-2.0",
            pins["python"]["license"],
            pins["tessdata"]["license"],
            *(model["license"] for model in pins["models"]),
            *(item["license"] for item in libraries),
        ],
    )
    notices = root / "notices"
    licenses.write_notices(distributions, notices)
    licenses.write_runtime_notices(notices, libraries)

    for name in ("LICENSE", "NOTICE"):
        shutil.copyfile(REPOSITORY / name, notices / f"AriadShift-{name}")
    python_licences = (
        runtime / "lib" / f"python{PYTHON_VERSION}" / "LICENSE.txt",
        runtime / "LICENSE.txt",
    )
    for candidate in python_licences:
        if candidate.is_file():
            shutil.copyfile(candidate, notices / "Python-LICENSE.txt")
            break
    else:
        raise SystemExit("the standalone Python runtime has no LICENSE.txt")
    texts = {
        "Apache-2.0": REPOSITORY / "LICENSE",
        "CDLA-Permissive-2.0": PACK_DIR / "licenses" / "CDLA-Permissive-2.0.txt",
    }
    lines = ["Models and data bundled in this pack:", ""]
    for item in [*pins["models"], {"name": "tessdata_best", **pins["tessdata"]}]:
        if item["license"] not in licenses.ALLOWED_DATA:
            raise SystemExit(f"{item['name']}: licence {item['license']} is not allowed")
        lines.append(f"{item['name']}: {item['license']}, {item['source']} at {item['revision']}")
        source = texts.get(item["license"])
        if source is not None:
            shutil.copyfile(source, notices / f"{item['license']}.txt")
    lines.append(
        f"python-build-standalone CPython {pins['python']['version']}: "
        f"{pins['python']['license']}, {pins['python']['source']} {pins['python']['release']}"
    )
    (notices / "models-and-data.txt").write_text("\n".join(lines) + "\n", encoding="utf-8")
    return expression


def smoke(runtime: Path, target: str) -> None:
    """Asks the freshly built engine to describe itself with a cleared environment."""
    env = {"PATH": os.environ.get("PATH", "")}
    if is_windows(target):
        env["SYSTEMROOT"] = os.environ.get("SYSTEMROOT", r"C:\Windows")
    request = b'{"protocol":"ariad-engine/1","job":"pack-smoke","op":"describe"}\n'
    completed = subprocess.run(
        [str(python_executable(runtime, target)), "-I", "-B", "-m", "ariad_docling"],
        input=request,
        capture_output=True,
        env=env,
        check=False,
        timeout=120,
    )
    lines = completed.stdout.decode("ascii").splitlines()
    if completed.returncode != 0 or not lines:
        raise SystemExit(f"engine smoke test failed:\n{completed.stderr.decode(errors='replace')}")
    capabilities = json.loads(lines[0])
    expected = {"docling-layout-heron", "tableformer", "rapidocr", "tessdata-vie", "tessdata-eng"}
    missing = expected - set(capabilities.get("models", []))
    if missing:
        raise SystemExit(f"engine smoke test: missing assets {sorted(missing)}")
    print(f"engine describes itself as Docling {capabilities['version']}")

    # describe imports no third-party code. Run a real request too; a timed traceback
    # makes native library, model and OCR initialization hangs visible in CI.
    probe = """
import faulthandler, runpy
faulthandler.dump_traceback_later(60, repeat=True)
runpy.run_module('ariad_docling', run_name='__main__')
"""
    with tempfile.TemporaryDirectory(prefix="docling-smoke-") as directory:
        work = Path(directory)
        (work / "out").mkdir()
        (work / "tmp").mkdir()
        request = {
            "protocol": "ariad-engine/1",
            "job": "pack-pdf-smoke",
            "op": "convert",
            "input": {"path": str(REPOSITORY / "fixtures/pdf/en-map-room.pdf"), "format": "pdf"},
            "output": {"dir": str(work / "out"), "format": "docling+json"},
            "work_dir": str(work / "tmp"),
            "options": {},
            "limits": {"max_nesting_depth": 64},
        }
        try:
            converted = subprocess.run(
                [str(python_executable(runtime, target)), "-I", "-B", "-c", probe],
                input=json.dumps(request).encode() + b"\n",
                capture_output=True,
                env=env,
                check=False,
                timeout=180,
            )
        except subprocess.TimeoutExpired as error:
            detail = (error.stderr or b"").decode(errors="replace")
            raise SystemExit(f"PDF runtime conversion timed out:\n{detail}") from error
        events = [json.loads(line) for line in converted.stdout.splitlines()]
        if converted.returncode or not events or not events[-1].get("ok"):
            raise SystemExit(
                f"PDF runtime conversion failed:\n{converted.stderr.decode(errors='replace')}"
                f"\n{events[-1:]!r}"
            )
    print("PDF runtime converted the smoke fixture", flush=True)


def archive(root: Path, members: list[str], destination: Path) -> str:
    """Writes a reproducible tar.zst of ``members`` and returns its file name."""
    epoch = int(os.environ.get("SOURCE_DATE_EPOCH", "0"))

    def normalize(info: tarfile.TarInfo) -> tarfile.TarInfo:
        info.uid = info.gid = 0
        info.uname = info.gname = ""
        info.mtime = epoch
        if info.isfile():
            info.mode = 0o755 if info.mode & 0o111 else 0o644
        elif info.isdir():
            info.mode = 0o755
        return info

    with tarfile.open(destination, "w:zst", level=10) as tar:
        for member in members:
            base = root / member
            paths = [base, *sorted(base.rglob("*"))]
            for path in paths:
                tar.add(
                    path,
                    arcname=path.relative_to(root).as_posix(),
                    recursive=False,
                    filter=normalize,
                )
    size = destination.stat().st_size
    if size >= MAX_COMPONENT_BYTES:
        raise SystemExit(f"{destination.name} is {size} bytes; components must stay under 2 GiB")
    return destination.name


def build(target: str, out: Path, cache: Path, ashift: str) -> TargetBuild:
    if target not in PLATFORMS:
        raise SystemExit(f"unsupported target {target}; known: {', '.join(PLATFORMS)}")
    version = engine_version()
    pins = fetch.load_pins()
    work = out / "work" / target
    shutil.rmtree(work, ignore_errors=True)
    root = work / "root"
    root.mkdir(parents=True)

    runtime = unpack_runtime(fetch.fetch(fetch.python_archive(pins, target), cache), root)
    prune_runtime(runtime, target)
    install_wheels(work, runtime, target)
    native = host_target() == target
    if native:
        compile_bytecode(runtime, target)
    expression = write_notices(root, runtime, target, pins)
    fetch.install_assets(pins, root, cache, copy=True)
    if native:
        smoke(runtime, target)

    out.mkdir(parents=True, exist_ok=True)
    record = TargetBuild(
        triple=target,
        license=expression,
        version=version,
        runtime=archive(
            root, ["runtime", "notices"], out / f"docling-{version}-{target}-runtime.tar.zst"
        ),
        models=archive(root, ["models"], out / f"docling-{version}-models.tar.zst"),
        tessdata=archive(root, ["tessdata"], out / f"docling-{version}-tessdata.tar.zst"),
    )
    (out / f"build-{target}.json").write_text(json.dumps(asdict(record), indent=2) + "\n", "utf-8")
    shutil.rmtree(work)
    write_manifest(ashift, out, [record])
    return record


def entrypoint(target: str) -> list[str]:
    return [python_executable(Path("runtime"), target).as_posix(), "-I", "-m", "ariad_docling"]


def _toml(value: str) -> str:
    return json.dumps(value, ensure_ascii=False)


def pack_fields(version: str, expression: str) -> str:
    notices = ", ".join(_toml(notice) for notice in NOTICES)
    return "\n".join(
        [
            'schema = "ariad-pack/1"',
            f"name = {_toml(PACK_NAME)}",
            f"version = {_toml(version)}",
            f"description = {_toml(DESCRIPTION)}",
            f"license = {_toml(expression)}",
            f"requires_ashift = {_toml(REQUIRES_ASHIFT)}",
            f"notices = [{notices}]",
            "",
        ]
    )


def target_block(target: str) -> str:
    """The target's entrypoint and default options, without components."""
    argv = ", ".join(_toml(part) for part in entrypoint(target))
    return "\n".join(
        [
            "",
            "[[targets]]",
            f"triple = {_toml(target)}",
            f"entrypoint = [{argv}]",
            'options = { artifacts_path = "{root}/models", tessdata_path = "{root}/tessdata" }',
            "",
        ]
    )


def write_manifest(ashift: str, out: Path, builds: list[TargetBuild]) -> None:
    """Writes the unsigned ``ariad-pack/1`` manifest of ``builds`` to ``out/pack.toml``.

    ``ashift __pack manifest`` hashes the component files and declares the size and entry count
    of each archive. A target is added by passing the previous output as ``--base``, with the
    target's own entrypoint and options appended to it.
    """
    versions = {build.version for build in builds}
    if len(versions) != 1:
        raise SystemExit(f"targets come from different pack versions: {sorted(versions)}")
    version = versions.pop()
    manifest = out / "pack.toml"
    base_text = pack_fields(
        version, licenses.combined_expression(build.license for build in builds)
    )
    for build in sorted(builds, key=lambda item: item.triple):
        base = out / "pack.base.toml"
        base.write_text(base_text + target_block(build.triple), encoding="utf-8")
        command = [
            ashift,
            "__pack",
            "manifest",
            "--name",
            PACK_NAME,
            "--version",
            version,
            "--target",
            build.triple,
            "--base",
            base,
            "--output",
            manifest,
            *(f"{name}={out / file}" for name, file in build.files.items()),
        ]
        run(command)
        base_text = manifest.read_text(encoding="utf-8")
    (out / "pack.base.toml").unlink(missing_ok=True)


def load_build(out: Path, target: str) -> TargetBuild:
    return TargetBuild(**json.loads((out / f"build-{target}.json").read_text(encoding="utf-8")))


def verify(ashift: str, out: Path, target: str, source: Path, expect: str, offline: bool) -> None:
    """Unpacks the pack into a clean directory and converts ``source`` through ``ashift``.

    The unpacked directory is used as the development pack ``ASHIFT_PACK_DOCLING``, so the
    manifest's entrypoint and options, the engine, the models and the OCR data all take part.
    With ``offline`` the conversion runs in a new network namespace (``unshare -rn``, Linux
    only), which proves the pack needs no downloads.
    """
    build = load_build(out, target)
    with tempfile.TemporaryDirectory(prefix="docling-pack-") as directory:
        root = Path(directory) / "pack"
        root.mkdir()
        for file in build.files.values():
            with tarfile.open(out / file, "r:zst") as tar:
                tar.extractall(root, filter="data")
        shutil.copyfile(out / "pack.toml", root / "pack.toml")
        result = Path(directory) / f"converted{source.suffix}.md"
        env = {
            "PATH": os.environ.get("PATH", ""),
            "ASHIFT_PACK_DOCLING": str(root),
            "ASHIFT_DATA_DIR": str(Path(directory) / "data"),
            "HOME": directory,
        }
        if is_windows(target):
            env["SYSTEMROOT"] = os.environ.get("SYSTEMROOT", r"C:\Windows")
            env["USERPROFILE"] = directory
        prefix = ["unshare", "-rn"] if offline else []
        # Exercise discovery and planning separately so a stalled host round-trip identifies
        # its stage rather than consuming the conversion timeout without diagnostics.
        for args in (["engines", "--json"], ["plan", str(source), "--to", "md", "--json"]):
            command = [*prefix, ashift, *args]
            print("+", " ".join(command), flush=True)
            try:
                checked = subprocess.run(
                    command, capture_output=True, env=env, check=False, timeout=120
                )
            except subprocess.TimeoutExpired as error:
                raise SystemExit(f"pack host check timed out: {args[0]}") from error
            print(checked.stdout.decode(errors="replace"), flush=True)
            if checked.returncode:
                raise SystemExit(checked.stderr.decode(errors="replace")[-4000:])
        command = [*prefix, ashift, "convert", source, "--to", "md", "-o", result]
        print("+", " ".join(str(part) for part in command), flush=True)
        completed = subprocess.run(
            [str(part) for part in command],
            capture_output=True,
            env=env,
            check=False,
            timeout=1800,
        )
        if completed.returncode != 0 or not result.is_file():
            stderr = completed.stderr.decode(errors="replace")[-4000:]
            raise SystemExit(f"pack conversion failed ({completed.returncode}):\n{stderr}")
        text = result.read_text(encoding="utf-8")
        if expect not in text:
            raise SystemExit(
                f"pack conversion did not produce the expected text {expect!r}:\n{text}"
            )
        print(f"pack converted {source.name}: {len(text)} characters of Markdown")


def default_ashift() -> str:
    return os.environ.get("ASHIFT_BIN", "ashift")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    commands = parser.add_subparsers(dest="command", required=True)
    build_parser = commands.add_parser("build")
    build_parser.add_argument("--target", default=host_target())
    build_parser.add_argument("--out", type=Path, required=True)
    build_parser.add_argument("--cache", type=Path, default=fetch.default_cache())
    build_parser.add_argument("--ashift", default=default_ashift())
    verify_parser = commands.add_parser("verify")
    verify_parser.add_argument("--target", default=host_target())
    verify_parser.add_argument("--out", type=Path, required=True)
    verify_parser.add_argument("--input", type=Path, required=True)
    verify_parser.add_argument("--expect", required=True)
    verify_parser.add_argument("--offline", action="store_true")
    verify_parser.add_argument("--ashift", default=default_ashift())
    manifest_parser = commands.add_parser("manifest")
    manifest_parser.add_argument("--out", type=Path, required=True)
    manifest_parser.add_argument("--ashift", default=default_ashift())
    manifest_parser.add_argument("targets", nargs="+", choices=sorted(PLATFORMS))
    args = parser.parse_args(argv)

    if args.command == "build":
        if args.target is None:
            raise SystemExit("this host has no pack target; pass --target")
        record = build(args.target, args.out.resolve(), args.cache, args.ashift)
        print(f"built the {record.triple} pack in {args.out}")
    elif args.command == "verify":
        if args.target is None:
            raise SystemExit("this host has no pack target; pass --target")
        verify(
            args.ashift,
            args.out.resolve(),
            args.target,
            args.input.resolve(),
            args.expect,
            args.offline,
        )
    else:
        out = args.out.resolve()
        write_manifest(args.ashift, out, [load_build(out, target) for target in args.targets])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
