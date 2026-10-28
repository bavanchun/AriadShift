"""Pack metadata stays independent of the installer interpreter's absolute path."""

from __future__ import annotations

import hashlib
import json
import os
import re
import shutil
import struct
import subprocess
import sys
import tomllib
from argparse import Namespace
from base64 import urlsafe_b64encode

import pytest
from conftest import REPOSITORY

sys.path.insert(0, str(REPOSITORY / "engines" / "docling" / "pack"))

import build
import fetch
import licenses
import native_build
import torch_build
import windows_imports


def test_pack_manifest_keeps_the_frozen_protocol_host_range():
    manifest = tomllib.loads(build.pack_fields("0.2.0", "Apache-2.0"))
    assert manifest["requires_ashift"] == ">=0.2.0, <2.0.0"
    assert manifest["schema"] == "ariad-pack/1"


@pytest.mark.parametrize(
    "source",
    (
        "changed upstream probe",
        torch_build.SLEEF_BUILTIN_MATH_CHECK * 2,
        torch_build.SLEEF_BUILTIN_MATH_CHECK + torch_build.SLEEF_MSVC_GUARD,
    ),
)
def test_sleef_compiler_patch_refuses_unreviewed_source(tmp_path, source):
    configure = tmp_path / "third_party/sleef/Configure.cmake"
    configure.parent.mkdir(parents=True)
    configure.write_text(source, encoding="utf-8")
    with pytest.raises(SystemExit, match="review upstream SLEEF's changed"):
        torch_build.patch_sleef_msvc(tmp_path)
    assert configure.read_text(encoding="utf-8") == source


@pytest.mark.skipif(
    sys.platform != "linux" or not shutil.which("cmake") or not shutil.which("cc"),
    reason="real CMake compiler-check proof needs Linux CMake and a C compiler",
)
@pytest.mark.parametrize("msvc_version,expected", ((None, "1"), (1951, ""), (1952, "1")))
def test_sleef_compiler_patch_only_rejects_the_affected_version(tmp_path, msvc_version, expected):
    configure = tmp_path / "third_party/sleef/Configure.cmake"
    configure.parent.mkdir(parents=True)
    configure.write_text(torch_build.SLEEF_BUILTIN_MATH_CHECK, encoding="utf-8")
    torch_build.patch_sleef_msvc(tmp_path)
    (tmp_path / "CMakeLists.txt").write_text(
        "cmake_minimum_required(VERSION 3.20)\n"
        "project(SleefProbe C)\n"
        "include(CheckCSourceCompiles)\n"
        "include(third_party/sleef/Configure.cmake)\n"
        'file(WRITE "${CMAKE_BINARY_DIR}/result.txt" "${COMPILER_SUPPORTS_BUILTIN_MATH}")\n',
        encoding="utf-8",
    )
    command = ["cmake", "-S", str(tmp_path), "-B", str(tmp_path / "build")]
    if msvc_version is not None:
        command.append(f"-DCMAKE_REQUIRED_DEFINITIONS=-D_MSC_VER={msvc_version}")
    result = subprocess.run(command, capture_output=True, text=True, timeout=60)
    assert result.returncode == 0, result.stdout + result.stderr
    assert (tmp_path / "build/result.txt").read_text(encoding="utf-8") == expected


@pytest.mark.skipif(
    not os.environ.get("ASHIFT_VCOMP_ABI_LIBRARY"),
    reason="native wrapper ABI proof needs a source-built LLVM runtime",
)
def test_msvc_fork_abi_against_real_llvm_runtime():
    code = (
        "import ctypes, runpy, sys\n"
        "runpy.run_path(sys.argv[1])['vcomp_abi'](ctypes.CDLL(sys.argv[2]))\n"
    )
    subprocess.run(
        [
            sys.executable,
            "-c",
            code,
            str(native_build.PACK / "probe-runtime.py"),
            os.environ["ASHIFT_VCOMP_ABI_LIBRARY"],
        ],
        check=True,
        timeout=120,
    )


