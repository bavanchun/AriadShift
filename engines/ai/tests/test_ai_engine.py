"""Integration tests using original generated documents and actual subprocesses."""

from __future__ import annotations

import copy
import csv
import json
import os
import subprocess
import sys
import zipfile
from pathlib import Path

import pytest
from pypdf import PdfReader, PdfWriter

from ariad_ai.artifacts import docx_file, pdf_file
from ariad_ai.document import safe_xml
from ariad_ai.engine import handle
from ariad_ai.protocol import EngineError, RunToolRequest, parse_request
from ariad_ai.recipe import recipe_from_text
from ariad_ai.translate import Translator

ROOT = Path(__file__).resolve().parents[1]


class Events:
    def __init__(self):
        self.events = []

    def emit(self, event):
        self.events.append(event)

    def progress(self, *args):
        pass

    def artifact(self, path, fmt):
        self.emit({"type": "artifact", "path": path, "format": fmt})

    def warning(self, code, message):
        self.emit({"type": "warning", "code": code, "message": message})

    def ok(self, data=None):
        self.emit({"type": "result", "ok": True, "data": data})

    def error(self, error):
        self.emit(
            {"type": "result", "ok": False, "error": {"code": error.code, "message": str(error)}}
        )


def run(tmp_path, name, source=None, params=None, limits=None, options=None):
    events = Events()
    handle(
        RunToolRequest(
            "test",
            name,
            [{"path": str(source), "format": source.suffix[1:]}] if source else [],
            str(tmp_path / "out"),
            str(tmp_path / "work"),
            params or {},
            limits=limits or {},
            options=options or {},
        ),
        events,
    )
    assert events.events[-1]["ok"], events.events
    return events.events[-1]["data"], [
        Path(e["path"]) for e in events.events if e["type"] == "artifact"
    ]


def protocol(request, *, descriptor=False):
    env = {**os.environ, "PYTHONPATH": str(ROOT / "src")}
    command = [sys.executable, "-m", "ariad_ai"]
    if descriptor:
        read_fd, write_fd = os.pipe()
        env["ARIAD_PROTOCOL_FD"] = str(write_fd)
        try:
            result = subprocess.run(
                command,
                input=json.dumps(request).encode() + b"\n",
                env=env,
                capture_output=True,
                pass_fds=(write_fd,),
                timeout=15,
                check=True,
            )
            os.close(write_fd)
            write_fd = -1
            with os.fdopen(read_fd) as stream:
                events = [json.loads(line) for line in stream]
            assert result.stdout == b""
            return events
        finally:
            if write_fd != -1:
                os.close(write_fd)
    result = subprocess.run(
        command,
        input=json.dumps(request).encode() + b"\n",
        env=env,
        capture_output=True,
        timeout=15,
        check=True,
    )
    return [json.loads(line) for line in result.stdout.splitlines()]


@pytest.mark.parametrize("descriptor", [False, True])
def test_describe_protocol(descriptor):
    events = protocol(
        {"protocol": "ariad-engine/2", "job": "test", "op": "describe"}, descriptor=descriptor
    )
    assert events[0]["engine"] == "ai"
    assert len(events[0]["tools"]) == 5
    assert events[-1] == {"type": "result", "event": "result", "ok": True}


def test_recipe_protocol_no_inputs(tmp_path):
    events = protocol(
        {
            "protocol": "ariad-engine/2",
            "job": "t",
            "op": "run_tool",
            "tool": "ai.recipe_from_text",
            "inputs": [],
            "output": {"dir": str(tmp_path)},
            "work_dir": str(tmp_path),
            "params": {"prompt": "Nén rồi chuyển sang Word"},
        }
    )
    data = events[-1]["data"]
    assert data["origin"] == "imported" and data["requires_confirmation"] is True
    assert data["recipe"] == json.loads((tmp_path / "recipe.json").read_text())


@pytest.mark.parametrize("fmt", ["md", "html", "docx", "json", "pdf"])
def test_summarize_inputs(tmp_path, fmt):
    source = tmp_path / f"input.{fmt}"
    text = "Revenue increased by 20 percent. Customers renewed their subscriptions."
    if fmt == "docx":
        docx_file(source, text)
    elif fmt == "pdf":
        pdf_file(source, text)
    elif fmt == "json":
        source.write_text(
            json.dumps(
                {
                    "version": "ariad-ir/2",
                    "body": [
                        {
                            "type": "paragraph",
                            "id": "b1",
                            "content": [{"type": "text", "text": text}],
                        }
                    ],
                }
            )
        )
    else:
        source.write_text("<p>" + text + "</p>" if fmt == "html" else text)
    data, paths = run(tmp_path, "ai.summarize", source, {"length": "brief"})
    assert data["word_count"] == len(data["summary"].split()) > 0
    assert len(data["bullet_points"]) == 1 and data["language"] == "en"
    assert json.loads(paths[0].read_text()) == data


