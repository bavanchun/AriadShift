"""Shared helpers: run the engine as the host does, with a cleared environment."""

from __future__ import annotations

import base64
import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

REPOSITORY = Path(__file__).resolve().parents[3]
FIXTURES = REPOSITORY / "fixtures"
PROTOCOL = "ariad-engine/1"
TIMEOUT_S = 600


def native_record_rows(site: Path, paths) -> str:
    rows = []
    for relative in paths:
        source = site / relative
        digest = (
            base64.urlsafe_b64encode(hashlib.sha256(source.read_bytes()).digest())
            .decode()
            .rstrip("=")
        )
        rows.append(f"{relative},sha256={digest},{source.stat().st_size}\n")
    return "".join(rows)


def reviewed_native_file(distribution, reviewed_paths) -> str:
    """Keep real-byte coverage mandatory on Linux and explain absent host artifacts."""
    files = {str(path) for path in distribution.files or []}
    relative = next((path for path in reviewed_paths if path in files), None)
    if relative is not None:
        return relative
    reason = (
        f"{distribution.metadata['Name']} {distribution.version}: reviewed native fixture "
        f"is absent on {sys.platform}; the real-object checks remain mandatory on Linux"
    )
    if sys.platform == "linux":
        pytest.fail(reason)
    pytest.skip(reason)


def clean_env() -> dict[str, str]:
    """The variables the host runner passes through ``env_clear``."""
    env = {"PATH": os.environ.get("PATH", "")}
    if sys.platform == "win32":
        env["SYSTEMROOT"] = os.environ.get("SYSTEMROOT", r"C:\Windows")
    return env


def run_engine(*requests: dict | bytes, timeout: float = TIMEOUT_S) -> subprocess.CompletedProcess:
    payload = b"".join(
        item if isinstance(item, bytes) else json.dumps(item).encode() + b"\n" for item in requests
    )
    return subprocess.run(
        [sys.executable, "-I", "-m", "ariad_docling"],
        input=payload,
        capture_output=True,
        env=clean_env(),
        timeout=timeout,
        check=False,
    )


def events(completed: subprocess.CompletedProcess) -> list[dict]:
    """Parses stdout; every line must be one JSON object."""
    lines = completed.stdout.decode("ascii").splitlines()
    return [json.loads(line) for line in lines]


@pytest.fixture
def workspace(tmp_path: Path) -> Path:
    for name in ("in", "out", "tmp"):
        (tmp_path / name).mkdir()
    return tmp_path


def convert_request(
    workspace: Path,
    source: Path,
    input_format: str,
    *,
    job: str = "job-1",
    options: dict | None = None,
    limits: dict | None = None,
    session_id: str | None = None,
) -> dict:
    copy = workspace / "in" / source.name
    copy.write_bytes(source.read_bytes())
    request = {
        "protocol": PROTOCOL,
        "job": job,
        "op": "convert",
        "input": {"path": str(copy), "format": input_format},
        "output": {"dir": str(workspace / "out"), "format": "docling+json"},
        "work_dir": str(workspace / "tmp"),
        "options": options or {},
        "limits": limits if limits is not None else {"max_nesting_depth": 64},
    }
    if session_id is not None:
        request["session_id"] = session_id
    return request
