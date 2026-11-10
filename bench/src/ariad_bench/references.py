"""Reference resolution for benchmark fixtures.

Per Principle 5 ("Evidence over claims"):
- Pandoc reader edges are never scored against Pandoc's own reading.
- Generated fixtures use authored truth companions written by fixtures/gen.
- Markdown fixtures use Pandoc's GFM reader as an independent implementation.
- Unscored fixtures (e.g. public government DOCX files or edge cases exceeding canonical form) return None.
"""

from __future__ import annotations

import json
import subprocess
import tempfile
import xml.etree.ElementTree as ET
import zipfile
from contextlib import ExitStack
from pathlib import Path
from typing import Any

from ariad_bench.canonical import CanonicalDoc, normalize


class ReferenceResolutionError(RuntimeError):
    """Raised when an expected reference cannot be resolved or computed."""


# These writer targets have no base ashift reader, or intentionally lose IR
# structure. Use an independent reader rather than requiring a round trip.
INDEPENDENT_READBACK_FORMATS = {"txt", "csv", "xlsx", "odt", "rtf", "pptx"}


def editor_assertions_only(data: dict[str, Any]) -> bool:
    """Preservation assertions do not describe a complete scoring reference."""
    return (
        data.get("schema") == "ariad-editor-truth/1"
        and isinstance(data.get("semantics"), dict)
        and not isinstance(data.get("canonical"), dict)
        and not isinstance(data.get("ir"), dict)
    )


def _xlsx_for_pandoc(path: Path, directory: Path) -> Path:
    """Express inline strings as shared strings for Pandoc's XLSX reader.

    Pandoc 3.12 silently drops inlineStr cell contents. Change only the string
    storage in a temporary copy, using the workbook's actual cells as evidence.
    """
    ns = "{http://schemas.openxmlformats.org/spreadsheetml/2006/main}"
    rel_ns = "{http://schemas.openxmlformats.org/package/2006/relationships}"
    type_ns = "{http://schemas.openxmlformats.org/package/2006/content-types}"
    string_type = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings"
    replacements: dict[str, bytes] = {}
    with zipfile.ZipFile(path) as source:
        names = source.namelist()
        strings = (
            ET.fromstring(source.read("xl/sharedStrings.xml"))
            if "xl/sharedStrings.xml" in names else ET.Element(ns + "sst")
        )
        for name in names:
            if not (name.startswith("xl/worksheets/") and name.endswith(".xml")):
                continue
            sheet = ET.fromstring(source.read(name))
            changed = False
            for cell in sheet.iter(ns + "c"):
                if cell.get("t") != "inlineStr":
                    continue
                inline = cell.find(ns + "is")
                if inline is None:
                    raise ValueError("inline string cell has no string content")
                index = len(strings)
                cell.remove(inline)
                inline.tag = ns + "si"
                strings.append(inline)
                cell.set("t", "s")
                ET.SubElement(cell, ns + "v").text = str(index)
                changed = True
            if changed:
                replacements[name] = ET.tostring(sheet, encoding="utf-8")
        if not replacements:
            return path
        # Counts are optional; omit stale values after appending string entries.
        strings.attrib.pop("count", None)
        strings.attrib.pop("uniqueCount", None)
        replacements["xl/sharedStrings.xml"] = ET.tostring(strings, encoding="utf-8")
        rels = ET.fromstring(source.read("xl/_rels/workbook.xml.rels"))
        if not any(rel.get("Type") == string_type for rel in rels):
            used_ids = {rel.get("Id") for rel in rels}
            identifier = "sharedStrings"
            while identifier in used_ids:
                identifier += "_"
            ET.SubElement(rels, rel_ns + "Relationship", {
                "Id": identifier, "Type": string_type, "Target": "sharedStrings.xml",
            })
            replacements["xl/_rels/workbook.xml.rels"] = ET.tostring(rels, encoding="utf-8")
        types = ET.fromstring(source.read("[Content_Types].xml"))
        if not any(part.get("PartName") == "/xl/sharedStrings.xml" for part in types):
            ET.SubElement(types, type_ns + "Override", {
                "PartName": "/xl/sharedStrings.xml",
                "ContentType": "application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml",
            })
            replacements["[Content_Types].xml"] = ET.tostring(types, encoding="utf-8")
        output = directory / "readback.xlsx"
        with zipfile.ZipFile(output, "w") as destination:
            for info in source.infolist():
                destination.writestr(info, replacements.pop(info.filename, source.read(info)))
            for name, data in replacements.items():
                destination.writestr(name, data, compress_type=zipfile.ZIP_DEFLATED)
        return output