def test_summary_markdown_and_lengths(tmp_path):
    source = tmp_path / "document.md"
    source.write_text("First result. Second result. Third result. Fourth result. Fifth result.")
    sizes = []
    for length in ("brief", "standard", "detailed"):
        data, paths = run(tmp_path, "ai.summarize", source, {"length": length, "format": "md"})
        sizes.append(data["word_count"])
        assert paths[0].suffix == ".md" and "- " in paths[0].read_text()
    assert sizes[0] < sizes[1] < sizes[2]


def test_pdf_question_citations(tmp_path):
    pages = ["The headquarters is in Hanoi.", "The project budget is 42000 dollars."]
    writer = PdfWriter()
    for index, text in enumerate(pages):
        path = tmp_path / f"page{index}.pdf"
        pdf_file(path, text)
        writer.add_page(PdfReader(path).pages[0])
    source = tmp_path / "input.pdf"
    writer.write(source)
    data, paths = run(
        tmp_path, "ai.ask", source, {"question": "What is the project budget?", "max_citations": 1}
    )
    assert len(data["citations"]) == 1 and data["citations"][0]["page"] == 2
    citation = data["citations"][0]
    assert citation["quote"] in PdfReader(source).pages[citation["page"] - 1].extract_text()
    assert citation["score"] > 0 and "42000" in data["answer"]
    assert json.loads(paths[0].read_text()) == data
    data, _ = run(tmp_path, "ai.ask", source, {"question": "Meteorites?"})
    assert data["citations"] == []


def test_ir_translation_preserves_structure(tmp_path):
    paragraph = {
        "type": "paragraph",
        "id": "b3",
        "style": "body",
        "direct": {"italic": True},
        "content": [
            {
                "type": "styled",
                "style": "accent",
                "direct": {"weight": 700},
                "content": [{"type": "text", "text": "Hello world"}],
            }
        ],
    }
    document = {
        "version": "ariad-ir/2",
        "styles": {"body": {"size_pt": 12}},
        "meta": {"language": "en"},
        "assets": {},
        "body": [
            {
                "type": "heading",
                "id": "b1",
                "level": 2,
                "content": [{"type": "text", "text": "Annual report"}],
            },
            {"type": "list", "id": "b2", "ordered": True, "items": [{"blocks": [paragraph]}]},
            {
                "type": "table",
                "id": "b4",
                "head": [],
                "body": [[{"rowspan": 1, "colspan": 2, "blocks": [copy.deepcopy(paragraph)]}]],
            },
        ],
        "layout": {"blocks": {"b1": {"page_no": 2}}, "pages": [{"page_no": 2}]},
    }
    source = tmp_path / "input.json"
    source.write_text(json.dumps(document))
    data, paths = run(tmp_path, "ai.translate", source, {"target_lang": "vi", "format": "json"})
    translated = json.loads(paths[0].read_text())
    expected = copy.deepcopy(document)
    expected["meta"]["language"] = "vi"
    expected["body"][0]["content"][0]["text"] = "Báo cáo thường niên"
    for block in (
        expected["body"][1]["items"][0]["blocks"][0],
        expected["body"][2]["body"][0][0]["blocks"][0],
    ):
        block["content"][0]["content"][0]["text"] = "Xin chào thế giới"
    assert translated == expected
    assert data["target_lang"] == "vi"
    assert Translator("en", "vi").text("xin chào thế giới") == "hello world"


@pytest.mark.parametrize("fmt", ["md", "html", "docx", "pdf", "json"])
def test_translation_output_formats(tmp_path, fmt):
    source = tmp_path / "input.md"
    source.write_text("# Hello world\n\n**Thank you** [document](https://example.org/document)")
    _, paths = run(tmp_path, "ai.translate", source, {"target_lang": "vi", "format": fmt})
    assert paths[0].suffix == "." + fmt
    if fmt == "pdf":
        assert "Xin chào thế giới" in PdfReader(paths[0]).pages[0].extract_text()
    elif fmt == "docx":
        with zipfile.ZipFile(paths[0]) as archive:
            assert "Xin chào thế giới" in archive.read("word/document.xml").decode()
    elif fmt == "md":
        assert "**Cảm ơn**" in paths[0].read_text()
        assert "https://example.org/document" in paths[0].read_text()