def test_llvm_export_map_preserves_upstream_and_exports_every_msvc_wrapper(tmp_path):
    source = tmp_path / "openmp/runtime/src"
    source.mkdir(parents=True)
    original = "# Existing LLVM exports\nomp_get_num_threads 718\n"
    (source / "dllexports").write_text(original, encoding="utf-8")
    (source / "CMakeLists.txt").write_text("# Existing LLVM build\n", encoding="utf-8")
    native_build.configure_llvm_openmp(tmp_path, tmp_path / "build/libiomp5md.lib")
    exports = (source / "dllexports").read_text(encoding="utf-8")
    assert exports.startswith(original)
    wrappers = set(
        re.findall(
            r"__declspec\(dllexport\) void (_vcomp_\w+)\(",
            (native_build.PACK / "vcomp-compat.cpp").read_text(encoding="utf-8"),
        )
    )
    assert set(exports[len(original) :].split()) == wrappers


@pytest.mark.parametrize("name", native_build.VCOMP_EXPORTS)
def test_llvm_export_changes_require_review_if_upstream_adds_a_wrapper(tmp_path, name):
    source = tmp_path / "openmp/runtime/src"
    source.mkdir(parents=True)
    original = f"{name} 123\n"
    (source / "dllexports").write_text(original, encoding="utf-8")
    with pytest.raises(SystemExit, match="review upstream LLVM's existing MSVC export"):
        native_build.configure_llvm_openmp(tmp_path, tmp_path / "build/libiomp5md.lib")
    assert (source / "dllexports").read_text(encoding="utf-8") == original


@pytest.mark.skipif(
    sys.platform != "linux"
    or not all(shutil.which(tool) for tool in ("clang-cl", "llvm-dlltool", "llvm-lib", "llvm-nm")),
    reason="Linux COFF archive proof needs the system LLVM tools",
)
def test_msvc_marker_requires_an_object_in_the_import_archive(tmp_path):
    marker = "_You_must_link_with_Microsoft_OpenMP_library"
    definition = tmp_path / "libiomp5md.def"
    definition.write_text(
        "LIBRARY libiomp5md.dll\nEXPORTS\n"
        + "\n".join(native_build.VCOMP_EXPORTS)
        + f"\n{marker} DATA\n",
        encoding="utf-8",
    )
    raw = tmp_path / "raw.lib"
    obj = tmp_path / "markers.obj"
    corrected = tmp_path / "libiomp5md.lib"
    subprocess.run(
        ["llvm-dlltool", "-m", "i386:x86-64", "-d", str(definition), "-l", str(raw)],
        check=True,
    )
    raw_symbols = subprocess.check_output(["llvm-nm", "--defined-only", str(raw)], text=True)
    assert f"__imp_{marker}" in raw_symbols.split()
    assert marker not in raw_symbols.split()
    subprocess.run(
        ["clang-cl", "/nologo", "/c", f"/Fo{obj}", str(native_build.PACK / "vcomp-import.cpp")],
        check=True,
    )
    subprocess.run(["llvm-lib", f"/out:{corrected}", str(raw), str(obj)], check=True)
    symbols = subprocess.check_output(
        ["llvm-nm", "--defined-only", str(corrected)], text=True
    ).split()
    assert marker in symbols
    assert "_You_must_link_with_exactly_one_OpenMP_library" in symbols
    assert set(native_build.VCOMP_EXPORTS) <= set(symbols)


def test_torch_source_build_requires_a_matching_native_runner():
    target = "x86_64-unknown-linux-gnu" if sys.platform == "win32" else "x86_64-pc-windows-msvc"
    with pytest.raises(SystemExit, match="matching native x86_64 runner"):
        torch_build.build(Namespace(target=target))


def test_opencv_source_pin_matches_the_locked_distribution():
    pins = fetch.load_pins()
    native_build.check_opencv_pin(pins)
    pins["opencv"]["version"] = "0.0.0"
    with pytest.raises(SystemExit, match="review the OpenCV source pin"):
        native_build.check_opencv_pin(pins)


