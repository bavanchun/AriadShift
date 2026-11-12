"""Writers for local JSON, CSV and standards-compliant Office artifacts."""

from __future__ import annotations

import csv
import html
import json
import textwrap
import unicodedata
import xml.etree.ElementTree as ET
import zipfile
from pathlib import Path
from typing import Any

from pypdf import PdfWriter
from pypdf.generic import (
    ArrayObject,
    DecodedStreamObject,
    DictionaryObject,
    FloatObject,
    NameObject,
    NumberObject,
)

from ariad_ai.document import Table, safe_xml, text_of

XML_HEADER = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
SHEET_NS = "http://schemas.openxmlformats.org/spreadsheetml/2006/main"
REL_NS = "http://schemas.openxmlformats.org/package/2006/relationships"
DOC_REL_NS = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
TYPE_NS = "http://schemas.openxmlformats.org/package/2006/content-types"


def json_file(path: Path, value: Any) -> None:
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def office_zip(path: Path, parts: dict[str, str], main: str, kind: str) -> None:
    types = (
        f'{XML_HEADER}<Types xmlns="{TYPE_NS}">'
        '<Default Extension="rels" '
        'ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
        '<Default Extension="xml" ContentType="application/xml"/>'
        f'<Override PartName="/{main}" ContentType="{kind}"/>'
    )
    if "xl/worksheets/sheet1.xml" in parts:
        types += (
            '<Override PartName="/xl/worksheets/sheet1.xml" '
            'ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>'
        )
    parts["[Content_Types].xml"] = types + "</Types>"
    parts["_rels/.rels"] = (
        f'{XML_HEADER}<Relationships xmlns="{REL_NS}">'
        f'<Relationship Id="rId1" Type="{DOC_REL_NS}/officeDocument" '
        f'Target="{main}"/></Relationships>'
    )
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as archive:
        for name, content in parts.items():
            archive.writestr(name, content)


def xlsx_file(path: Path, rows: list[list[str]]) -> None:
    sheet = ET.Element("worksheet", xmlns=SHEET_NS)
    data = ET.SubElement(sheet, "sheetData")
    for y, values in enumerate(rows, 1):
        row = ET.SubElement(data, "row", r=str(y))
        for x, value in enumerate(values, 1):
            column = ""
            while x:
                x, remainder = divmod(x - 1, 26)
                column = chr(65 + remainder) + column
            cell = ET.SubElement(row, "c", r=f"{column}{y}", t="inlineStr")
            text = ET.SubElement(ET.SubElement(cell, "is"), "t")
            text.set("{http://www.w3.org/XML/1998/namespace}space", "preserve")
            text.text = value
    office_zip(
        path,
        {
            "xl/workbook.xml": (
                f'{XML_HEADER}<workbook xmlns="{SHEET_NS}" xmlns:r="{DOC_REL_NS}">'
                '<sheets><sheet name="Table" sheetId="1" r:id="rId1"/></sheets></workbook>'
            ),
            "xl/_rels/workbook.xml.rels": (
                f'{XML_HEADER}<Relationships xmlns="{REL_NS}"><Relationship Id="rId1" '
                f'Type="{DOC_REL_NS}/worksheet" Target="worksheets/sheet1.xml"/></Relationships>'
            ),
            "xl/worksheets/sheet1.xml": ET.tostring(sheet, encoding="unicode"),
        },
        "xl/workbook.xml",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml",
    )


def table_file(path: Path, table: Table, fmt: str) -> None:
    if fmt == "json":
        json_file(path, {"page": table.page, "rows": table.rows})
    elif fmt == "xlsx":
        xlsx_file(path, table.rows)
    else:
        with path.open("w", encoding="utf-8", newline="") as stream:
            csv.writer(stream).writerows(table.rows)