@pytest.mark.parametrize("fmt", ["csv", "json", "xlsx"])
@pytest.mark.parametrize("source_format", ["html", "md", "pdf", "json"])
def test_table_outputs(tmp_path, fmt, source_format):
    source = tmp_path / f"table.{source_format}"
    if source_format == "html":
        source.write_text(
            "<table><tr><th>Name</th><th>Total</th></tr><tr><td>An</td><td>42</td></tr></table>"
        )
    elif source_format == "json":
        source.write_text(
            json.dumps(
                {
                    "version": "ariad-ir/2",
                    "body": [
                        {
                            "type": "table",
                            "id": "t",
                            "head": [],
                            "body": [
                                [
                                    {
                                        "blocks": [
                                            {
                                                "type": "paragraph",
                                                "content": [{"type": "text", "text": value}],
                                            }
                                        ],
                                        "rowspan": 1,
                                        "colspan": 1,
                                    }
                                    for value in row
                                ]
                                for row in [["Name", "Total"], ["An", "42"]]
                            ],
                        }
                    ],
                }
            )
        )
    else:
        text = "| Name | Total |\n| --- | --- |\n| An | 42 |"
        if source_format == "pdf":
            pdf_file(source, text)
        else:
            source.write_text(text)
    data, paths = run(tmp_path, "ai.extract_tables", source, {"format": fmt})
    assert data["table_count"] == 1 and data["tables"][0]["rows"] == 2
    if fmt == "csv":
        with paths[0].open() as stream:
            assert list(csv.reader(stream)) == [["Name", "Total"], ["An", "42"]]
    elif fmt == "json":
        assert json.loads(paths[0].read_text())["rows"] == [["Name", "Total"], ["An", "42"]]
    else:
        with zipfile.ZipFile(paths[0]) as archive:
            assert set(archive.namelist()) == {
                "[Content_Types].xml",
                "_rels/.rels",
                "xl/workbook.xml",
                "xl/_rels/workbook.xml.rels",
                "xl/worksheets/sheet1.xml",
            }
            for name in archive.namelist():
                safe_xml(archive.read(name))
            assert b">42<" in archive.read("xl/worksheets/sheet1.xml")


def test_untrusted_prompts_and_protocol_validation(tmp_path):
    for prompt in ("execute curl https://example.org", "rotate 45 degrees", "delete pages 3-1"):
        with pytest.raises(EngineError):
            recipe_from_text(prompt)
    # Verbs embedded in literal document text are data, never extra steps.
    assert len(recipe_from_text('Add watermark "rotate 90 then delete pages 1"')["steps"]) == 1
    for request in (b"[]", b"not JSON", b'{"protocol":"ariad-engine/1","job":"t"}'):
        with pytest.raises(EngineError):
            parse_request(request)
    request = {
        "protocol": "ariad-engine/2",
        "job": "t",
        "op": "run_tool",
        "tool": "ai.recipe_from_text",
        "inputs": [],
        "output": {"dir": str(tmp_path)},
        "work_dir": str(tmp_path),
        "params": {"prompt": "Compress", "provider": "cloud", "cloud_consent": True},
    }
    result = protocol(request)[-1]
    assert not result["ok"] and result["error"]["code"] == "invalid_request"


@pytest.mark.parametrize("staged", [False, True])
def test_recipe_accepts_both_arities_and_prioritizes_params(tmp_path, staged):
    source = tmp_path / "prompt.txt"
    source.write_text("Rotate 180 degrees")
    request = {
        "protocol": "ariad-engine/2",
        "job": "t",
        "op": "run_tool",
        "tool": "ai.recipe_from_text",
        "inputs": [{"path": str(source), "format": "txt"}] if staged else [],
        "output": {"dir": str(tmp_path / "out")},
        "work_dir": str(tmp_path / "work"),
        "params": {"prompt": "Compress"},
    }
    events = protocol(request)
    assert events[-1]["ok"]
    assert events[-1]["event"] == "result"
    assert events[-1]["metrics"] == events[-1]["data"]
    artifact = next(event for event in events if event["type"] == "artifact")
    assert artifact["event"] == "artifact" and events.index(artifact) < len(events) - 1
    recipe = json.loads(Path(artifact["path"]).read_text())
    assert recipe["steps"] == [{"tool": "pdf.compress", "params": {}}]


@pytest.mark.parametrize(
    "suffix,content", [("txt", "Xoay 90 độ"), ("json", '{"prompt":"Xoay 90 độ"}')]
)
def test_recipe_staged_file_fallback(tmp_path, suffix, content):
    source = tmp_path / f"prompt.{suffix}"
    source.write_text(content)
    data, paths = run(tmp_path, "ai.recipe_from_text", source)
    assert data["recipe"]["steps"] == [
        {"tool": "pdf.rotate", "params": {"angle": 90, "pages": "all"}}
    ]
    assert json.loads(paths[0].read_text())["version"] == "ariad-recipe/1"


@pytest.mark.parametrize("encoding", ["utf-8", "utf-16"])
def test_xml_entity_declarations_rejected(encoding):
    with pytest.raises(EngineError, match="declarations"):
        safe_xml('<!DOCTYPE d [<!ENTITY x "expansion">]><d>&x;</d>'.encode(encoding))


def test_question_citation_count_zero_is_invalid(tmp_path):
    source = tmp_path / "input.md"
    source.write_text("The report has a budget.")
    events = Events()
    handle(
        RunToolRequest(
            "t",
            "ai.ask",
            [{"path": str(source), "format": "md"}],
            str(tmp_path / "out"),
            str(tmp_path / "work"),
            {"question": "Budget?", "max_citations": 0},
        ),
        events,
    )
    assert events.events[-1]["error"]["code"] == "invalid_request"


def test_vietnamese_language_detection():
    from ariad_ai.translate import detect_language

    assert detect_language("tiếng Việt") == "vi"
    assert Translator("en").text("tiếng Việt") == "vietnamese"