def release_pin(tmp_path, *, kind="opencv"):
    target = "x86_64-unknown-linux-gnu"
    name = "opencv_python_headless-5.0.0.93-cp314-cp314-manylinux_2_28_x86_64.whl"
    content = b"release artifact"
    digest = hashlib.sha256(content).hexdigest()
    cache = tmp_path / "cache"
    blob = cache / "blobs" / digest
    blob.parent.mkdir(parents=True)
    blob.write_bytes(content)
    pin = {
        "name": name,
        "url": f"https://github.com/bavanchun/AriadShift/releases/download/toolchain-wheels-1/{name}",
        "sha256": digest,
        "size": len(content),
    }
    entry = {"build_key": "source-key", "files": [pin]}
    pins = {"toolchain": {"release": "toolchain-wheels-1", "targets": {target: {kind: entry}}}}
    return target, pins, cache, blob, pin


def test_pinned_release_files_are_rehashed_before_they_are_used(tmp_path, monkeypatch):
    target, pins, cache, blob, pin = release_pin(tmp_path)
    output = tmp_path / "out"
    files = native_build.release_files("opencv", target, pins, cache, output, "source-key")
    assert files[pin["name"]].read_bytes() == b"release artifact"
    blob.write_bytes(b"damaged artifact")
    monkeypatch.setattr(fetch, "ATTEMPTS", 1)
    monkeypatch.setattr(fetch.time, "sleep", lambda _: None)

    def unavailable(*_):
        raise OSError("release unavailable")

    monkeypatch.setattr(fetch, "_download", unavailable)
    with pytest.raises(SystemExit, match="could not download"):
        native_build.release_files("opencv", target, pins, cache, output, "source-key")


def test_only_unpinned_toolchains_select_source_build_fallback(tmp_path):
    target, pins, cache, _, _ = release_pin(tmp_path)
    assert native_build.release_files("torch", target, pins, cache, tmp_path / "out", "key") is None
    with pytest.raises(SystemExit, match="rebuild and repin"):
        native_build.release_files("opencv", target, pins, cache, tmp_path / "out", "stale")


@pytest.mark.parametrize(
    ("field", "value", "message"),
    [
        ("name", "../wheel.whl", "unique single file names"),
        ("name", r"C:\wheel.whl", "unique single file names"),
        ("url", "https://example.com/wheel.whl", "pinned repository release"),
        (
            "url",
            "https://github.com/bavanchun/AriadShift/releases/download/toolchain-wheels-2/wheel.whl",
            "pinned repository release",
        ),
        ("sha256", "unverified", "SHA-256 and positive size"),
        ("size", 0, "SHA-256 and positive size"),
    ],
)
def test_release_pins_reject_unreviewed_locations_and_missing_hashes(
    tmp_path, field, value, message
):
    target, pins, cache, _, pin = release_pin(tmp_path)
    pin[field] = value
    with pytest.raises(SystemExit, match=message):
        native_build.release_files("opencv", target, pins, cache, tmp_path / "out", "source-key")


def test_release_pins_require_nonempty_unique_files(tmp_path):
    target, pins, cache, _, pin = release_pin(tmp_path)
    entry = pins["toolchain"]["targets"][target]["opencv"]
    entry["files"].append(pin)
    with pytest.raises(SystemExit, match="unique single file names"):
        native_build.release_files("opencv", target, pins, cache, tmp_path / "out", "source-key")
    entry["files"] = []
    with pytest.raises(SystemExit, match="has no files"):
        native_build.release_files("opencv", target, pins, cache, tmp_path / "out", "source-key")


def test_torch_bootstrap_requires_a_native_runner(tmp_path, monkeypatch):
    target = "x86_64-unknown-linux-gnu"
    monkeypatch.setattr(build, "host_target", lambda: "aarch64-apple-darwin")
    monkeypatch.setattr(torch_build, "key", lambda _: "key")
    monkeypatch.setattr(fetch, "load_pins", lambda: {})
    with pytest.raises(SystemExit, match="matching native runner"):
        build.install_torch_toolchain(tmp_path, tmp_path / "runtime", target, tmp_path / "cache")


