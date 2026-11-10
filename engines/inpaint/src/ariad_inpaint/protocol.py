"""Framing and validation for ariad-engine/2 messages."""

from __future__ import annotations

import json
import os
import threading
from dataclasses import dataclass, field
from pathlib import Path
from typing import IO, Any

PROTOCOL = "ariad-engine/2"
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
        "encrypted_input",
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
        if len(data) > MAX_LINE_BYTES:
            raise EngineError("limit_exceeded", "protocol event exceeds 1 MiB")
        with self._lock:
            view = memoryview(data)
            while view:
                written = os.write(self._fd, view)
                view = view[written:]

    def progress(self, stage: str, done: int | None = None, total: int | None = None) -> None:
        event: dict[str, Any] = {"type": "progress", "stage": stage}
        if done is not None:
            event["done"] = done
        if total is not None:
            event["total"] = total
        self.emit(event)

    def warning(self, code: str, message: str) -> None:
        self.emit({"type": "warning", "code": code, "message": message})

    def artifact(self, path: str, output_format: str) -> None:
        self.emit({"type": "artifact", "event": "artifact", "path": path, "format": output_format})

    def ok(self, data: dict[str, Any] | None = None) -> None:
        event: dict[str, Any] = {"type": "result", "event": "result", "ok": True}
        if data is not None:
            event["data"] = data
            event["metrics"] = data
        self.emit(event)

    def error(self, error: EngineError) -> None:
        self.emit(
            {
                "type": "result",
                "event": "result",
                "ok": False,
                "error": {"code": error.code, "message": error.message},
            }
        )


@dataclass(frozen=True)
class RunToolRequest:
    """A validated ``run_tool`` request in ariad-engine/2."""

    job: str
    tool: str
    inputs: list[dict[str, Any]]
    output_dir: str
    work_dir: str
    params: dict[str, Any] = field(default_factory=dict)
    params_file: str | None = None
    limits: dict[str, Any] = field(default_factory=dict)
    options: dict[str, Any] = field(default_factory=dict)


@dataclass(frozen=True)
class DescribeRequest:
    """A validated ``describe`` request."""

    job: str


def read_line(stream: IO[bytes]) -> bytes | None:
    """Reads one NDJSON protocol line bounded to MAX_LINE_BYTES."""
    line = stream.readline(MAX_LINE_BYTES + 1)
    if not line:
        return None
    if len(line) > MAX_LINE_BYTES:
        raise EngineError("limit_exceeded", "protocol line exceeds 1 MiB limit")
    return line


def parse_request(line: bytes) -> RunToolRequest | DescribeRequest:
    """Parses and validates an ariad-engine/2 request line."""
    try:
        obj = json.loads(line)
    except Exception as exc:
        raise EngineError("invalid_request", f"invalid JSON: {exc}") from exc

    if not isinstance(obj, dict):
        raise EngineError("invalid_request", "request must be a JSON object")

    protocol = obj.get("protocol")
    if protocol != PROTOCOL:
        raise EngineError(
            "invalid_request",
            f"unsupported protocol: expected '{PROTOCOL}', got '{protocol}'",
        )

    job = obj.get("job")
    if not isinstance(job, str) or not job:
        raise EngineError("invalid_request", "missing or invalid 'job' field")

    op = obj.get("op")
    if op == "describe":
        return DescribeRequest(job=job)

    if op == "run_tool":
        tool = obj.get("tool")
        if not isinstance(tool, str) or not tool:
            raise EngineError("invalid_request", "missing or invalid 'tool' field")

        inputs = obj.get("inputs")
        if not isinstance(inputs, list):
            raise EngineError("invalid_request", "'inputs' must be a list")

        output = obj.get("output")
        if not isinstance(output, dict) or "dir" not in output:
            raise EngineError("invalid_request", "'output.dir' is required")
        output_dir = output["dir"]
        if not isinstance(output_dir, str) or not output_dir:
            raise EngineError("invalid_request", "output.dir must be a nonempty path")
        for item in inputs:
            if not isinstance(item, dict) or not isinstance(item.get("path"), str):
                raise EngineError("invalid_request", "each input must contain a path")

        work_dir = obj.get("work_dir")
        if not isinstance(work_dir, str) or not work_dir:
            raise EngineError("invalid_request", "missing or invalid 'work_dir'")

        params = obj.get("params", {})
        params_file = obj.get("params_file")
        if params_file:
            if not isinstance(params_file, str):
                raise EngineError("invalid_request", "params_file must be a path")
            params_path = Path(params_file).resolve()
            if not params_path.is_relative_to(Path(work_dir).resolve()):
                raise EngineError("invalid_request", "params_file must be inside work_dir")
            if not params_path.is_file():
                raise EngineError("io", f"params_file '{params_file}' does not exist")
            if params_path.stat().st_size > 8 * 1024 * 1024:
                raise EngineError("limit_exceeded", "params_file exceeds 8 MiB")
            try:
                with open(params_path, "rb") as f:
                    params = json.load(f)
            except Exception as exc:
                raise EngineError("invalid_request", f"failed to read params_file: {exc}") from exc

        limits = obj.get("limits", {})
        options = obj.get("options", {})
        if not all(isinstance(value, dict) for value in (params, limits, options)):
            raise EngineError("invalid_request", "params, limits and options must be objects")

        return RunToolRequest(
            job=job,
            tool=tool,
            inputs=inputs,
            output_dir=output_dir,
            work_dir=work_dir,
            params=params,
            params_file=params_file,
            limits=limits,
            options=options,
        )

    raise EngineError("invalid_request", f"unsupported op: '{op}'")
