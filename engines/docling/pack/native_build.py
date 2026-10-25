"""Build the pack's OpenCV and Windows OpenMP replacements from verified sources."""

from __future__ import annotations

import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tarfile
import tomllib
from pathlib import Path, PurePosixPath

import fetch

PACK = Path(__file__).resolve().parent
REPOSITORY = PACK.parents[2]
# These options also form part of the native artifact cache key. Keep optional
# device SDKs and vendor binary downloads out of this document-processing build.
OPENCV_FLAGS = (
    "-DWITH_IPP=OFF",
    "-DWITH_ITT=OFF",
    "-DWITH_OPENVINO=OFF",
    "-DWITH_CUDA=OFF",
    "-DWITH_CUDNN=OFF",
    "-DWITH_NVCUVID=OFF",
    "-DWITH_NVCUVENC=OFF",
    "-DWITH_OPENCL=OFF",
    "-DWITH_MFX=OFF",
    "-DWITH_OBSENSOR=OFF",
    "-DWITH_MSMF=OFF",
    "-DWITH_FFMPEG=OFF",
    "-DWITH_GSTREAMER=OFF",
    "-DWITH_V4L=OFF",
    "-DWITH_LAPACK=OFF",
    "-DWITH_TBB=OFF",
    "-DBUILD_TESTS=OFF",
    "-DBUILD_PERF_TESTS=OFF",
    "-DBUILD_EXAMPLES=OFF",
    "-DBUILD_JAVA=OFF",
    "-DCMAKE_BUILD_TYPE=Release",
)
LLVM_FLAGS = (
    "-DLLVM_ENABLE_RUNTIMES=openmp",
    "-DCMAKE_BUILD_TYPE=Release",
    "-DLIBOMP_LIB_NAME=libiomp5md",
    "-DOPENMP_MSVC_NAME_SCHEME=OFF",
    "-DLIBOMP_OMPT_SUPPORT=OFF",
    "-DLIBOMP_OMPD_SUPPORT=OFF",
    "-DLIBOMP_USE_ITT_NOTIFY=OFF",
    "-DOPENMP_ENABLE_LIBOMPTARGET=OFF",
    "-DLIBOMP_ENABLE_SHARED=ON",
)
IPP_HEADING = (
    "Intel(R) IPP ICV library statically linked within x86 and x86_64 opencv-python packages."
)
NOTICE_SEPARATOR = "------------------------------------------------------------------------------"


def check_opencv_pin(pins):
    with (REPOSITORY / "uv.lock").open("rb") as handle:
        locked = next(
            p for p in tomllib.load(handle)["package"] if p["name"] == "opencv-python-headless"
        )
    pin = pins["opencv"]
    sdist = locked["sdist"]
    if (locked["version"], sdist["url"], sdist["hash"], sdist["size"]) != (
        pin["version"],
        pin["url"],
        f"sha256:{pin['sha256']}",
        pin["size"],
    ):
        raise SystemExit("review the OpenCV source pin together with the locked dependency")


def run(command, **kwargs):
    print("+", " ".join(map(str, command)), flush=True)
    subprocess.run(list(map(str, command)), check=True, **kwargs)


def tools(*args):
    return [
        "uv",
        "run",
        "--project",
        REPOSITORY,
        "--package",
        "ariad-docling",
        "--group",
        "torch-build",
        "--frozen",
        "--",
        *args,
    ]


def source(pin, destination, cache, *, components=None):
    blob = fetch.fetch(
        fetch.Pinned(
            pin["url"], pin["sha256"], pin["size"], PurePosixPath(pin["url"].rsplit("/", 1)[-1])
        ),
        cache,
    )
    with tarfile.open(blob) as archive:
        members = [
            m
            for m in archive
            if components is None
            or (
                len(PurePosixPath(m.name).parts) > 1
                and PurePosixPath(m.name).parts[1] in components
            )
        ]
        archive.extractall(destination, members=members, filter="data")
    roots = list(destination.iterdir())
    if len(roots) != 1:
        raise SystemExit(f"expected one source root in {destination}")
    return roots[0]


def cache_key(kind, target, pins):
    inputs = {
        "pin": pins[kind],
        "target": target,
        "python": "3.14",
        "flags": OPENCV_FLAGS if kind == "opencv" else LLVM_FLAGS,
        "manylinux": pins["manylinux"] if target.endswith("linux-gnu") else None,
    }
    digest = hashlib.sha256(json.dumps(inputs, sort_keys=True).encode())
    sources = [Path(__file__), REPOSITORY / "uv.lock"]
    if kind == "llvm":
        sources.append(PACK / "vcomp-compat.cpp")
        sources.append(PACK / "vcomp-probe.cpp")
    for path in sources:
        digest.update(path.read_bytes())
    return digest.hexdigest()


