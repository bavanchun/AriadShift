"""Acceptance gates for original documents and the scoped generator CLI."""

from __future__ import annotations

import base64
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tomllib
from zipfile import ZipFile

from lxml import etree
import pytest

from ariad_fixture_gen import editor_roundtrip as corpus

ROOT = Path(__file__).resolve().parents[3]
REQUIRED_CASES = {
    "linked-footnote", "reused-footnote", "nested-footnote", "raw-html-block",
    "raw-tex-block", "inline-raw", "captioned-figure", "contained-figure",
    "table-colspan", "table-rowspan", "table-combined-spans", "table-caption",
    "table-footnotes", "nested-cell-blocks", "running-header", "running-footer",
    "inline-math", "display-math", "styled-nesting", "mixed-document",
}


def _records(root=ROOT):
    return [item for item in tomllib.loads((root / "fixtures/manifest.toml").read_text())["fixture"]
            if item["path"].startswith("fixtures/editor/")]


def _files(root):
    return {str(path.relative_to(root)): path.read_bytes() for path in root.rglob("*") if path.is_file()}


def _run(root, *args):
    # Exercise the real CLI parser and file/hash writer against an isolated root.
    script = "from pathlib import Path; import sys; from ariad_fixture_gen.__main__ import main; root=Path(sys.argv.pop(1)); main(root)"
    return subprocess.run([sys.executable, "-c", script, str(root), "--only", "editor-roundtrip", *args],
        capture_output=True, text=True, timeout=90,
        env={**os.environ, "PYTHONPATH": str(ROOT / "fixtures/gen/src"), "PYTHONDONTWRITEBYTECODE": "1"})


def _sandbox(tmp_path):
    (tmp_path / "fixtures").mkdir()
    (tmp_path / "fixtures/manifest.toml").write_bytes((ROOT / "fixtures/manifest.toml").read_bytes())
    (tmp_path / "fixtures/existing-sentinel.txt").write_bytes(b"existing family: keep these exact bytes\r\n")
    return tmp_path


def test_editor_corpus_has_twenty_distinct_primary_documents():
    records = _records()
    assert len(records) == 24
    assert len({hashlib.sha256((ROOT / item["path"]).read_bytes()).digest() for item in records}) == 24
    assert {Path(item["path"]).name.removesuffix(".ir.json") for item in records if item["path"].endswith(".ir.json")} == REQUIRED_CASES
    assert {item["format"] for item in records} == {"ariad-ir+json", "md", "html", "docx", "pdf"}
    paths = set()
    for record in records:
        assert record["origin"] == "generated" and record["license"] == "Apache-2.0"
        assert record["license_basis"] and record["attribution"] and record["generated_by"]
        for item in [record, *record["companions"]]:
            assert item["path"] not in paths
            paths.add(item["path"])
            assert hashlib.sha256((ROOT / item["path"]).read_bytes()).hexdigest() == item["sha256"]
    assert paths == {str(path.relative_to(ROOT)) for path in (ROOT / "fixtures/editor").iterdir()}


def test_editor_generation_is_byte_deterministic(tmp_path):
    root = _sandbox(tmp_path)
    first = _run(root)
    assert first.returncode == 0, first.stderr
    before = _files(root)
    second = _run(root)
    assert second.returncode == 0, second.stderr
    assert _files(root) == before
    assert before["fixtures/existing-sentinel.txt"] == b"existing family: keep these exact bytes\r\n"
    original = tomllib.loads((ROOT / "fixtures/manifest.toml").read_text())["fixture"]
    regenerated = tomllib.loads(before["fixtures/manifest.toml"].decode())["fixture"]
    assert [item for item in original if not item["path"].startswith(corpus.PREFIX)] == [
        item for item in regenerated if not item["path"].startswith(corpus.PREFIX)]
    assert before["fixtures/manifest.toml"] == (ROOT / "fixtures/manifest.toml").read_bytes()


def test_editor_truth_is_authored_and_covers_styles_and_layout():
    truth = json.loads((ROOT / "fixtures/editor/styled-nesting.truth.json").read_bytes())
    assert truth["origin"] == "independent-original-specification"
    assert truth["ir"]["version"] == "ariad-ir/2"
    assert truth["styles"]["paragraph"] == {"family": "Libertinus Serif", "base_size_pt": 11.0,
        "direct_size_pt": 12.0, "line_height": 1.25, "align": "justify"}
    assert truth["styles"]["character"]["direct_weight"] == 650
    assert truth["styles"]["character"]["italic"] is False
    assert truth["layout"]["first_bbox"] == {"left": 54.0, "top": 60.0, "right": 540.0, "bottom": 76.0}
    for extension in ("docx", "pdf"):
        paired = json.loads((ROOT / f"fixtures/editor/vi-styled-pair-{extension}.truth.json").read_bytes())
        assert paired["semantics"]["paragraph"] == "Bến sông có ba ghế gỗ. Nhóm đọc sách gặp nhau vào sáng thứ bảy."
        assert paired["styles"]["heading_size_pt"] == 18.0
        assert paired["styles"]["heading_weight"] == 700
        assert paired["styles"]["heading_color"] == "#185c4c"
        assert paired["layout"]["page_width_pt"] == 595.0
        assert paired["ir"]["version"] == "ariad-ir/2"
        assert paired["canonical"]["tables"][0]["rows"][1][0]["text"] == "Bến sông"
    # Truth remains available with no generated documents or engine output to read.
    assert corpus._truth("mixed-document")["layout"]["section_count"] == 2


