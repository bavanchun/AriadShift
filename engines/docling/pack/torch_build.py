"""Build and audit the x86_64 CPU torch toolchain on native GitHub Actions runners.

The Linux driver reexecutes in the pinned manylinux image. Source archives and
build tools are pinned; sccache preserves compiler work across bounded jobs.
Only the final wheels and their provenance are exported, never a build tree.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import shutil
import subprocess
import sys
import tomllib
import zipfile
from pathlib import Path

import fetch
import licenses
import native_build

PACK = Path(__file__).resolve().parent
REPOSITORY = PACK.parents[2]
CONFIG = PACK / "torch-build.toml"
TARGETS = ("x86_64-unknown-linux-gnu", "x86_64-pc-windows-msvc")
ARM_TARGETS = ("aarch64-unknown-linux-gnu", "aarch64-apple-darwin")
OPENBLAS_FLAGS = (
    "-DCMAKE_BUILD_TYPE=Release",
    "-DCMAKE_POSITION_INDEPENDENT_CODE=ON",
    "-DBUILD_SHARED_LIBS=OFF",
    "-DBUILD_STATIC_LIBS=ON",
    "-DBUILD_TESTING=OFF",
    "-DBUILD_BENCHMARKS=OFF",
    "-DDYNAMIC_ARCH=ON",
    "-DTARGET=NEHALEM",
    "-DUSE_THREAD=OFF",
    "-DUSE_LOCKING=ON",
    "-DC_LAPACK=ON",
    "-DNOFORTRAN=ON",
)
VISION_PROBE = """
import torch, torchvision
from torchvision.ops import nms, roi_align
boxes = torch.tensor([[0., 0., 10., 10.], [1., 1., 9., 9.], [20., 20., 30., 30.]])
assert nms(boxes, torch.tensor([.9, .8, .7]), .5).tolist() == [0, 2]
image = torch.arange(64, dtype=torch.float32).reshape(1, 1, 8, 8)
result = roi_align(image, [torch.tensor([[0., 0., 7., 7.]])], (2, 2))
assert result.shape == (1, 1, 2, 2) and torch.isfinite(result).all()
print(torch.__version__, torchvision.__version__, 'NMS and ROIAlign passed')
"""
CPU_PROBE = """
from concurrent.futures import ThreadPoolExecutor
import torch
print(torch.__config__.show())
assert not torch.backends.mkl.is_available(), 'MKL remains enabled'
assert torch.version.cuda is None and torch.version.hip is None
torch.manual_seed(123)
a, b = torch.randn(67, 41), torch.randn(41, 53)
torch.set_num_threads(1)
expected = a @ b
for threads in (2, 3):
    torch.set_num_threads(threads)
    torch.testing.assert_close(a @ b, expected)
with ThreadPoolExecutor(max_workers=4) as pool:
    for actual in pool.map(lambda _: a @ b, range(8)):
        torch.testing.assert_close(actual, expected)
"""
TORCH_PROBE = (
    CPU_PROBE
    + """
