"""Framing and validation for ariad-engine/1 messages."""

from __future__ import annotations

import json
import os
import threading
from dataclasses import dataclass, field
from typing import IO, Any

PROTOCOL = "ariad-engine/1"
MAX_LINE_BYTES = 1 << 20

ERROR_CODES = frozenset(
    {
        "invalid_request",
        "unsupported_route",
        "limit_exceeded",
        "engine_failure",
        "tool_missing",
        "tool_version",
        "io",
    }
)


class EngineError(Exception):
    """A typed failure reported to the host as ``result`` with ``ok: false``."""

    def __init__(self, code: str, message: str) -> None:
        if code not in ERROR_CODES:
            raise ValueError(f"unknown protocol error code: {code}")
        super().__init__(message)
        self.code = code
        self.message = message


class Writer:
    """Writes protocol events as single ASCII JSON lines to the protocol descriptor."""

    def __init__(self, fd: int) -> None:
        self._fd = fd
        self._lock = threading.Lock()

    def emit(self, event: dict[str, Any]) -> None:
        line = json.dumps(event, ensure_ascii=True, separators=(",", ":")) + "\n"
        data = line.encode("ascii")
        with self._lock:
            view = memoryview(data)
            while view:
                written = os.write(self._fd, view)
                view = view[written:]

    def progress(self, done: int, total: int) -> None:
        self.emit({"type": "progress", "stage": "convert", "done": done, "total": total})

    def warning(self, code: str, message: str) -> None:
        self.emit({"type": "warning", "code": code, "message": message})

    def artifact(self, path: str, output_format: str) -> None:
        self.emit({"type": "artifact", "path": path, "format": output_format})

    def ok(self, metrics: dict[str, Any] | None = None) -> None:
        event: dict[str, Any] = {"type": "result", "ok": True}
        if metrics is not None:
            event["metrics"] = metrics
        self.emit(event)

    def error(self, error: EngineError) -> None:
        self.emit(
            {
                "type": "result",
                "ok": False,
                "error": {"code": error.code, "message": error.message},
            }
        )


@dataclass(frozen=True)
class ConvertRequest:
    """A validated ``convert`` request."""

    job: str
    input_path: str
    input_format: str
    output_dir: str
    output_format: str
    work_dir: str
    options: dict[str, Any] = field(default_factory=dict)
    limits: dict[str, Any] = field(default_factory=dict)
    session_id: str | None = None


@dataclass(frozen=True)
class DescribeRequest:
    """A validated ``describe`` request."""

    job: str


def read_line(stream: IO[bytes]) -> bytes | None:
    """Reads one request line; ``None`` means the host closed stdin."""
    line = stream.readline(MAX_LINE_BYTES + 1)
    if not line:
        return None
    if len(line) > MAX_LINE_BYTES:
        raise EngineError("invalid_request", "request line exceeds the 1 MiB limit")
    return line


def parse_request(line: bytes) -> ConvertRequest | DescribeRequest:
    """Parses and validates one request line."""
    try:
        value = json.loads(line)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise EngineError("invalid_request", "request is not valid JSON") from error
    if not isinstance(value, dict):
        raise EngineError("invalid_request", "request must be a JSON object")
    if value.get("protocol") != PROTOCOL:
        raise EngineError("invalid_request", f"unsupported protocol; expected {PROTOCOL}")
    job = value.get("job")
    if not isinstance(job, str):
        raise EngineError("invalid_request", "request job must be a string")
    op = value.get("op")
    if op == "describe":
        return DescribeRequest(job=job)
    if op != "convert":
        raise EngineError("invalid_request", "request op must be convert or describe")

    request_input = _object(value, "input")
    output = _object(value, "output")
    options = value.get("options", {})
    limits = value.get("limits")
    session_id = value.get("session_id")
    if not isinstance(options, dict):
        raise EngineError("invalid_request", "request options must be an object")
    if not isinstance(limits, dict):
        raise EngineError("invalid_request", "convert requests require limits")
    if session_id is not None and not isinstance(session_id, str):
        raise EngineError("invalid_request", "session_id must be a string")
    return ConvertRequest(
        job=job,
        input_path=_string(request_input, "path", "input.path"),
        input_format=_string(request_input, "format", "input.format"),
        output_dir=_string(output, "dir", "output.dir"),
        output_format=_string(output, "format", "output.format"),
        work_dir=_string(value, "work_dir", "work_dir"),
        options=options,
        limits=limits,
        session_id=session_id,
    )


def _object(value: dict[str, Any], key: str) -> dict[str, Any]:
    item = value.get(key)
    if not isinstance(item, dict):
        raise EngineError("invalid_request", f"request {key} must be an object")
    return item


def _string(value: dict[str, Any], key: str, label: str) -> str:
    item = value.get(key)
    if not isinstance(item, str) or not item:
        raise EngineError("invalid_request", f"request {label} must be a non-empty string")
    return item


def optional_int(limits: dict[str, Any], key: str) -> int | None:
    """Reads an optional positive integer limit; absent or null means unlimited."""
    value = limits.get(key)
    if value is None:
        return None
    if isinstance(value, bool) or not isinstance(value, int) or value <= 0:
        raise EngineError("invalid_request", f"limits.{key} must be a positive integer")
    return value
