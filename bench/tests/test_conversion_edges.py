"""New conversion edges use real writers and independent output readers."""

import json
import os
import shutil
from pathlib import Path

import pytest
from ariad_bench.canonical import normalize
from ariad_bench.heavy import IR, Bench
from ariad_bench.references import ReferenceResolutionError, read_writer_output
from ariad_bench.run import find_binary, run_benchmark

ROOT = Path(__file__).resolve().parents[2]


@pytest.fixture
def binaries():
    build_dir = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target"))
    ashift = find_binary("ashift", None, [build_dir / "debug/ashift", build_dir / "release/ashift"])
    pandoc = find_binary(
        "pandoc", os.environ.get("ASHIFT_PANDOC"), [ROOT / ".tools/pandoc/bin/pandoc"]
    )
    return ashift, pandoc


@pytest.fixture
def corpus(tmp_path):
    fixtures = tmp_path / "fixtures"
    fixtures.mkdir()
    shutil.copyfile(ROOT / "fixtures/heavy/en-map.docx.truth.json", fixtures / "sample.truth.json")
    (fixtures / "sample.md").write_text("# Notes from the Map Room\n", encoding="utf-8")
    # No target-specific routes: all new writers must find the authored IR corpus.
    (fixtures / "manifest.toml").write_text('''
[[fixture]]
id = "sample"
format = "md"
path = "fixtures/sample.md"
routes = []
[[fixture.companions]]
path = "fixtures/sample.truth.json"
''', encoding="utf-8")
    return tmp_path


@pytest.mark.parametrize("target", ["txt", "csv", "xlsx", "odt", "rtf"])
def test_light_writer_edges_without_native_readback(corpus, binaries, target, capsys):
    ashift, pandoc = binaries
    data_dir = corpus / "crates/ariad-core/data"
    data_dir.mkdir(parents=True)
    template = data_dir / "capabilities.json"
    template.write_text(json.dumps({"edges": [{
        "from": IR, "to": target,
        "engine": "pandoc" if target in ("odt", "rtf") else "ariad-core",
        "metrics": None,
    }]}), encoding="utf-8")
    original = template.read_bytes()
    data = run_benchmark(corpus, ashift, pandoc, repeat_count=1, out_file=corpus / "result.json")
    assert "[fail]" not in capsys.readouterr().out
    metrics = data["edges"][0]["metrics"]
    assert metrics["samples"] == 1
    assert metrics["fidelity"] > 0
    assert template.read_bytes() == original


@pytest.mark.parametrize("target", ["txt", "csv", "xlsx", "odt", "rtf"])
def test_heavy_writer_edges_use_independent_readback(corpus, binaries, target):
    ashift, pandoc = binaries
    truth = json.loads((corpus / "fixtures/sample.truth.json").read_text(encoding="utf-8"))
    source = corpus / "source.ir.json"
    source.write_text(json.dumps(truth["ir"]), encoding="utf-8")
    bench = Bench(corpus, ashift, pandoc, corpus, 1)
    fixture = {
        "id": "sample", "format": IR, "path": str(source),
        "companions": [{"path": "fixtures/sample.truth.json"}],
    }
    engine = "pandoc" if target in ("odt", "rtf") else "ariad-core"
    bench.measure(fixture, engine, target)
    assert len(bench.samples) == 1
    assert bench.samples[0]["fidelity"] > 0
    assert len(bench.edge_scores[(IR, target, engine)]) == 1


def test_plain_text_readback_preserves_text_without_inventing_structure(tmp_path):
    output = tmp_path / "output.txt"
    output.write_text("A heading\n\nA paragraph.\n", encoding="utf-8")
    doc = read_writer_output(output, "txt", "unused")
    assert doc.text == "A heading\nA paragraph."
    assert not doc.headings and not doc.tables


def test_invalid_workbook_readback_is_a_failure(tmp_path, binaries):
    _, pandoc = binaries
    output = tmp_path / "broken.xlsx"
    output.write_bytes(b"invalid workbook")
    with pytest.raises(ReferenceResolutionError, match="xlsx readback failed"):
        read_writer_output(output, "xlsx", pandoc)


def test_xlsx_readback_keeps_actual_inline_cell_text(corpus, binaries):
    ashift, pandoc = binaries
    truth = json.loads((corpus / "fixtures/sample.truth.json").read_text(encoding="utf-8"))
    source = corpus / "source.ir.json"
    source.write_text(json.dumps(truth["ir"]), encoding="utf-8")
    bench = Bench(corpus, ashift, pandoc, corpus, 1)
    output = bench.hook("__write", source, "xlsx")[0]
    original = output.read_bytes()
    doc = read_writer_output(output, "xlsx", pandoc)
    assert "Archive item" in doc.text
    assert "Street plan" in doc.text
    assert "Recorded detail" in doc.text
    assert "Survey date" in doc.text
    assert doc.tables and any(cell.text == "Brass" for table in doc.tables
                              for row in table.rows for cell in row)
    assert output.read_bytes() == original