assert torch.backends.mkldnn.is_available(), 'oneDNN must remain enabled'
assert not torch.distributed.is_available(), 'distributed must be disabled'
assert 'BLAS_INFO=open' in torch.__config__.show(), 'OpenBLAS was not selected'
image, weight = torch.randn(2, 3, 16, 16), torch.randn(5, 3, 3, 3)
expected = torch.nn.functional.conv2d(image, weight)
actual = torch.nn.functional.conv2d(image.to_mkldnn(), weight.to_mkldnn()).to_dense()
torch.testing.assert_close(actual, expected, rtol=1e-4, atol=1e-5)
print('MKL-free OpenBLAS / oneDNN CPU math passed')
"""
)


def load_config():
    with CONFIG.open("rb") as handle:
        config = tomllib.load(handle)
    with (REPOSITORY / "uv.lock").open("rb") as handle:
        locked = tomllib.load(handle)["package"]
    locked_versions = {
        name: {package["version"] for package in locked if package["name"] == name}
        for name in ("torch", "torchvision")
    }
    expected = {
        "torch": config["torch"]["version"],
        "torchvision": config["vision"]["version"],
    }
    mismatches = {
        name: (version, sorted(locked_versions[name]))
        for name, version in expected.items()
        if version not in locked_versions[name]
    }
    if mismatches:
        raise SystemExit(f"source build versions do not match uv.lock: {mismatches}")
    return config


def key(target):
    load_config()
    digest = hashlib.sha256(target.encode())
    # Model pins and published wheel digests do not invalidate the source build.
    for path in (
        CONFIG,
        Path(__file__),
        PACK / "native_build.py",
        PACK / "vcomp-compat.cpp",
        PACK / "vcomp-probe.cpp",
        PACK / "vcomp-import.cpp",
        REPOSITORY / ".github/workflows/torch-docling.yml",
        REPOSITORY / "uv.lock",
    ):
        digest.update(path.read_bytes())
    pins = fetch.load_pins()
    digest.update(json.dumps({p: pins[p] for p in ("llvm", "manylinux")}, sort_keys=True).encode())
    return digest.hexdigest()


def run(command, **kwargs):
    native_build.run(command, **kwargs)


def report_cmake_failures(source: Path) -> None:
    """Expose failed compiler checks without hiding the original build failure."""
    path = source / "build/CMakeFiles/CMakeConfigureLog.yaml"
    if not path.is_file():
        return
    import yaml

    try:
        for document in yaml.safe_load_all(path.read_text("utf-8")):
            for event in (document or {}).get("events", []):
                result = event.get("buildResult", {})
                if event.get("kind", "").startswith("try_compile-") and result.get("exitCode"):
                    print(
                        "Failed CMake compiler check: "
                        + json.dumps(
                            {
                                key: event[key]
                                for key in ("checks", "backtrace", "buildResult")
                                if key in event
                            }
                        ),
                        flush=True,
                    )
    except (OSError, yaml.YAMLError) as error:
        print(f"Could not read CMake compiler diagnostics: {error}", flush=True)


def linux_container(args):
    """Run only on a native x86_64 Linux host; never emulate on the Mac."""
    cache = args.cache.resolve()
    cache.mkdir(parents=True, exist_ok=True)
    uv = Path(shutil.which("uv")).resolve()
    sccache = Path(shutil.which("sccache")).resolve()
    run(
        [
            "docker",
            "run",
            "--rm",
            "--name",
            os.environ.get("TORCH_BUILD_CONTAINER", f"ashift-torch-{os.getpid()}"),
            "--user",
            f"{os.getuid()}:{os.getgid()}",
            "--volume",
            f"{REPOSITORY}:{REPOSITORY}",
            "--volume",
            f"{cache}:{cache}",
            "--volume",
            f"{uv}:/usr/local/bin/uv:ro",
            "--volume",
            f"{sccache}:/usr/local/bin/sccache:ro",
            "--workdir",
            REPOSITORY,
            "--env",
            "HOME=/tmp",
            "--env",
            "UV_PYTHON=/opt/python/cp314-cp314/bin/python",
            "--env",
            "UV_PYTHON_DOWNLOADS=never",
            "--env",
            "UV_PROJECT_ENVIRONMENT=/tmp/torch-build-env",
            "--env",
            "UV_CACHE_DIR=/tmp/uv-cache",
            "--env",
            f"SCCACHE_DIR={REPOSITORY / '.tools/sccache'}",
            "--env",
            "SCCACHE_CACHE_SIZE=3G",
            "--env",
            "SCCACHE_IDLE_TIMEOUT=0",
            fetch.load_pins()["manylinux"]["image"],
            *native_build.tools(
                "python",
                Path(__file__),
                "build",
                "--target",
                args.target,
                "--out",
                args.out.resolve(),
                "--cache",
                cache,
                "--inside-manylinux",
            ),
        ]
    )


def openblas(config, work, cache):
    src = native_build.source(config["openblas"], work / "openblas-source", cache)
    prefix = work / "openblas"
    command = [
        "cmake",
        "-S",
        src,
        "-B",
        work / "openblas-build",
        "-G",
        "Ninja",
        f"-DCMAKE_INSTALL_PREFIX={prefix.as_posix()}",
        *OPENBLAS_FLAGS,
        "-DCMAKE_C_COMPILER_LAUNCHER=sccache",
    ]
    if sys.platform == "win32":
        # clang's MSVC target can assemble OpenBLAS's GNU-syntax x86 kernels
        # while producing an MSVC-compatible static library. C LAPACK avoids
        # shipping a Fortran compiler runtime.
        command += ["-DCMAKE_C_COMPILER=clang", "-DCMAKE_ASM_COMPILER=clang"]
    run(command)
    run(["cmake", "--build", work / "openblas-build", "--parallel", "3"])
    run(["cmake", "--install", work / "openblas-build"])
    pattern = "*openblas*.lib" if sys.platform == "win32" else "libopenblas*.a"
    libraries = list(prefix.rglob(pattern))
    if len(libraries) != 1:
        raise SystemExit(f"expected one static OpenBLAS library, found {libraries}")
    headers = list(prefix.rglob("cblas.h"))
    if len(headers) != 1:
        raise SystemExit(f"expected one installed OpenBLAS cblas.h, found {headers}")
    return prefix, libraries[0], src / "LICENSE"


def probe(python, code, **kwargs):
    return subprocess.run([str(python), "-I", "-B", "-c", code], timeout=180, **kwargs)


def audit_artifacts(out: Path, audit: Path, openmp: Path | None = None) -> None:
    """Scan native wheel and sidecar bytes, including restored cache entries."""
    shutil.rmtree(audit, ignore_errors=True)
    wheels = audit / "wheels"
    wheels.mkdir(parents=True)
    for wheel in out.glob("*.whl"):
        with zipfile.ZipFile(wheel) as archive:
            archive.extractall(wheels / wheel.stem)

    approved = {}
    if openmp is not None:
        expected_digest = fetch._sha256(openmp)
        for dll in wheels.rglob("*iomp*.dll"):
            if fetch._sha256(dll) != expected_digest:
                raise SystemExit(f"unexpected OpenMP DLL in built wheel: {dll}")
            approved[dll.relative_to(audit).as_posix()] = expected_digest
        sidecar = audit / "sidecars" / openmp.name
        sidecar.parent.mkdir()
        shutil.copyfile(openmp, sidecar)
        approved[sidecar.relative_to(audit).as_posix()] = expected_digest
    licenses.check_vendor_files(audit, approved)
    shutil.rmtree(audit)


def repair_linux_wheel(out: Path, work: Path, name: str) -> None:
    wheels = list(out.glob(f"{name}-*.whl"))
    if len(wheels) != 1:
        raise SystemExit(f"expected exactly one built {name} wheel, found {wheels}")
    repaired = work / f"{name}-repaired"
    # torchvision declares torch as a wheel dependency. Keep those libraries
    # in torch instead of grafting a second copy of its runtime into vision.
    excludes = (
        ["--exclude", "libtorch*.so*", "--exclude", "libc10*.so*"] if name == "torchvision" else []
    )
    run(
        [
            "auditwheel",
            "repair",
            "--plat",
            "manylinux_2_28_x86_64",
            "--wheel-dir",
            repaired,
            *excludes,
            wheels[0],
        ]
    )
    wheels[0].unlink()
    shutil.move(next(repaired.glob("*.whl")), out)


def build(args):
    expected = "Windows" if args.target.endswith("windows-msvc") else "Linux"
    if platform.system() != expected or platform.machine().lower() not in ("amd64", "x86_64"):
        raise SystemExit("torch source builds require a matching native x86_64 runner")
    if expected == "Linux" and not args.inside_manylinux:
        linux_container(args)
        return
    config = load_config()
    pins = fetch.load_pins()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    for pattern in (
        "*.whl",
        "build.json",
        "OpenBLAS-LICENSE.txt",
        "LLVM-OpenMP-LICENSE.txt",
        "libiomp5md.dll",
    ):
        for artifact in out.glob(pattern):
            artifact.unlink()
    work = REPOSITORY / ".tools/torch-source"
    shutil.rmtree(work, ignore_errors=True)
    work.mkdir(parents=True)
    llvm = (
        native_build.llvm_openmp(args.target, pins, args.cache) if expected == "Windows" else None
    )
    prefix, blas, blas_license = openblas(config, work, args.cache)
    src = native_build.source(config["torch"], work / "torch", args.cache)
    env = dict(
        os.environ,
        **config["torch"]["options"],
        PYTORCH_BUILD_VERSION=config["torch"]["version"],
        PYTORCH_BUILD_NUMBER="1",
        MAX_JOBS="3",
        CMAKE_BUILD_PARALLEL_LEVEL="3",
        CMAKE_BUILD_TYPE="Release",
        CMAKE_GENERATOR="Ninja",
        CMAKE_C_COMPILER_LAUNCHER="sccache",
        CMAKE_CXX_COMPILER_LAUNCHER="sccache",
        SOURCE_DATE_EPOCH="0",
        PYTHONHASHSEED="0",
        OpenBLAS_HOME=str(prefix),
        CMAKE_POLICY_VERSION_MINIMUM="3.5",
    )
    # OpenBLAS installs headers in include/openblas on some platforms. Pass
    # their actual location instead of allowing CMake to select a system BLAS.
    options = {
        "OpenBLAS_LIB": blas.as_posix(),
        "OpenBLAS_INCLUDE_DIR": next(prefix.rglob("cblas.h")).parent.as_posix(),
        "CMAKE_POSITION_INDEPENDENT_CODE": "ON",
    }
    if llvm is not None:
        env.update(CC="cl", CXX="cl")
        options.update(
            {
                "OpenMP_libomp_LIBRARY": (llvm / "libiomp5md.lib").as_posix(),
                "OpenMP_CXX_FLAGS": f'/openmp /I"{llvm.as_posix()}"',
                "OpenMP_C_FLAGS": f'/openmp /I"{llvm.as_posix()}"',
                "OpenMP_CXX_LIB_NAMES": "libomp",
                "OpenMP_C_LIB_NAMES": "libomp",
                "CMAKE_INCLUDE_PATH": llvm.as_posix(),
            }
        )
    # PEP 517 config settings preserve spaces in Windows paths and OpenMP
    # flags without depending on CMAKE_ARGS shell tokenization.
    try:
        run(
            [
                sys.executable,
                "-m",
                "build",
                "--wheel",
                "--no-isolation",
                "--outdir",
                out,
                *(
                    f"--config-setting=cmake.define.{name}={value}"
                    for name, value in options.items()
                ),
                src,
            ],
            env=env,
        )
    except subprocess.CalledProcessError:
        report_cmake_failures(src)
        raise
    if expected == "Linux":
        repair_linux_wheel(out, work, "torch")
    wheels = list(out.glob("torch-*.whl"))
    if len(wheels) != 1:
        raise SystemExit("expected exactly one built torch wheel")
    # Install before deciding whether the official torchvision ABI is compatible.
    run(["uv", "pip", "install", "--python", sys.executable, "--no-deps", "--reinstall", wheels[0]])
    site = Path(
        subprocess.check_output(
            [sys.executable, "-c", "import sysconfig; print(sysconfig.get_path('purelib'))"],
            text=True,
        ).strip()
    )
    if llvm is not None:
        target = site / "torch/lib/libiomp5md.dll"
        shutil.copyfile(llvm / "libiomp5md.dll", target)
    probe(sys.executable, TORCH_PROBE, check=True)
    vision_source = False
    if probe(sys.executable, VISION_PROBE).returncode:
        vision = native_build.source(config["vision"], work / "vision", args.cache)
        vision_env = dict(os.environ, FORCE_CUDA="0", BUILD_VERSION=config["vision"]["version"])
        run(
            [sys.executable, vision / "setup.py", "bdist_wheel", "--dist-dir", out],
            cwd=vision,
            env=vision_env,
        )
        if expected == "Linux":
            repair_linux_wheel(out, work, "torchvision")
        wheel = next(out.glob("torchvision-*.whl"))
        run(["uv", "pip", "install", "--python", sys.executable, "--no-deps", "--reinstall", wheel])
        probe(sys.executable, VISION_PROBE, check=True)
        vision_source = True
    shutil.copyfile(blas_license, out / "OpenBLAS-LICENSE.txt")
    openmp = None
    if llvm is not None:
        shutil.copyfile(llvm / "LLVM-OpenMP-LICENSE.txt", out / "LLVM-OpenMP-LICENSE.txt")
        openmp = out / "libiomp5md.dll"
        shutil.copyfile(llvm / "libiomp5md.dll", openmp)
    audit_artifacts(out, work / "audit", openmp)
    record = {
        "schema": 1,
        "target": args.target,
        "build_key": key(args.target),
        "sources": config,
        "openblas_flags": OPENBLAS_FLAGS,
        "torchvision_source_build": vision_source,
        "files": {
            p.name: {"sha256": fetch._sha256(p), "size": p.stat().st_size}
            for p in sorted(out.iterdir())
            if p.is_file()
        },
    }
    (out / "build.json").write_text(json.dumps(record, indent=2) + "\n", "utf-8")
    run(["sccache", "--show-stats"])
    shutil.rmtree(work)


def verify(args):
    out = args.out.resolve()
    record = json.loads((out / "build.json").read_text("utf-8"))
    if record["build_key"] != key(args.target) or record["target"] != args.target:
        raise SystemExit("cached torch wheel was built with different inputs")
    actual = {path.name for path in out.iterdir() if path.is_file() and path.name != "build.json"}
    if set(record["files"]) != actual:
        raise SystemExit("cached torch build contains missing or unexpected files")
    for name, pin in record["files"].items():
        path = out / name
        if path.stat().st_size != pin["size"] or fetch._sha256(path) != pin["sha256"]:
            raise SystemExit(f"cached torch build changed: {name}")
    openmp = out / "libiomp5md.dll" if args.target.endswith("windows-msvc") else None
    audit_artifacts(out, REPOSITORY / ".tools/torch-cache-audit", openmp)
    wheels = [out / name for name in sorted(record["files"]) if name.endswith(".whl")]
    run(["uv", "pip", "install", "--python", sys.executable, "--no-deps", "--reinstall", *wheels])
    if args.target.endswith("windows-msvc"):
        site = Path(
            subprocess.check_output(
                [sys.executable, "-c", "import sysconfig; print(sysconfig.get_path('purelib'))"],
                text=True,
            ).strip()
        )
        shutil.copyfile(openmp, site / "torch/lib/libiomp5md.dll")
    probe(sys.executable, TORCH_PROBE, check=True)
    probe(sys.executable, VISION_PROBE, check=True)


def verify_upstream(args):
    """Keep the accepted official ARM wheels, but audit and execute them natively."""
    expected = "Darwin" if args.target.endswith("apple-darwin") else "Linux"
    if platform.system() != expected or platform.machine().lower() not in ("arm64", "aarch64"):
        raise SystemExit("ARM wheel verification requires a matching native ARM runner")
    load_config()
    # uv run --frozen supplies the hash-locked wheels. Do not scan unrelated
    # development dependencies such as the still-unreplaced OpenCV wheel.
    code = """
