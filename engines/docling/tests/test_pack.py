"""Pack metadata stays independent of the installer interpreter's absolute path."""

from __future__ import annotations

import hashlib
import struct
import sys
from base64 import urlsafe_b64encode

import pytest
from conftest import REPOSITORY

sys.path.insert(0, str(REPOSITORY / "engines" / "docling" / "pack"))

import build
import windows_imports


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
        record.write_text("\n".join(lines) + "\n")
        build.drop_build_records(site)
        assert not (info / "direct_url.json").exists()
        assert not (info / "uv_cache.json").exists()
        records.append(record.read_text())
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
    record.write_text(f"vendor/{name},sha256=old,0\nvendor-1.dist-info/RECORD,,\n")
    with pytest.raises(SystemExit, match="Microsoft runtime DLL must not be bundled"):
        build.archive(tmp_path, ["runtime"], tmp_path / "runtime.tar.zst")
    build.prune_microsoft_runtime(root)
    assert not forbidden.exists()
    assert record.read_text() == "vendor-1.dist-info/RECORD,,\n"
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
    record.write_text(f"module.pyd,sha256=old,1024\n{dll.name},sha256=old,0\n")
    build.prune_microsoft_runtime(root)
    result = path.read_bytes()
    assert len(result) == len(image)
    assert result[640 : 640 + len(alias)] == b"msvcp140.dll".ljust(len(alias), b"\0")
    assert struct.unpack_from("<I", result, 88 + 64)[0] == 0
    digest = urlsafe_b64encode(hashlib.sha256(result).digest()).decode().rstrip("=")
    assert record.read_text() == f"module.pyd,sha256={digest},1024\n"
    path.write_bytes(b"MZ")
    with pytest.raises(SystemExit, match="cannot retarget Microsoft runtime imports"):
        windows_imports.retarget(path, {alias: b"msvcp140.dll"})