def cached_build(kind, target, pins, cache, builder):
    destination = cache / "native" / f"{kind}-{cache_key(kind, target, pins)}"
    record = destination / "build.json"
    if record.is_file():
        files = json.loads(record.read_text("utf-8"))
        actual = {path.name for path in destination.iterdir() if path.is_file() and path != record}
        if (
            files
            and set(files) == actual
            and all(
                (destination / n).is_file() and fetch._sha256(destination / n) == h
                for n, h in files.items()
            )
        ):
            return destination
    shutil.rmtree(destination, ignore_errors=True)
    destination.mkdir(parents=True)
    scratch = destination / "work"
    scratch.mkdir()
    builder(scratch, destination)
    shutil.rmtree(scratch)
    files = {p.name: fetch._sha256(p) for p in destination.iterdir() if p.is_file()}
    record.write_text(json.dumps(files, sort_keys=True) + "\n", "utf-8")
    return destination


def release_files(kind, target, pins, cache, destination, build_key):
    """Fetch reviewed release outputs, or return None while bootstrapping.

    A configured pin is authoritative: stale inputs, bad hashes and download
    errors must fail rather than silently switching to a different build.
    """
    toolchain = pins.get("toolchain", {})
    entry = toolchain.get("targets", {}).get(target, {}).get(kind)
    if entry is None:
        print(f"No pinned {kind} release for {target}; building from source", flush=True)
        return None
    release = toolchain.get("release", "")
    if not re.fullmatch(r"toolchain-wheels-[A-Za-z0-9][A-Za-z0-9._-]*", release):
        raise SystemExit("toolchain release must be named toolchain-wheels-<version>")
    if entry.get("build_key") != build_key:
        raise SystemExit(f"rebuild and repin {kind} wheels after changing source build inputs")
    prefix = f"https://github.com/bavanchun/AriadShift/releases/download/{release}/"
    files = {}
    for pin in entry.get("files", []):
        name = pin["name"]
        if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._+-]*", name) or name in files:
            raise SystemExit("toolchain assets must have unique single file names")
        url = pin["url"]
        asset = url.removeprefix(prefix)
        if not url.startswith(prefix) or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._+-]*", asset):
            raise SystemExit("toolchain assets must come from the pinned repository release")
        if not re.fullmatch(r"[a-f0-9]{64}", pin["sha256"]) or pin["size"] <= 0:
            raise SystemExit("toolchain assets require a SHA-256 and positive size")
        blob = fetch.fetch(
            fetch.Pinned(url, pin["sha256"], pin["size"], PurePosixPath(name)), cache
        )
        destination.mkdir(parents=True, exist_ok=True)
        path = destination / name
        shutil.copyfile(blob, path)
        files[name] = path
    if not files:
        raise SystemExit(f"pinned {kind} toolchain has no files")
    return files


def remove_inapplicable_ipp_notice(path: Path) -> None:
    """Remove only the reviewed IPP section, after proving IPP is absent.

    Upstream's common notice includes this x86-only section even in ARM wheels
    and source builds with WITH_IPP=OFF. Preserve every other upstream notice.
    """
    text = path.read_text("utf-8")
    sections = text.split(NOTICE_SEPARATOR)
    matches = [section for section in sections if section.strip().startswith(IPP_HEADING)]
    if len(matches) != 1 or "Intel Simplified Software License" not in matches[0]:
        raise SystemExit(f"unreviewed OpenCV IPP notice layout: {path}")
    path.write_text(NOTICE_SEPARATOR.join(s for s in sections if s != matches[0]), "utf-8")