@pytest.mark.parametrize("target", ["txt", "csv"])
def test_text_only_routes_do_not_require_a_visual_reference(corpus, binaries, target):
    ashift, pandoc = binaries
    truth = json.loads((corpus / "fixtures/sample.truth.json").read_text(encoding="utf-8"))
    source = corpus / "source.ir.json"
    source.write_text(json.dumps(truth["ir"]), encoding="utf-8")
    bench = Bench(corpus, ashift, pandoc, corpus, 1)
    runs = [bench.hook("__write", source, target)]
    bench.record(
        {"id": "sample", "format": IR, "companions": [{"path": "fixtures/sample.truth.json"}]},
        "ariad-core", target, "off", runs, bench.readback(runs[0][0], target), route=True,
    )
    assert bench.samples[0]["fidelity"] > 0
    assert "ssim" not in bench.samples[0]
    assert "visual_renderer" not in bench.samples[0]


@pytest.mark.parametrize("fmt,content,expected", [
    ("txt", "Reader text.\n", "Reader text."),
    ("csv", "Name,Count\nAriad,2\n", "Name\nCount\nAriad\n2"),
])
def test_native_reader_edges_use_authored_truth(tmp_path, binaries, fmt, content, expected, capsys):
    ashift, pandoc = binaries
    fixtures = tmp_path / "fixtures"
    fixtures.mkdir()
    (fixtures / f"sample.{fmt}").write_text(content, encoding="utf-8")
    (fixtures / "sample.truth.json").write_text(json.dumps({"canonical": {
        "text": expected, "headings": [], "tables": [],
    }}), encoding="utf-8")
    (fixtures / "manifest.toml").write_text(f'''
[[fixture]]
id = "sample"
format = "{fmt}"
path = "fixtures/sample.{fmt}"
[[fixture.companions]]
path = "fixtures/sample.truth.json"
''', encoding="utf-8")
    data_dir = tmp_path / "crates/ariad-core/data"
    data_dir.mkdir(parents=True)
    (data_dir / "capabilities.json").write_text(json.dumps({"edges": [{
        "from": fmt, "to": IR, "engine": "ariad-core", "metrics": None,
    }]}), encoding="utf-8")
    data = run_benchmark(tmp_path, ashift, pandoc, repeat_count=1, out_file=tmp_path / "result.json")
    assert "[fail]" not in capsys.readouterr().out
    assert data["edges"][0]["metrics"]["samples"] == 1


class RouteTrace(Bench):
    """Trace orchestration; the tests above exercise real conversion output."""

    def __init__(self, root):
        super().__init__(root, Path("ashift"), Path("pandoc"), root, 1)
        self.calls = []

    def engine(self, engine, path, source, target, ocr="off"):
        self.calls.append((engine, source, target))
        return path, 1, 1

    def convert(self, source, target, engine=None):
        self.calls.append(("convert", source.suffix[1:], target, engine))
        return source, 1, 1

    def hook(self, verb, source, target=IR):
        self.calls.append((verb, target))
        return source, 1, 1

    def readback(self, artifact, target):
        self.calls.append(("readback", target))
        return normalize({"text": "", "headings": [], "tables": []})

    def record(self, *args, **kwargs):
        pass


@pytest.mark.parametrize("source,target,engine", [
    ("pdf", "pptx", "libreoffice"),
    ("odg", "pdf", "libreoffice"),
    ("wpd", "pdf", "libreoffice"),
    ("wps", "pdf", "libreoffice"),
    ("pub", "pdf", "libreoffice"),
    ("vsd", "pdf", "libreoffice"),
    ("cbz", "pdf", "ariad-pdf"),
])
def test_direct_heavy_routes_do_not_invent_intermediate_ir(tmp_path, source, target, engine):
    bench = RouteTrace(tmp_path)
    bench.route({"format": source, "path": f"source.{source}"}, engine, target)
    expected = ("convert", source, target, "libreoffice" if engine == "libreoffice" else None)
    assert bench.calls == [expected, ("readback", target)]


@pytest.mark.parametrize("target", ["odt", "rtf"])
def test_pdf_route_selects_pandoc_writer(tmp_path, target):
    bench = RouteTrace(tmp_path)
    bench.route({"format": "pdf", "path": "source.pdf"}, "pdf", target)
    assert bench.calls == [("pdf", "pdf", IR), ("pandoc", IR, target), ("readback", target)]
