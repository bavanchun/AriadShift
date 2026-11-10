"""The integrated corpus checker must catch drift without rewriting authored input."""
import shutil
from pathlib import Path

import pytest

from ariad_bench.editor_roundtrip import check

ROOT = Path(__file__).resolve().parents[2]


def test_integrated_editor_corpus_satisfies_bytes_hashes_and_schema():
    assert check(ROOT) == []


@pytest.mark.parametrize("drift", ["bytes", "schema", "missing"])
def test_editor_corpus_check_rejects_drift_without_mutation(tmp_path, drift):
    shutil.copytree(ROOT / "fixtures/editor", tmp_path / "fixtures/editor")
    shutil.copy2(ROOT / "fixtures/manifest.toml", tmp_path / "fixtures/manifest.toml")
    shutil.copytree(ROOT / "schemas", tmp_path / "schemas")
    target = tmp_path / "fixtures/editor/linked-footnote.ir.json"
    if drift == "missing":
        target.unlink()
    elif drift == "schema":
        target.write_text(target.read_text(encoding="utf-8").replace('"ariad-ir/2"', '"ariad-ir/99"'), encoding="utf-8")
    else:
        target.write_bytes(target.read_bytes() + b"\n")
    before = {p: p.read_bytes() for p in tmp_path.rglob("*") if p.is_file()}
    assert check(tmp_path)
    assert before == {p: p.read_bytes() for p in tmp_path.rglob("*") if p.is_file()}