def opencv(target, pins, cache):
    key = cache_key("opencv", target, pins)
    released = release_files(
        "opencv", target, pins, cache, cache / "native" / f"opencv-release-{key}", key
    )
    if released is not None:
        wheels = [path for path in released.values() if path.name.endswith(".whl")]
        if len(wheels) != 1 or not wheels[0].name.startswith(
            f"opencv_python_headless-{pins['opencv']['version']}-"
        ):
            raise SystemExit("pinned OpenCV toolchain must contain exactly one headless wheel")
        return wheels[0]

    def build(work, destination):
        src = source(pins["opencv"], work / "source", cache)
        if target.endswith("windows-msvc"):
            # The upstream packager otherwise requires its downloaded FFmpeg
            # plugin even when OpenCV was explicitly built without FFmpeg.
            setup = src / "setup.py"
            text, count = re.subn(
                r'\[r"bin/opencv_videoio_ffmpeg[^\n]+\]\s*if os\.name == "nt"\s*else \[\]',
                "[]",
                setup.read_text("utf-8"),
            )
            if count != 1:
                raise SystemExit("unreviewed OpenCV Windows FFmpeg packaging rule")
            setup.write_text(text, "utf-8")
        env = dict(
            os.environ,
            ENABLE_HEADLESS="1",
            ENABLE_CONTRIB="0",
            CMAKE_ARGS=" ".join(OPENCV_FLAGS),
            CMAKE_BUILD_PARALLEL_LEVEL="3",
            SOURCE_DATE_EPOCH="0",
        )
        if target.endswith("windows-msvc"):
            env.update(CMAKE_GENERATOR="Ninja", CC="cl", CXX="cl")
        command = tools("python", src / "setup.py", "bdist_wheel", "--dist-dir", destination)
        if target.endswith("linux-gnu"):
            # Preserve the existing glibc 2.28 floor instead of linking against
            # the CI runner's newer libc and optional host image libraries.
            uv = Path(shutil.which("uv")).resolve()
            command = tools("python", Path(__file__), "--source", src, "--out", destination)
            command = [
                "docker",
                "run",
                "--rm",
                "--name",
                f"ashift-opencv-{os.getpid()}",
                "--user",
                f"{os.getuid()}:{os.getgid()}",
                "--volume",
                f"{REPOSITORY}:{REPOSITORY}",
                "--volume",
                f"{cache.resolve()}:{cache.resolve()}",
                "--volume",
                f"{uv}:/usr/local/bin/uv:ro",
                "--workdir",
                REPOSITORY,
                "--env",
                "UV_PROJECT_ENVIRONMENT=/tmp/ashift-opencv-env",
                "--env",
                "UV_PYTHON=/opt/python/cp314-cp314/bin/python",
                "--env",
                "UV_PYTHON_DOWNLOADS=never",
                "--env",
                "UV_CACHE_DIR=/tmp/uv-cache",
                "--env",
                "HOME=/tmp",
                *[
                    arg
                    for key in (
                        "ENABLE_HEADLESS",
                        "ENABLE_CONTRIB",
                        "CMAKE_ARGS",
                        "CMAKE_BUILD_PARALLEL_LEVEL",
                        "SOURCE_DATE_EPOCH",
                    )
                    for arg in ("--env", f"{key}={env[key]}")
                ],
                pins["manylinux"]["image"],
                *command,
            ]
        run(command, cwd=REPOSITORY, env=env)
        wheels = list(destination.glob("*.whl"))
        if len(wheels) != 1:
            raise SystemExit("OpenCV source build did not produce exactly one wheel")

    return next(cached_build("opencv", target, pins, cache, build).glob("*.whl"))


def llvm_openmp(target, pins, cache):
    def build(work, destination):
        src = source(
            pins["llvm"],
            work / "source",
            cache,
            components={"openmp", "cmake", "llvm", "runtimes", "third-party"},
        )
        cmake = src / "openmp/runtime/src/CMakeLists.txt"
        with cmake.open("a", encoding="utf-8") as handle:
            handle.write(
                f'\ntarget_sources(omp PRIVATE "{(PACK / "vcomp-compat.cpp").as_posix()}")\n'
                f'add_executable(vcomp-probe "{(PACK / "vcomp-probe.cpp").as_posix()}")\n'
                "target_compile_options(vcomp-probe PRIVATE /openmp)\n"
                "target_link_options(vcomp-probe PRIVATE /NODEFAULTLIB:vcomp)\n"
                "target_link_libraries(vcomp-probe PRIVATE omp)\n"
            )
        run(
            tools(
                "cmake",
                "-S",
                src / "runtimes",
                "-B",
                work / "build",
                "-G",
                "Ninja",
                "-DCMAKE_C_COMPILER=cl",
                "-DCMAKE_CXX_COMPILER=cl",
                *LLVM_FLAGS,
            )
        )
        run(
            tools(
                "cmake",
                "--build",
                work / "build",
                "--config",
                "Release",
                "--target",
                "omp",
                "vcomp-probe",
                "--parallel",
                "3",
            )
        )
        dlls = list((work / "build").rglob("libiomp5md.dll"))
        if len(dlls) != 1:
            raise SystemExit(f"expected one LLVM OpenMP DLL, found {dlls}")
        executable = next((work / "build").rglob("vcomp-probe.exe"))
        env = dict(os.environ, PATH=str(dlls[0].parent) + os.pathsep + os.environ.get("PATH", ""))
        run([executable], env=env, timeout=120)
        shutil.copyfile(dlls[0], destination / "libiomp5md.dll")
        shutil.copyfile(
            next((work / "build").rglob("libiomp5md.lib")), destination / "libiomp5md.lib"
        )
        shutil.copyfile(next((work / "build").rglob("omp.h")), destination / "omp.h")
        shutil.copyfile(src / "openmp/LICENSE.TXT", destination / "LLVM-OpenMP-LICENSE.txt")

    return cached_build("llvm", target, pins, cache, build)


def verify_opencv(runtime_python, *, env=None):
    """Native runtime proof; never remove notices based on a build flag alone."""
    probe = """
import cv2
information = cv2.getBuildInformation()
print(information)
assert not cv2.ipp.useIPP(), 'OpenCV enables Intel IPP'
assert not any('Intel IPP' in line and 'NO' not in line for line in information.splitlines())
"""
    run([runtime_python, "-I", "-B", "-c", probe], env=env)


if __name__ == "__main__":
    import argparse

    parser = argparse.ArgumentParser(description="Build and repair the manylinux OpenCV wheel")
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    raw = args.out / "work/unrepaired"
    run([sys.executable, args.source / "setup.py", "bdist_wheel", "--dist-dir", raw])
    wheel = next(raw.glob("*.whl"))
    run(["auditwheel", "repair", "--plat", "manylinux_2_28_x86_64", "--wheel-dir", args.out, wheel])