def _nodes(value):
    if isinstance(value, dict):
        yield value
        for item in value.values():
            yield from _nodes(item)
    elif isinstance(value, list):
        for item in value:
            yield from _nodes(item)


def test_authored_documents_have_resolving_ids_notes_assets_and_geometry():
    for case in REQUIRED_CASES:
        document = json.loads((ROOT / f"fixtures/editor/{case}.ir.json").read_bytes())
        nodes = list(_nodes(document))
        blocks = [node for node in nodes if "direct" in node and "id" in node]
        ids = [block["id"] for block in blocks]
        assert len(ids) == len(set(ids)) and ids
        assert all(len(identity) == 65 and identity.startswith("b") for identity in ids)
        assert set(document["layout"]["blocks"]) == set(document["source_map"]) == set(ids)
        notes = {node["note_id"] for node in nodes if node.get("type") == "footnote"}
        refs = [node["id"] for node in nodes if node.get("type") == "footnote_ref"]
        assert set(refs) == notes
        if case in ("reused-footnote", "mixed-document"):
            assert refs.count("survey-note") == (3 if case == "mixed-document" else 2)
        for identity, asset in document["assets"].items():
            assert hashlib.sha256(base64.b64decode(asset["bytes"])).hexdigest() == identity
        for section in document.get("sections", []):
            for field in ("start", "header", "footer"):
                if field in section:
                    assert section[field] in ids
    mixed = json.loads((ROOT / "fixtures/editor/mixed-document.ir.json").read_bytes())
    assert len(mixed["source_map"][mixed["body"][0]["id"]]) == 2
    assert any(node.get("type") == "table" and node.get("style") == "survey" for node in _nodes(mixed))


def test_real_docx_pdf_and_text_containers():
    with ZipFile(ROOT / "fixtures/editor/vi-styled-pair.docx") as archive:
        assert all(item.date_time == (1980, 1, 1, 0, 0, 0) for item in archive.infolist())
        xml = etree.fromstring(archive.read("word/document.xml"))
        ns = {"w": "http://schemas.openxmlformats.org/wordprocessingml/2006/main"}
        assert "Sổ khảo sát ven sông" in xml.xpath("//w:t/text()", namespaces=ns)
        styles = etree.fromstring(archive.read("word/styles.xml"))
        heading = styles.xpath('//w:style[@w:styleId="SurveyHeading"]', namespaces=ns)[0]
        assert heading.xpath(".//w:sz/@w:val", namespaces=ns) == ["36"]
        assert heading.xpath(".//w:color/@w:val", namespaces=ns) == ["185C4C"]
    pdf = (ROOT / "fixtures/editor/vi-styled-pair.pdf").read_bytes()
    assert pdf.startswith(b"%PDF-") and b"%%EOF" in pdf[-40:]
    assert b"/Font" in pdf and b"/MediaBox" in pdf
    assert (ROOT / "fixtures/editor/vi-field-notes.md").read_text().startswith("# Ghi chép bên sông")
    assert 'rowspan="2"' in (ROOT / "fixtures/editor/vi-field-notes.html").read_text()


@pytest.mark.parametrize("drift", ["none", "bytes", "hash", "missing", "undeclared", "unemitted"])
def test_check_is_read_only_and_rejects_drift(tmp_path, drift):
    root = _sandbox(tmp_path)
    result = _run(root)
    assert result.returncode == 0, result.stderr
    path = root / "fixtures/editor/linked-footnote.ir.json"
    manifest = root / "fixtures/manifest.toml"
    record = _records(root)[0]
    if drift == "bytes":
        path.write_bytes(path.read_bytes() + b" ")
    elif drift == "hash":
        manifest.write_text(manifest.read_text().replace(record["sha256"], "0" * 64))
    elif drift == "missing":
        path.unlink()
    elif drift == "undeclared":
        manifest.write_text(manifest.read_text().replace('path = "fixtures/editor/linked-footnote.ir.json"',
            'path = "fixtures/editor/renamed.ir.json"'))
    elif drift == "unemitted":
        manifest.write_text(manifest.read_text() + '\n[[fixture]]\npath = "fixtures/editor/extra.ir.json"\norigin = "generated"\nsha256 = "' + "0" * 64 + '"\n')
    before = _files(root)
    checked = _run(root, "--check")
    assert (checked.returncode == 0) == (drift == "none"), checked.stdout + checked.stderr
    assert _files(root) == before
