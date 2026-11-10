"""Local text and table extraction; no document content is fetched remotely."""

from __future__ import annotations

import json
import re
import xml.etree.ElementTree as ET
import zipfile
from dataclasses import dataclass, field
from html.parser import HTMLParser
from pathlib import Path
from typing import Any

from pypdf import PdfReader

from ariad_ai.protocol import EngineError

MAX_INPUT_BYTES = 64 * 1024 * 1024
MAX_TEXT_CHARS = 2_000_000
WORD_NS = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"


class NoDoctype(ET.TreeBuilder):
    def doctype(self, name: str, pubid: str | None, system: str | None) -> None:
        raise EngineError("invalid_request", "document type declarations are not allowed")


def safe_xml(data: bytes) -> ET.Element:
    if len(data) > MAX_INPUT_BYTES:
        raise EngineError("limit_exceeded", "XML exceeds 64 MiB")
    # The parser rejects every DTD, including UTF-16 encoded entity declarations.
    return ET.fromstring(data, parser=ET.XMLParser(target=NoDoctype()))  # noqa: S314


def text_of(node: Any) -> str:
    if isinstance(node, str):
        return node
    if isinstance(node, list):
        return " ".join(filter(None, (text_of(item) for item in node)))
    if not isinstance(node, dict):
        return ""
    if node.get("type") in {"text", "code", "math"}:
        return str(node.get("text", ""))
    for key in ("content", "blocks", "items", "body"):
        if key in node:
            return text_of(node[key])
    return ""


@dataclass
class Table:
    rows: list[list[str]]
    page: int = 1


@dataclass
class Document:
    pages: list[str]
    tables: list[Table] = field(default_factory=list)
    ir: dict[str, Any] | None = None
    source: str = ""
    format: str = "md"

    @property
    def text(self) -> str:
        return "\n\n".join(self.pages)


class HtmlText(HTMLParser):
    """Extract visible text and cells without executing scripts or loading resources."""

    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.parts: list[str] = []
        self.tables: list[Table] = []
        self.rows: list[list[str]] | None = None
        self.row: list[str] | None = None
        self.cell: list[str] | None = None
        self.hidden = 0

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        if tag in {"script", "style"}:
            self.hidden += 1
        if self.hidden:
            return
        if tag in {"p", "br", "div", "tr", "li", "h1", "h2", "h3"}:
            self.parts.append("\n")
        if tag == "table":
            self.rows = []
        elif tag == "tr":
            self.row = []
        elif tag in {"td", "th"}:
            self.cell = []

    def handle_endtag(self, tag: str) -> None:
        if tag in {"script", "style"}:
            self.hidden = max(0, self.hidden - 1)
            return
        if self.hidden:
            return
        if tag in {"td", "th"} and self.cell is not None:
            if self.row is not None:
                self.row.append("".join(self.cell).strip())
            self.cell = None
        elif tag == "tr" and self.rows is not None and self.row:
            self.rows.append(self.row)
            self.row = None
        elif tag == "table" and self.rows:
            self.tables.append(Table(self.rows))
            self.rows = None
        if tag in {"p", "div", "li", "h1", "h2", "h3"}:
            self.parts.append("\n")

    def handle_data(self, data: str) -> None:
        if not self.hidden:
            self.parts.append(data)
            if self.cell is not None:
                self.cell.append(data)


def line_tables(text: str, page: int = 1) -> list[Table]:
    tables: list[Table] = []
    rows: list[list[str]] = []
    for line in [*text.splitlines(), ""]:
        if "|" in line:
            cells = [c.strip() for c in re.split(r"(?<!\\)\|", line.strip().strip("|"))]
        else:
            cells = re.split(r"\t| {2,}", line.strip())
        if len(cells) > 1 and any(cells):
            if not all(re.fullmatch(r":?-{2,}:?", c) for c in cells):
                rows.append(cells)
        else:
            if len(rows) >= 2:
                tables.append(Table(rows, page))
            rows = []
    return tables


