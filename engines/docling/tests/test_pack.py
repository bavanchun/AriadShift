"""Pack metadata stays independent of the installer interpreter's absolute path."""

from __future__ import annotations

import hashlib
import sys
from base64 import urlsafe_b64encode

import pytest
from conftest import REPOSITORY

sys.path.insert(0, str(REPOSITORY / "engines" / "docling" / "pack"))

import build


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
    kept = [library / "ssl.py", library / "ctypes/__init__.py", library / "threading.py"]
    for path in removed + kept:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(b"content")
    build.prune_runtime(runtime, target)
    assert all(not path.exists() for path in removed)
    assert all(path.exists() for path in kept)