import importlib.metadata, pathlib
for name in ('torch', 'torchvision'):
    distribution = importlib.metadata.distribution(name)
    print(pathlib.Path(distribution.locate_file(name)).resolve())
    print(pathlib.Path(distribution.locate_file(f'{name}-{distribution.version}.dist-info')).resolve())
"""
    roots = subprocess.check_output([sys.executable, "-I", "-c", code], text=True).splitlines()
    for root in roots:
        path = Path(root)
        if not path.is_dir():
            raise SystemExit(f"missing upstream wheel directory: {path}")
        licenses.check_vendor_files(path)
    probe(sys.executable, CPU_PROBE, check=True)
    probe(sys.executable, VISION_PROBE, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("command", choices=("build", "key", "verify", "verify-upstream"))
    parser.add_argument("--target", choices=(*TARGETS, *ARM_TARGETS), required=True)
    parser.add_argument("--out", type=Path, default=Path("torch-out"))
    parser.add_argument("--cache", type=Path, default=fetch.default_cache())
    parser.add_argument("--inside-manylinux", action="store_true")
    args = parser.parse_args()
    if args.command == "verify-upstream":
        if args.target not in ARM_TARGETS:
            parser.error("verify-upstream is only for the retained official ARM wheels")
        verify_upstream(args)
    elif args.target not in TARGETS:
        parser.error("source builds and their caches are only for x86_64 targets")
    elif args.command == "key":
        print(key(args.target))
    elif args.command == "build":
        build(args)
    else:
        verify(args)


if __name__ == "__main__":
    main()