@pytest.mark.parametrize("cached", [False, True])
def test_torch_bootstrap_builds_once_then_verifies_cached_outputs(tmp_path, monkeypatch, cached):
    target = "x86_64-unknown-linux-gnu"
    cache = tmp_path / "cache"
    output = cache / "native/torch-key"
    output.mkdir(parents=True)
    if cached:
        (output / "build.json").write_text(json.dumps({"build_key": "key"}), encoding="utf-8")
    version = torch_build.load_config()["torch"]["version"]
    wheel = output / f"torch-{version}-cp314-cp314-manylinux_2_28_x86_64.whl"
    wheel.write_bytes(b"wheel")
    (output / "OpenBLAS-LICENSE.txt").write_text("BSD-3-Clause", encoding="utf-8")
    monkeypatch.setattr(build, "host_target", lambda: target)
    monkeypatch.setattr(torch_build, "key", lambda _: "key")
    monkeypatch.setattr(fetch, "load_pins", lambda: {})
    commands = []
    monkeypatch.setattr(build, "run", lambda command, **_: commands.append(list(map(str, command))))
    files = build.install_torch_toolchain(tmp_path, tmp_path / "runtime", target, cache)
    assert files[wheel.name].read_bytes() == b"wheel"
    assert ("verify" if cached else "build") in commands[0]
    assert str(files[wheel.name]) in commands[1]
    assert "--reinstall" in commands[1]


def test_pinned_opencv_is_consumed_without_starting_a_source_build(tmp_path, monkeypatch):
    target, pins, cache, _, pin = release_pin(tmp_path)
    pins["opencv"] = {"version": "5.0.0.93"}
    monkeypatch.setattr(native_build, "cache_key", lambda *_: "source-key")
    wheel = native_build.opencv(target, pins, cache)
    assert wheel.name == pin["name"]
    assert wheel.read_bytes() == b"release artifact"


@pytest.mark.parametrize("target", ["x86_64-unknown-linux-gnu", "x86_64-pc-windows-msvc"])
def test_pinned_torch_installs_without_building_and_requires_its_license_sidecars(
    tmp_path, monkeypatch, target
):
    version = torch_build.load_config()["torch"]["version"]
    platform = "win_amd64" if build.is_windows(target) else "manylinux_2_28_x86_64"
    name = f"torch-{version}-cp314-cp314-{platform}.whl"
    files = {name: tmp_path / name, "OpenBLAS-LICENSE.txt": tmp_path / "OpenBLAS-LICENSE.txt"}
    if build.is_windows(target):
        files.update(
            {name: tmp_path / name for name in ("libiomp5md.dll", "LLVM-OpenMP-LICENSE.txt")}
        )
    monkeypatch.setattr(native_build, "release_files", lambda *_: files)
    monkeypatch.setattr(torch_build, "key", lambda _: "key")
    commands = []
    monkeypatch.setattr(build, "run", lambda command, **_: commands.append(list(map(str, command))))
    result = build.install_torch_toolchain(
        tmp_path, tmp_path / "runtime", target, tmp_path / "cache"
    )
    assert result == files
    assert len(commands) == 1
    assert str(files[name]) in commands[0]
    assert "--reinstall" in commands[0]
    del files["OpenBLAS-LICENSE.txt"]
    with pytest.raises(SystemExit, match="missing required sidecars"):
        build.install_torch_toolchain(tmp_path, tmp_path / "runtime", target, tmp_path / "cache")
    assert len(commands) == 1


@pytest.mark.parametrize(
    "name",
    [
        "libiomp5md.dll",
        "libiompstubs5md.dll",
        "libiomp5-abcdef.dll",
        "libippicv.so",
        "libmkl_rt.so",
        "libippcore.so",
    ],
)
def test_vendor_gate_rejects_intel_libraries_inside_wheels(tmp_path, name):
    path = tmp_path / "runtime/Lib/site-packages/vendor" / name
    path.parent.mkdir(parents=True)
    path.write_bytes(b"native code")
    with pytest.raises(SystemExit, match="unapproved Intel library"):
        licenses.check_vendor_files(tmp_path)