def ir_markdown(document: dict[str, Any]) -> str:
    def inline(node: Any) -> str:
        if isinstance(node, list):
            return "".join(inline(n) for n in node)
        if not isinstance(node, dict):
            return ""
        content = inline(node.get("content", []))
        kind = node.get("type")
        if kind == "text":
            return node.get("text", "")
        if kind in {"strong", "emphasis", "strikeout", "code"}:
            marker = {"strong": "**", "emphasis": "*", "strikeout": "~~", "code": "`"}[kind]
            return marker + (node.get("text", "") if kind == "code" else content) + marker
        if kind == "link":
            return f"[{content}]({node.get('target', node.get('url', ''))})"
        if kind == "styled":
            direct = node.get("direct", {})
            if direct.get("weight", 400) >= 600:
                content = "**" + content + "**"
            if direct.get("italic"):
                content = "*" + content + "*"
        return content

    def blocks(nodes: list[dict[str, Any]]) -> str:
        parts = []
        for node in nodes:
            kind = node.get("type")
            content = inline(node.get("content", []))
            if kind == "heading":
                parts.append("#" * min(6, max(1, node.get("level", 1))) + " " + content)
            elif kind == "list":
                for index, item in enumerate(node.get("items", []), node.get("start") or 1):
                    prefix = f"{index}. " if node.get("ordered") else "- "
                    parts.append(prefix + blocks(item.get("blocks", [])))
            elif kind == "table":
                rows = node.get("head", []) + node.get("body", [])
                for index, row in enumerate(rows):
                    parts.append("| " + " | ".join(text_of(cell) for cell in row) + " |")
                    if index == 0:
                        parts.append("| " + " | ".join("---" for _ in row) + " |")
            elif kind == "quote":
                parts.append("> " + blocks(node.get("blocks", [])).replace("\n", "\n> "))
            elif kind == "code_block":
                parts.append("```\n" + node.get("text", "") + "\n```")
            else:
                parts.append(content or blocks(node.get("blocks", [])))
        return "\n\n".join(parts)

    return blocks(document.get("body", document.get("blocks", [])))


def docx_file(path: Path, text: str) -> None:
    ns = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
    root = ET.Element("w:document", {"xmlns:w": ns})
    body = ET.SubElement(root, "w:body")
    for line in text.splitlines():
        paragraph = ET.SubElement(body, "w:p")
        run = ET.SubElement(paragraph, "w:r")
        ET.SubElement(run, "w:t").text = line
    office_zip(
        path,
        {"word/document.xml": ET.tostring(root, encoding="unicode")},
        "word/document.xml",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
    )