def ir_tables(node: Any, page_by_id: dict[str, int], page: int = 1) -> list[Table]:
    tables: list[Table] = []
    if isinstance(node, list):
        for child in node:
            tables.extend(ir_tables(child, page_by_id, page))
    elif isinstance(node, dict):
        page = page_by_id.get(node.get("id", ""), node.get("page", page))
        if node.get("type") == "table":
            # Preserve the grid, including occupied row/column spans.
            occupied: dict[tuple[int, int], str] = {}
            rows = node.get("head", []) + node.get("body", [])
            for row_index, row in enumerate(rows):
                column = 0
                for cell in row:
                    while (row_index, column) in occupied:
                        column += 1
                    spans = [cell.get("rowspan", 1), cell.get("colspan", 1)]
                    if any(type(span) is not int or not 1 <= span <= 1024 for span in spans):
                        raise EngineError(
                            "invalid_request", "table spans must be between 1 and 1024"
                        )
                    if len(occupied) + spans[0] * spans[1] > 1_000_000:
                        raise EngineError("limit_exceeded", "table grid exceeds one million cells")
                    value = text_of(cell)
                    for dy in range(max(1, int(cell.get("rowspan", 1)))):
                        for dx in range(max(1, int(cell.get("colspan", 1)))):
                            occupied[row_index + dy, column + dx] = value if dx == dy == 0 else ""
                    column += max(1, int(cell.get("colspan", 1)))
            if occupied:
                height = max(y for y, _ in occupied) + 1
                width = max(x for _, x in occupied) + 1
                tables.append(
                    Table(
                        [[occupied.get((y, x), "") for x in range(width)] for y in range(height)],
                        int(page),
                    )
                )
        else:
            for key in ("body", "blocks", "items", "content"):
                tables.extend(ir_tables(node.get(key), page_by_id, int(page)))
    return tables


def read_document(item: dict[str, Any], limits: dict[str, Any]) -> Document:
    path = Path(item["path"])
    if not path.is_file():
        raise EngineError("io", "input file does not exist")
    if path.stat().st_size > MAX_INPUT_BYTES:
        raise EngineError("limit_exceeded", "input exceeds 64 MiB")
    fmt = item.get("format", path.suffix.lstrip(".")).lower()
    doc = Document([], source=str(path), format=fmt)
    if fmt == "pdf":
        reader = PdfReader(path)
        if reader.is_encrypted:
            raise EngineError("encrypted_input", "decrypt the document before text extraction")
        if limits.get("max_pages") and len(reader.pages) > limits["max_pages"]:
            raise EngineError("limit_exceeded", "PDF exceeds max_pages")
        for number, page in enumerate(reader.pages, 1):
            text = page.extract_text(extraction_mode="layout") or ""
            doc.pages.append(text)
            doc.tables.extend(line_tables(text, number))
    elif fmt == "docx":
        with zipfile.ZipFile(path) as archive:
            info = archive.getinfo("word/document.xml")
            if info.file_size > MAX_INPUT_BYTES:
                raise EngineError("limit_exceeded", "DOCX document XML exceeds 64 MiB")
            xml = archive.read(info)
        if b"<!DOCTYPE" in xml or b"<!ENTITY" in xml:
            raise EngineError("invalid_request", "DOCX XML entities are not allowed")
        root = safe_xml(xml)
        ns = {"w": WORD_NS}
        doc.pages = ["\n".join("".join(p.itertext()) for p in root.findall(".//w:p", ns))]
        for table in root.findall(".//w:tbl", ns):
            doc.tables.append(
                Table(
                    [
                        [
                            " ".join(t.text or "" for t in cell.findall(".//w:t", ns))
                            for cell in row.findall("w:tc", ns)
                        ]
                        for row in table.findall("w:tr", ns)
                    ]
                )
            )
    elif fmt in {"json", "ariad-ir+json"}:
        obj = json.loads(path.read_text(encoding="utf-8"))
        if not isinstance(obj, dict) or obj.get("version") != "ariad-ir/2":
            raise EngineError("invalid_request", "expected ariad-ir/2 JSON")
        doc.ir = obj
        layout = obj.get("layout") or {}
        page_by_id = {key: value["page_no"] for key, value in layout.get("blocks", {}).items()}
        for key, value in (obj.get("source_map") or {}).items():
            if value:
                page_by_id.setdefault(key, value[0]["page_no"])
        count = max([1, *page_by_id.values(), *(p["page_no"] for p in layout.get("pages", []))])
        if count > min(limits.get("max_pages") or 10000, 10000):
            raise EngineError("limit_exceeded", "IR exceeds max_pages")
        doc.pages = [""] * count
        for block in obj.get("body", obj.get("blocks", [])):
            page = int(page_by_id.get(block.get("id"), block.get("page", 1)))
            doc.pages[page - 1] += text_of(block) + "\n"
        doc.tables = ir_tables(obj.get("body", obj.get("blocks", [])), page_by_id)
    elif fmt in {"html", "htm"}:
        parser = HtmlText()
        parser.feed(path.read_text(encoding="utf-8"))
        doc.pages = ["".join(parser.parts)]
        doc.tables = parser.tables
    elif fmt in {"md", "markdown", "txt"}:
        doc.pages = path.read_text(encoding="utf-8").split("\f")
        for page, text in enumerate(doc.pages, 1):
            doc.tables.extend(line_tables(text, page))
    else:
        raise EngineError("unsupported_route", "unsupported document format")
    if len(doc.text) > MAX_TEXT_CHARS:
        raise EngineError("limit_exceeded", "extracted text exceeds 2 million characters")
    return doc