@pytest.mark.parametrize(
    "content",
    [
        b"Intel(R) IPP ICV library\nIntel Simplified Software License",
        b"Intel(R) IPP ICV library statically linked",
        b"Intel(R) Integrated Performance Primitives",
    ],
)
def test_vendor_gate_rejects_ipp_notices_even_with_permissive_wheel_metadata(tmp_path, content):
    info = tmp_path / "opencv-1.dist-info"
    info.mkdir()
    (info / "METADATA").write_text(
        "Name: opencv-python-headless\nLicense-Expression: Apache-2.0\n", encoding="utf-8"
    )
    (info / "LICENSE-3RD-PARTY.txt").write_bytes(content)
    with pytest.raises(SystemExit, match="proprietary IPP ICV notice"):
        licenses.check_vendor_files(tmp_path)


def test_vendor_gate_admits_only_the_exact_llvm_build_and_rejects_tampering(tmp_path):
    path = tmp_path / "runtime/Lib/site-packages/torch/lib/libiomp5md.dll"
    path.parent.mkdir(parents=True)
    path.write_bytes(b"LLVM OpenMP")
    approved = {
        path.relative_to(tmp_path).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
    }
    licenses.check_vendor_files(tmp_path, approved)
    path.write_bytes(b"another OpenMP runtime")
    with pytest.raises(SystemExit, match="unapproved Intel library"):
        licenses.check_vendor_files(tmp_path, approved)
    path.unlink()
    with pytest.raises(SystemExit, match="missing LLVM OpenMP replacement"):
        licenses.check_vendor_files(tmp_path, approved)


@pytest.mark.parametrize(
    "content",
    [
        b"ippicvGetLibVersion",
        b"@(#) Intel(R) OMP Fortran support",
        b"Intel(R) oneAPI Math Kernel Library Version 2024.2",
        b"MKL_Get_Version_String",
        b"mkl_blas_sgemm",
        b"ippGetLibVersion",
        "Intel(R) OpenMP* Runtime Library".encode("utf-16le"),
    ],
)
def test_vendor_gate_rejects_renamed_proprietary_binaries(tmp_path, content):
    (tmp_path / "renamed.dll").write_bytes(b"MZ" + content)
    with pytest.raises(SystemExit, match="proprietary Intel binary content"):
        licenses.check_vendor_files(tmp_path)


def test_vendor_gate_allows_permissive_itt_and_onednn_symbols(tmp_path):
    (tmp_path / "libtorch_cpu.so").write_bytes(
        b"__itt_sync_create_init_3_0\0iJIT_NotifyEvent\0dnnl_get_version\0mkldnn_convolution"
    )
    (tmp_path / "libittnotify.so").write_bytes(b"BSD-3-Clause ITT API __itt_api_init")
    licenses.check_vendor_files(tmp_path)


def test_vendor_gate_preserves_common_notices_for_components_not_in_the_binary(tmp_path):
    (tmp_path / "ThirdPartyNotices.txt").write_text(
        "Intel Math Kernel Library (Intel MKL)\n"
        "Intel Simplified Software License (Version April 2018)",
        encoding="utf-8",
    )
    (tmp_path / "onnxruntime.so").write_bytes(b"CPUExecutionProvider MLAS")
    licenses.check_vendor_files(tmp_path)
    (tmp_path / "onnxruntime.so").write_bytes(b"MKL_Get_Version_String")
    with pytest.raises(SystemExit, match="proprietary Intel binary content"):
        licenses.check_vendor_files(tmp_path)


def test_vendor_gate_rejects_intel_even_if_the_cache_claims_its_digest(tmp_path):
    path = tmp_path / "libiomp5md.dll"
    path.write_bytes("Intel(R) OpenMP* Runtime Library".encode("utf-16le"))
    approved = {path.name: hashlib.sha256(path.read_bytes()).hexdigest()}
    with pytest.raises(SystemExit, match="proprietary Intel binary content"):
        licenses.check_vendor_files(tmp_path, approved)