def read_writer_output(path: Path, target: str, pandoc_bin: Path | str) -> CanonicalDoc:
    """Read actual writer output without invoking the writer's own implementation."""
    if target == "txt":
        # Keep structural losses visible in the score; never reconstruct headings
        # or tables from the input IR to make a lossy format look reversible.
        return CanonicalDoc.from_dict({"text": path.read_text(encoding="utf-8")})
    if target not in INDEPENDENT_READBACK_FORMATS:
        raise ValueError(f"No independent writer readback for {target}")
    try:
        with ExitStack() as resources:
            if target == "xlsx":
                directory = resources.enter_context(tempfile.TemporaryDirectory(prefix="ariad-xlsx-readback-"))
                path = _xlsx_for_pandoc(path, Path(directory))
            proc = subprocess.run(
                [str(pandoc_bin), "-f", target, "-t", "json", str(path)],
                capture_output=True,
                text=True,
                encoding="utf-8",
                timeout=60,
                check=True,
            )
    except (subprocess.CalledProcessError, subprocess.TimeoutExpired,
            zipfile.BadZipFile, ET.ParseError, KeyError, ValueError, OSError) as exc:
        raise ReferenceResolutionError(f"Pandoc {target} readback failed: {exc}") from exc
    return normalize(json.loads(proc.stdout))


def get_reference_for_fixture(
    fixture: dict[str, Any],
    root: Path,
    pandoc_bin: Path | str = "pandoc",
) -> CanonicalDoc | None:
    """Resolve the independent canonical reference for a fixture.

    Returns:
        CanonicalDoc if a valid independent reference exists, or None if the fixture
        is unscored (e.g. external document without an authored companion).
    """
    fmt = fixture.get("format", "").lower()
    fixture_path = root / fixture.get("path", "")

    # 1. Authored truth companions for generated fixtures (DOCX, HTML, EPUB)
    companions = fixture.get("companions", [])
    for comp in companions:
        comp_path = root / comp.get("path", "")
        if comp_path.name.endswith(".truth.json") and comp_path.is_file():
            data = json.loads(comp_path.read_text(encoding="utf-8"))
            if data.get("unscored") is True or editor_assertions_only(data):
                return None
            return normalize(data)

    # Also check convention-based path if companions metadata not yet loaded
    fixture_id = fixture.get("id", "")
    direct_companion = fixture_path.parent / f"{fixture_id}.truth.json"
    if direct_companion.is_file():
        data = json.loads(direct_companion.read_text(encoding="utf-8"))
        if data.get("unscored") is True or editor_assertions_only(data):
            return None
        return normalize(data)

    # 2. Markdown fixtures: read by Pandoc GFM reader (independent from our comrak reader)
    if fmt in ("md", "markdown"):
        if not fixture_path.is_file():
            raise ReferenceResolutionError(f"Markdown fixture does not exist: {fixture_path}")

        try:
            proc = subprocess.run(
                [str(pandoc_bin), "-f", "gfm", "-t", "json", str(fixture_path)],
                capture_output=True,
                text=True,
                encoding="utf-8",
                errors="replace",
                timeout=30,
                check=False,
            )
        except subprocess.TimeoutExpired as exc:
            raise ReferenceResolutionError(
                f"Pandoc GFM reader timed out after 30s on {fixture_path}"
            ) from exc

        if proc.returncode != 0:
            raise ReferenceResolutionError(
                f"Pandoc GFM reader failed on {fixture_path}: {proc.stderr.strip()}"
            )
        pandoc_ast = json.loads(proc.stdout)
        return normalize(pandoc_ast)

    # 3. External fixtures without companions are unscored
    return None
