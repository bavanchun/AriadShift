"""The engine as a process: describe, conversions without models, sessions, stdout discipline."""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

import pytest
from conftest import FIXTURES, PROTOCOL, clean_env, convert_request, events, run_engine

DESCRIBE = {"protocol": PROTOCOL, "job": "describe-1", "op": "describe"}


def assert_conforms(stream: list[dict]) -> None:
    results = [index for index, event in enumerate(stream) if event["type"] == "result"]
    assert results == [len(stream) - 1], "exactly one result, and it comes last"


def test_describe_reports_versions_routes_and_sessions_without_paths():
    completed = run_engine(DESCRIBE)
    assert completed.returncode == 0
    stream = events(completed)
    assert [event["type"] for event in stream] == ["capabilities", "result"]
    capabilities = stream[0]
    assert capabilities["engine"] == "docling"
    assert capabilities["license"] == "MIT"
    assert capabilities["supports_session"] is True
    assert capabilities["enforces_memory_limit"] is sys.platform.startswith("linux")
    assert capabilities["tool"]["name"] == "tesseract"
    assert capabilities["tool"]["status"] in {"found", "missing", "wrong_version"}
    assert {"input": "pdf", "output": "docling+json"} in capabilities["routes"]
    assert len(capabilities["routes"]) == 8
    serialized = json.dumps(capabilities)
    assert "/" not in json.dumps(capabilities["tool"])
    assert str(Path.home()) not in serialized


def test_invalid_requests_get_one_typed_result():
    stream = events(run_engine(b"{}\n"))
    assert stream[-1]["error"]["code"] == "invalid_request"
    assert_conforms(stream)
    stream = events(run_engine(b"\xff\xfe\n"))
    assert stream[-1]["error"]["code"] == "invalid_request"


def test_unsupported_routes_and_missing_inputs_are_typed(workspace):
    request = convert_request(workspace, FIXTURES / "md" / "en-crlf.md", "markdown")
    assert events(run_engine(request))[-1]["error"]["code"] == "unsupported_route"
    request = convert_request(workspace, FIXTURES / "docx" / "vi-route-table.docx", "docx")
    request["output"]["format"] = "ariad-ir+json"
    assert events(run_engine(request))[-1]["error"]["code"] == "unsupported_route"
    request = convert_request(workspace, FIXTURES / "docx" / "vi-route-table.docx", "docx")
    request["input"]["path"] = str(workspace / "in" / "missing.docx")
    assert events(run_engine(request))[-1]["error"]["code"] == "io"


def test_input_size_limit_is_enforced(workspace):
    request = convert_request(
        workspace,
        FIXTURES / "docx" / "vi-route-table.docx",
        "docx",
        limits={"max_nesting_depth": 64, "max_input_bytes": 10},
    )
    assert events(run_engine(request))[-1]["error"]["code"] == "limit_exceeded"


def test_converts_docx_to_docling_json_inside_the_output_directory(workspace):
    request = convert_request(workspace, FIXTURES / "docx" / "vi-route-table.docx", "docx")
    completed = run_engine(request)
    stream = events(completed)
    assert_conforms(stream)
    assert stream[-1]["ok"] is True, completed.stderr.decode(errors="replace")
    artifacts = [event for event in stream if event["type"] == "artifact"]
    assert len(artifacts) == 1
    output = Path(artifacts[0]["path"])
    assert output.parent == workspace / "out"
    assert artifacts[0]["format"] == "docling+json"
    document = json.loads(output.read_text(encoding="utf-8"))
    assert document["schema_name"] == "DoclingDocument"
    assert any("Bảng hành trình" in item["text"] for item in document["texts"])


@pytest.mark.parametrize(
    ("source", "input_format", "expected"),
    [
        (FIXTURES / "pptx" / "vi-garden-slides.pptx", "pptx", "Lịch chăm vườn"),
        (FIXTURES / "xlsx" / "en-repair-log.xlsx", "xlsx", "Desk lamp"),
    ],
)
def test_converts_presentations_and_workbooks(workspace, source, input_format, expected):
    stream = events(run_engine(convert_request(workspace, source, input_format)))
    assert stream[-1]["ok"] is True
    document = json.loads((workspace / "out" / "document.docling.json").read_text("utf-8"))
    texts = [item["text"] for item in document["texts"]]
    cells = [cell["text"] for table in document["tables"] for cell in table["data"]["table_cells"]]
    assert any(expected in text for text in texts + cells)


def test_session_reuses_one_process_and_rejects_foreign_jobs(workspace):
    source = FIXTURES / "docx" / "vi-route-table.docx"
    first = convert_request(workspace, source, "docx", job="a", session_id="s1")
    second = convert_request(workspace, source, "docx", job="b", session_id="s1")
    foreign = convert_request(workspace, source, "docx", job="c", session_id="s2")
    completed = run_engine(first, second, foreign, DESCRIBE)
    assert completed.returncode == 0
    results = [event for event in events(completed) if event["type"] == "result"]
    assert [result["ok"] for result in results] == [True, True, False, False]
    assert results[2]["error"]["code"] == "invalid_request"
    assert results[3]["error"]["code"] == "invalid_request"


def test_one_shot_requests_ignore_later_lines(workspace):
    source = FIXTURES / "docx" / "vi-route-table.docx"
    completed = run_engine(convert_request(workspace, source, "docx"), DESCRIBE)
    results = [event for event in events(completed) if event["type"] == "result"]
    assert len(results) == 1


def test_stray_writes_to_stdout_never_reach_the_protocol_stream(workspace):
    request = convert_request(workspace, FIXTURES / "docx" / "vi-route-table.docx", "docx")
    noisy = (
        "import os, runpy\n"
        "import ariad_docling.engine as engine\n"
        "original = engine.handle\n"
        "def handle(*args, **kwargs):\n"
        "    print('stray print')\n"
        "    os.write(1, b'native write\\n')\n"
        "    return original(*args, **kwargs)\n"
        "engine.handle = handle\n"
        "runpy.run_module('ariad_docling', run_name='__main__')\n"
    )
    completed = subprocess.run(
        [sys.executable, "-I", "-c", noisy],
        input=json.dumps(request).encode() + b"\n",
        capture_output=True,
        env=clean_env(),
        timeout=600,
        check=False,
    )
    stream = events(completed)
    assert stream[-1]["ok"] is True
    assert b"stray print" in completed.stderr
    assert b"native write" in completed.stderr


@pytest.mark.skipif(not sys.platform.startswith("linux"), reason="RLIMIT_DATA is Linux-only")
def test_memory_limit_covers_the_pdf_probe_before_models_load(workspace):
    request = convert_request(
        workspace,
        FIXTURES / "pdf" / "en-map-room.pdf",
        "pdf",
        limits={"max_nesting_depth": 64, "max_memory_mb": 64},
    )
    stream = events(run_engine(request))
    assert_conforms(stream)
    assert stream[-1]["error"]["code"] == "limit_exceeded"


@pytest.mark.skipif(not sys.platform.startswith("linux"), reason="RLIMIT_DATA is Linux-only")
def test_memory_limit_failures_are_typed(workspace):
    request = convert_request(
        workspace,
        FIXTURES / "docx" / "vi-route-table.docx",
        "docx",
        limits={"max_nesting_depth": 64, "max_memory_mb": 64},
    )
    completed = run_engine(request)
    stream = events(completed)
    assert_conforms(stream)
    assert stream[-1]["error"]["code"] == "limit_exceeded"