def test_only_the_reviewed_inapplicable_ipp_notice_is_removed(tmp_path):
    path = tmp_path / "LICENSE-3RD-PARTY.txt"
    separator = native_build.NOTICE_SEPARATOR
    prefix, suffix = "OpenCV Apache-2.0\n", "\nOrbbec MIT licence\n"
    path.write_text(
        prefix
        + separator
        + "\n"
        + native_build.IPP_HEADING
        + "\nIntel Simplified Software License\n"
        + separator
        + suffix,
        encoding="utf-8",
    )
    native_build.remove_inapplicable_ipp_notice(path)
    assert path.read_text(encoding="utf-8") == prefix + separator + suffix
    with pytest.raises(SystemExit, match="unreviewed OpenCV IPP notice layout"):
        native_build.remove_inapplicable_ipp_notice(path)


def test_replaced_openmp_and_notice_records_match_the_distributed_bytes(tmp_path):
    site = tmp_path / "site-packages"
    info = site / "torch-1.dist-info"
    info.mkdir(parents=True)
    dll = site / "libiomp5md.dll"
    dll.write_bytes(b"LLVM OpenMP")
    record = info / "RECORD"
    record.write_text(
        "libiomp5md.dll,sha256=old,0\nlibiompstubs5md.dll,sha256=old,0\n"
        "torch-1.dist-info/RECORD,,\n",
        encoding="utf-8",
    )
    build.refresh_wheel_records(site)
    digest = urlsafe_b64encode(hashlib.sha256(dll.read_bytes()).digest()).decode().rstrip("=")
    assert (
        record.read_text(encoding="utf-8")
        == f"libiomp5md.dll,sha256={digest},11\ntorch-1.dist-info/RECORD,,\n"
    )


@pytest.mark.parametrize("installer_records", [False, True])
def test_removed_console_script_records_do_not_retain_build_paths(tmp_path, installer_records):
    def entry(path, content):
        digest = urlsafe_b64encode(hashlib.sha256(content).digest()).decode().rstrip("=")
        return f"{path},sha256={digest},{len(content)}"

    records = []
    module = entry("demo.py", b"x")
    for name, interpreter in (("first", "/build/python"), ("second", "/other/build/python")):
        site = tmp_path / name
        info = site / "demo-1.0.dist-info"
        info.mkdir(parents=True)
        (site / "demo.py").write_bytes(b"x")
        script = f"#!{interpreter}\nprint('demo')\n".encode()
        lines = [
            module,
            "demo-1.0.dist-info/RECORD,,",
            entry("bin/demo", script),
        ]
        if installer_records:
            for filename in ("direct_url.json", "uv_cache.json"):
                content = interpreter.encode()
                (info / filename).write_bytes(content)
                lines.append(entry(f"{info.name}/{filename}", content))
        record = info / "RECORD"
        record.write_text("\n".join(lines) + "\n", encoding="utf-8")
        build.drop_build_records(site)
        assert not (info / "direct_url.json").exists()
        assert not (info / "uv_cache.json").exists()
        records.append(record.read_text(encoding="utf-8"))
    assert records[0] == records[1]
    assert records[0] == f"{module}\ndemo-1.0.dist-info/RECORD,,\n"


@pytest.mark.parametrize(
    "target", ["x86_64-unknown-linux-gnu", "aarch64-apple-darwin", "x86_64-pc-windows-msvc"]
)
def test_runtime_pruning_keeps_engine_requirements_and_removes_gui_and_tests(tmp_path, target):
    runtime = tmp_path / "runtime"
    library = build.library_root(runtime, target)
    library.mkdir(parents=True)
    # Model Windows' case-insensitive Lib alias when this fixture runs on Unix.
    if build.is_windows(target) and not (runtime / "lib").exists():
        (runtime / "lib").symlink_to(library.resolve(), target_is_directory=True)
    removed = [
        library / "tkinter/__init__.py",
        library / "idlelib/idle.py",
        library / "test/test_math.py",
        library / "sqlite3/test/test_db.py",
        library / "lib-dynload/_tkinter.so",
        library / "lib-dynload/_dbm.cpython-314-linux-gnu.so",
        runtime / "tcl/tk9.0/tk.tcl",
        runtime / "DLLs/_tkinter.pyd",
        runtime / "DLLs/tcl90t.dll",
        runtime / "DLLs/libtommath.dll",
        runtime / "bin/idle3",
    ]
    if not build.is_windows(target):
        removed.extend(
            [
                runtime / "lib/libtcl9.0.so",
                runtime / "lib/libtcl9tk9.0.dylib",
                runtime / "lib/tcl9.0/init.tcl",
                runtime / "lib/itcl4.3.8/itcl.tcl",
            ]
        )
    else:
        removed.extend([runtime / "vcruntime140.dll", runtime / "vcruntime140_1.dll"])
    kept = [library / "ssl.py", library / "ctypes/__init__.py", library / "threading.py"]
    for path in removed + kept:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(b"content")
    build.prune_runtime(runtime, target)
    assert all(not path.exists() for path in removed)
    assert all(path.exists() for path in kept)


