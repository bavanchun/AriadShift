"""Read-only validation of the authored editor corpus and published IR schema."""
from __future__ import annotations

import argparse
import json
from pathlib import Path

import jsonschema


def check(root: Path) -> list[str]:
    from ariad_fixture_gen.__main__ import _check
    from ariad_fixture_gen.editor_roundtrip import generate

    errors = _check(root, generate(root))
    schema = json.loads((root / "schemas/ir.v2.json").read_text(encoding="utf-8"))
    validator = jsonschema.Draft202012Validator(schema)
    sources = sorted((root / "fixtures/editor").glob("*.ir.json"))
    if len(sources) != 20:
        errors.append(f"Expected 20 authored IR documents, found {len(sources)}")
    for source in [*sources, *(root / "fixtures/editor").glob("vi-styled-pair-*.truth.json")]:
        document = json.loads(source.read_text(encoding="utf-8"))
        document = document.get("ir", document)
        errors.extend(f"{source.name}: {error.message}" for error in validator.iter_errors(document))
    return errors


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", required=True)
    parser.parse_args(argv)
    errors = check(Path(__file__).resolve().parents[3])
    if errors:
        parser.exit(1, "\n".join(errors) + "\n")
    print("Editor corpus bytes, manifest hashes and all authored /2 schemas are valid; no files changed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