def pdf_file(path: Path, text: str) -> None:
    """Searchable Unicode PDF, without a network fetch or system font dependency.

    A Type 3 font draws standard Helvetica glyphs and combining Vietnamese accents.
    Its ToUnicode map preserves the original characters during text extraction.
    """
    writer = PdfWriter()
    base = writer._add_object(
        DictionaryObject(
            {
                NameObject("/Type"): NameObject("/Font"),
                NameObject("/Subtype"): NameObject("/Type1"),
                NameObject("/BaseFont"): NameObject("/Helvetica"),
            }
        )
    )
    lines = [
        line for paragraph in text.splitlines() for line in (textwrap.wrap(paragraph, 85) or [""])
    ]
    for start in range(0, max(1, len(lines)), 55):
        group = lines[start : start + 55]
        chars = sorted(set("".join(group)))
        if len(chars) > 255:
            raise ValueError("a PDF page exceeds 255 unique glyphs; request JSON or DOCX")
        codes = {char: index for index, char in enumerate(chars, 1)}
        procs = DictionaryObject()
        differences = ArrayObject([NumberObject(1)])
        mappings = []
        for char, code in codes.items():
            name = NameObject(f"/g{code}")
            differences.append(name)
            decomposed = unicodedata.normalize("NFD", char)
            letter = decomposed[0].replace("đ", "d").replace("Đ", "D")
            base_char = letter if 32 <= ord(letter) < 127 else "?"
            escaped = base_char.replace("\\", "\\\\").replace("(", "\\(").replace(")", "\\)")
            commands = f"600 0 d0 BT /FB 950 Tf 0 0 Td ({escaped}) Tj ET\n"
            marks = {
                "\u0301": "250 780 m 420 920 l S",
                "\u0300": "250 920 m 420 780 l S",
                "\u0309": "250 820 m 380 950 470 820 330 780 c S",
                "\u0303": "150 830 m 250 940 350 720 470 840 c S",
                "\u0323": "280 -120 45 45 re f",
                "\u0302": "140 780 m 300 920 l 460 780 l S",
                "\u0306": "130 900 m 250 710 370 710 490 900 c S",
                "\u031b": "470 650 m 610 680 610 790 560 830 c S",
            }
            for accent in decomposed[1:]:
                commands += "35 w " + marks.get(accent, "") + "\n"
            if char in {"đ", "Đ"}:
                commands += "35 w 90 360 m 580 360 l S\n"
            proc = DecodedStreamObject()
            proc.set_data(commands.encode("ascii"))
            procs[name] = writer._add_object(proc)
            mappings.append(f"<{code:02X}> <{char.encode('utf-16-be').hex()}>")
        cmap = DecodedStreamObject()
        cmap.set_data(
            (
                "/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n"
                "/CIDSystemInfo << /Registry (Ariad) /Ordering (Unicode) /Supplement 0 >> def\n"
                "/CMapName /AriadUnicode def /CMapType 2 def\n"
                "1 begincodespacerange <00> <FF> endcodespacerange\n"
                + f"{len(mappings)} beginbfchar\n"
                + "\n".join(mappings)
                + "\nendbfchar endcmap CMapName currentdict /CMap defineresource pop end end"
            ).encode("ascii")
        )
        font = writer._add_object(
            DictionaryObject(
                {
                    NameObject("/Type"): NameObject("/Font"),
                    NameObject("/Subtype"): NameObject("/Type3"),
                    NameObject("/FontBBox"): ArrayObject(
                        [NumberObject(v) for v in [0, -200, 650, 1100]]
                    ),
                    NameObject("/FontMatrix"): ArrayObject(
                        [FloatObject(v) for v in [0.001, 0, 0, 0.001, 0, 0]]
                    ),
                    NameObject("/CharProcs"): procs,
                    NameObject("/Encoding"): DictionaryObject(
                        {
                            NameObject("/Type"): NameObject("/Encoding"),
                            NameObject("/Differences"): differences,
                        }
                    ),
                    NameObject("/FirstChar"): NumberObject(1),
                    NameObject("/LastChar"): NumberObject(max(1, len(chars))),
                    NameObject("/Widths"): ArrayObject([NumberObject(600)] * max(1, len(chars))),
                    NameObject("/Resources"): DictionaryObject(
                        {
                            NameObject("/Font"): DictionaryObject(
                                {
                                    NameObject("/FB"): base,
                                }
                            )
                        }
                    ),
                    NameObject("/ToUnicode"): writer._add_object(cmap),
                }
            )
        )
        page = writer.add_blank_page(595, 842)
        page[NameObject("/Resources")] = DictionaryObject(
            {
                NameObject("/Font"): DictionaryObject(
                    {
                        NameObject("/F0"): font,
                    }
                )
            }
        )
        content = DecodedStreamObject()
        commands = ["BT /F0 10 Tf 14 TL 40 800 Td"]
        for line in group:
            encoded = bytes(codes[c] for c in line).hex()
            commands.append(f"<{encoded}> Tj T*")
        commands.append("ET")
        content.set_data("\n".join(commands).encode("ascii"))
        page[NameObject("/Contents")] = writer._add_object(content)
    writer.write(path)


def html_file(path: Path, text: str) -> None:
    path.write_text(
        '<!doctype html><html><head><meta charset="utf-8"></head><body><pre>'
        + html.escape(text)
        + "</pre></body></html>",
        encoding="utf-8",
    )


def translate_docx(source: Path, dest: Path, translator: Any) -> None:
    with (
        zipfile.ZipFile(source) as incoming,
        zipfile.ZipFile(dest, "w", zipfile.ZIP_DEFLATED) as out,
    ):
        if sum(info.file_size for info in incoming.infolist()) > 64 * 1024 * 1024:
            raise ValueError("DOCX uncompressed content exceeds 64 MiB")
        for info in incoming.infolist():
            content = incoming.read(info)
            if info.filename.startswith("word/") and info.filename.endswith(".xml"):
                if b"<!DOCTYPE" in content or b"<!ENTITY" in content:
                    raise ValueError("XML entities are not allowed")
                root = safe_xml(content)
                for node in root.iter():
                    if node.tag.endswith("}t") and node.text:
                        node.text = translator.text(node.text)
                content = ET.tostring(root, encoding="utf-8", xml_declaration=True)
            out.writestr(info, content)
