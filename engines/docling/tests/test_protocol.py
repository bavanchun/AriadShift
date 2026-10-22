from __future__ import annotations

import io
import json
import os

import pytest

from ariad_docling.protocol import (
    MAX_LINE_BYTES,
    ConvertRequest,
    DescribeRequest,
    EngineError,
    Writer,
    optional_int,
    parse_request,
    read_line,
)


def _convert(**overrides) -> bytes:
    request = {
        "protocol": "ariad-engine/1",
        "job": "j",
        "op": "convert",
        "input": {"path": "/w/in/a.pdf", "format": "pdf"},
        "output": {"dir": "/w/out", "format": "docling+json"},
        "work_dir": "/w/tmp",
        "options": {"ocr": "auto"},
        "limits": {"max_nesting_depth": 64},
    }
    request.update(overrides)
    return json.dumps(request).encode()


def test_parses_convert_and_describe_requests():
    request = parse_request(_convert(session_id="s1"))
    assert isinstance(request, ConvertRequest)
    assert request.input_format == "pdf"
    assert request.options == {"ocr": "auto"}
    assert request.session_id == "s1"
    describe = parse_request(b'{"protocol":"ariad-engine/1","job":"d","op":"describe"}')
    assert describe == DescribeRequest(job="d")


@pytest.mark.parametrize(
    "line",
    [
        b"not json",
        b"[]",
        b'{"protocol":"ariad-engine/999","job":"j","op":"describe"}',
        b'{"protocol":"ariad-engine/1","job":1,"op":"describe"}',
        b'{"protocol":"ariad-engine/1","job":"j","op":"delete"}',
        _convert(limits=None),
        _convert(session_id=7),
        _convert(input={"path": "", "format": "pdf"}),
        _convert(options=[]),
    ],
)
def test_rejects_invalid_requests(line):
    with pytest.raises(EngineError) as error:
        parse_request(line)
    assert error.value.code == "invalid_request"


def test_read_line_bounds_the_request_size():
    assert read_line(io.BytesIO(b"")) is None
    assert read_line(io.BytesIO(b"{}\n{}\n")) == b"{}\n"
    with pytest.raises(EngineError):
        read_line(io.BytesIO(b"x" * (MAX_LINE_BYTES + 10)))


def test_writer_emits_one_ascii_json_line_per_event():
    read_fd, write_fd = os.pipe()
    writer = Writer(write_fd)
    writer.progress(1, 3)
    writer.warning("ocr_fallback", "Tiếng Việt")
    writer.error(EngineError("io", "missing"))
    os.close(write_fd)
    with os.fdopen(read_fd, "rb") as stream:
        lines = stream.read().splitlines()
    assert all(line.isascii() for line in lines)
    assert [json.loads(line)["type"] for line in lines] == ["progress", "warning", "result"]
    assert json.loads(lines[1])["message"] == "Tiếng Việt"


def test_optional_limits_must_be_positive_integers():
    assert optional_int({}, "max_pages") is None
    assert optional_int({"max_pages": None}, "max_pages") is None
    assert optional_int({"max_pages": 3}, "max_pages") == 3
    for value in (0, -1, True, "3", 1.5):
        with pytest.raises(EngineError):
            optional_int({"max_pages": value}, "max_pages")
