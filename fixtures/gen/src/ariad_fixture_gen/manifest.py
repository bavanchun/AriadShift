"""Update integrity hashes in the human-maintained fixture manifest."""

from __future__ import annotations

import hashlib
import re
import tomllib
from pathlib import Path

_FIXTURE_HEADER = re.compile(r"(?m)(?=^\[\[fixture\]\]\s*$)")


def update_generated_hashes(root: Path, generated_paths: set[str]) -> None:
    manifest_path = root / "fixtures" / "manifest.toml"
    manifest_text = manifest_path.read_text(encoding="utf-8")
    records = tomllib.loads(manifest_text).get("fixture", [])
    expected_generated_paths: set[str] = set()
    hashes: dict[str, str] = {}

    for record in records:
        if record["origin"] != "generated":
            continue
        candidate_paths = [record["path"], *(item["path"] for item in record.get("companions", []))]
        for relative_path in candidate_paths:
            if relative_path not in generated_paths:
                continue
            if not relative_path.startswith("fixtures/") or ".." in Path(relative_path).parts:
                raise ValueError(f"Generated fixture path escapes the repository: {relative_path}")
            file_path = root / relative_path
            if not file_path.is_file():
                raise FileNotFoundError(f"Generated fixture is missing: {relative_path}")
            expected_generated_paths.add(relative_path)
            hashes[relative_path] = hashlib.sha256(file_path.read_bytes()).hexdigest()

    if expected_generated_paths != generated_paths:
        missing = sorted(generated_paths - expected_generated_paths)
        raise ValueError(f"Generated paths are not declared as generated fixtures: {missing}")

    updated_parts: list[str] = []
    for block in _FIXTURE_HEADER.split(manifest_text):
        if not block.strip():
            updated_parts.append(block)
            continue
        record = tomllib.loads(block).get("fixture", [None])[0]
        if record is None or record["origin"] != "generated":
            updated_parts.append(block)
            continue

        if record["path"] in hashes:
            digest = hashes[record["path"]]
            block, count = re.subn(
                r'(?m)^sha256\s*=\s*"[^"]*"\s*$',
                f'sha256 = "{digest}"',
                block,
                count=1,
            )
            if count != 1:
                raise ValueError(f"Fixture has no top-level sha256 field: {record['path']}")

        for companion in record.get("companions", []):
            if companion["path"] not in hashes:
                continue
            digest = hashes[companion["path"]]
            path_literal = re.escape(companion["path"])
            block, count = re.subn(
                rf'(\{{\s*path\s*=\s*"{path_literal}"\s*,\s*sha256\s*=\s*")[^"]*("\s*\}})',
                rf'\g<1>{digest}\g<2>',
                block,
                count=1,
            )
            if count != 1:
                raise ValueError(f"Fixture companion has no inline sha256 field: {companion['path']}")

        updated_parts.append(block)

    updated_text = "".join(updated_parts)
    if updated_text != manifest_text:
        manifest_path.write_text(updated_text, encoding="utf-8", newline="\n")