@pytest.mark.parametrize("name", ["vcruntime140.dll", "VCRUNTIME140_1.DLL", "msvcp140.dll"])
def test_archive_rejects_microsoft_runtime_dlls_including_wheel_copies(tmp_path, name):
    root = tmp_path / "runtime"
    vendor = root / "Lib/site-packages/vendor"
    vendor.mkdir(parents=True)
    forbidden = vendor / name
    forbidden.touch()
    record = root / "Lib/site-packages/vendor-1.dist-info/RECORD"
    record.parent.mkdir()
    record.write_text(
        f"vendor/{name},sha256=old,0\nvendor-1.dist-info/RECORD,,\n", encoding="utf-8"
    )
    with pytest.raises(SystemExit, match="Microsoft runtime DLL must not be bundled"):
        build.archive(tmp_path, ["runtime"], tmp_path / "runtime.tar.zst")
    build.prune_microsoft_runtime(root)
    assert not forbidden.exists()
    assert record.read_text(encoding="utf-8") == "vendor-1.dist-info/RECORD,,\n"
    assert build.archive(tmp_path, ["runtime"], tmp_path / "runtime.tar.zst") == "runtime.tar.zst"


@pytest.mark.parametrize("directory", [1, 13])
def test_wheel_imports_use_system_runtime_names_without_relocating_image_data(tmp_path, directory):
    image = bytearray(1024)
    image[:2] = b"MZ"
    struct.pack_into("<I", image, 0x3C, 64)
    image[64:68] = b"PE\0\0"
    struct.pack_into("<H", image, 70, 1)
    struct.pack_into("<H", image, 84, 240)
    struct.pack_into("<H", image, 88, 0x20B)
    struct.pack_into("<I", image, 88 + 64, 123)
    struct.pack_into("<II", image, 88 + 112 + 8 * directory, 0x1000, 64)
    struct.pack_into("<IIII", image, 328 + 8, 512, 0x1000, 512, 512)
    if directory == 13:
        struct.pack_into("<II", image, 512, 1, 0x1080)
    else:
        struct.pack_into("<I", image, 512 + 12, 0x1080)
    alias = b"msvcp140-0123456789abcdef.dll"
    image[640 : 640 + len(alias) + 1] = alias + b"\0"
    root = tmp_path / "runtime"
    site = root / "Lib/site-packages"
    site.mkdir(parents=True)
    path = site / "module.pyd"
    path.write_bytes(image)
    dll = site / alias.decode()
    dll.touch()
    record = site / "module-1.dist-info/RECORD"
    record.parent.mkdir()
    record.write_text(f"module.pyd,sha256=old,1024\n{dll.name},sha256=old,0\n", encoding="utf-8")
    build.prune_microsoft_runtime(root)
    result = path.read_bytes()
    assert len(result) == len(image)
    assert result[640 : 640 + len(alias)] == b"msvcp140.dll".ljust(len(alias), b"\0")
    assert struct.unpack_from("<I", result, 88 + 64)[0] == 0
    digest = urlsafe_b64encode(hashlib.sha256(result).digest()).decode().rstrip("=")
    assert record.read_text(encoding="utf-8") == f"module.pyd,sha256={digest},1024\n"
    path.write_bytes(b"MZ")
    with pytest.raises(SystemExit, match="cannot retarget Microsoft runtime imports"):
        windows_imports.retarget(path, {alias: b"msvcp140.dll"})
