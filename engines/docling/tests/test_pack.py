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
