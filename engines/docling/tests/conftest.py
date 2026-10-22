"""Shared helpers: run the engine as the host does, with a cleared environment."""

from __future__ import annotations

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
