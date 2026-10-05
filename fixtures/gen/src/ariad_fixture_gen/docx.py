"""Generate reproducible DOCX fixtures with structural edge cases."""

from __future__ import annotations

from datetime import datetime
from io import BytesIO
from pathlib import Path
from zipfile import ZIP_STORED, ZipFile, ZipInfo

from docx import Document
from docx.enum.table import WD_TABLE_ALIGNMENT
from docx.oxml.ns import qn
from docx.shared import Inches
from lxml import etree

_WORD_NS = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
_REL_NS = "http://schemas.openxmlformats.org/package/2006/relationships"
_CONTENT_NS = "http://schemas.openxmlformats.org/package/2006/content-types"
_FOOTNOTE_REL = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes"


def _source(root: Path, filename: str) -> list[str]:
    path = root / "fixtures" / "gen" / "src" / "ariad_fixture_gen" / "text" / filename
    return [part.strip() for part in path.read_text(encoding="utf-8").split("\n\n") if part.strip()]


def _document(title: str) -> Document:
    document = Document()
    properties = document.core_properties
    properties.author = "AriadShift contributors"
    properties.last_modified_by = "AriadShift contributors"
    properties.title = title
    properties.subject = "Reproducible document-conversion fixture"
    properties.created = datetime(2000, 1, 1)
    properties.modified = datetime(2000, 1, 1)
    return document


def _set_cell_text(cell, text: str, *, bold: bool = False) -> None:
    cell.text = text
    for paragraph in cell.paragraphs:
        for run in paragraph.runs:
            run.bold = bold


def _styled_report(garden: list[str]) -> bytes:
    document = _document("Khu vườn đọc sách")
    section = document.sections[0]
    section.top_margin = Inches(0.8)
    section.bottom_margin = Inches(0.8)
    section.header.paragraphs[0].text = "Thư viện phường"
    section.footer.paragraphs[0].text = "Sổ chăm vườn"
    document.add_heading("Khu vườn đọc sách", level=1)
    document.add_paragraph(garden[0])
    note = document.add_paragraph(style="Quote")
    note.add_run("Mỗi sáng thứ bảy, một nhóm hàng xóm tưới cây và đổi sách.").italic = True
    document.add_heading("Sinh hoạt hằng tuần", level=2)
    document.add_paragraph(garden[1])
    document.add_paragraph(garden[2])
    return _save(document)


def _numbered_checklist(repair_cafe: list[str]) -> bytes:
    document = _document("Repair café opening checklist")
    document.add_heading("Before the café opens", level=1)
    document.add_paragraph(repair_cafe[0])
    for item in ("Clear the workbench", "Check each lamp", "Label the parts tray"):
        document.add_paragraph(item, style="List Number")
    document.add_paragraph("Ask the visitor before replacing a part.", style="List Bullet")
    document.add_paragraph("Record the repair", style="List Bullet 2")
    document.add_paragraph(repair_cafe[1])
    return _save(document)


def _route_table(river: list[str]) -> bytes:
    document = _document("Bảng hành trình ven sông")
    document.add_heading("Bảng hành trình ven sông", level=1)
    document.add_paragraph(river[0])
    table = document.add_table(rows=1, cols=3)
    table.style = "Light Shading Accent 1"
    table.alignment = WD_TABLE_ALIGNMENT.CENTER
    for cell, heading in zip(table.rows[0].cells, ("Điểm dừng", "Phút đi bộ", "Ghi chú"), strict=True):
        _set_cell_text(cell, heading, bold=True)
    for row in (("Bến sông", "8", "Bóng mát"), ("Vườn đọc", "14", "Có ghế dài")):
        cells = table.add_row().cells
        for cell, value in zip(cells, row, strict=True):
            _set_cell_text(cell, value)
    note = table.add_row().cells
    _set_cell_text(note[0].merge(note[2]), "Lịch khảo sát cập nhật mỗi tháng.")
    document.add_paragraph("Các khoảng thời gian là ước lượng đi bộ.")
    return _save(document)


def _footnote_report(map_room: list[str]) -> bytes:
    document = _document("Map archive note with a footnote")
    document.add_heading("Notes from the Map Room", level=1)
    paragraph = document.add_paragraph(map_room[0])
    paragraph.add_run("__FOOTNOTE_1__")
    document.add_paragraph(map_room[1])
    return _save(document, footnote_text="The archive keeps older map editions beside later revisions.")


def _merged_table_case() -> bytes:
    document = _document("Synthetic merged-table edge case")
    document.add_heading("Sparse route record", level=1)
    table = document.add_table(rows=3, cols=4)
    table.style = "Table Grid"
    _set_cell_text(table.cell(0, 0).merge(table.cell(0, 3)), "Route / Tuyến", bold=True)
    _set_cell_text(table.cell(1, 0), "Riverbank")
    _set_cell_text(table.cell(1, 1), "Bến sông")
    _set_cell_text(table.cell(1, 2), "")
    _set_cell_text(table.cell(1, 3), "8 min")
    _set_cell_text(table.cell(2, 0).merge(table.cell(2, 1)), "Nested record")
    nested = table.cell(2, 2).add_table(rows=1, cols=1)
    nested.style = "Table Grid"
    _set_cell_text(nested.cell(0, 0), "empty neighbor")
    _set_cell_text(table.cell(2, 3), "")
    document.add_paragraph("")
    return _save(document)


def _two_column_case(map_room: list[str]) -> bytes:
    document = _document("Synthetic two-column reading-order edge case")
    section = document.sections[0]
    columns = section._sectPr.xpath("./w:cols")
    columns[0].set(qn("w:num"), "2")
    columns[0].set(qn("w:space"), "720")
    document.add_heading("Two-column archive note", level=1)
    document.add_paragraph(map_room[0])
    document.add_paragraph(map_room[1])
    document.add_page_break()
    document.add_paragraph("This paragraph follows the page break and preserves document order.")
    return _save(document)


def _xml_bytes(root) -> bytes:
    return etree.tostring(root, encoding="UTF-8", xml_declaration=True, standalone=True)


def _add_footnote(parts: dict[str, bytes], text: str) -> None:
    namespaces = {"w": _WORD_NS}
    document_xml = etree.fromstring(parts["word/document.xml"])
    markers = document_xml.xpath(".//w:t[text()='__FOOTNOTE_1__']", namespaces=namespaces)
    if len(markers) != 1:
        raise ValueError("Expected exactly one footnote anchor")

    run = etree.Element(f"{{{_WORD_NS}}}r")
    run_properties = etree.SubElement(run, f"{{{_WORD_NS}}}rPr")
    style = etree.SubElement(run_properties, f"{{{_WORD_NS}}}rStyle")
    style.set(f"{{{_WORD_NS}}}val", "FootnoteReference")
    reference = etree.SubElement(run, f"{{{_WORD_NS}}}footnoteReference")
    reference.set(f"{{{_WORD_NS}}}id", "1")
    marker_run = markers[0].getparent()
    marker_run.getparent().replace(marker_run, run)
    parts["word/document.xml"] = _xml_bytes(document_xml)

    footnotes = etree.Element(f"{{{_WORD_NS}}}footnotes", nsmap={"w": _WORD_NS})
    for footnote_id, separator in ((-1, "separator"), (0, "continuationSeparator")):
        footnote = etree.SubElement(footnotes, f"{{{_WORD_NS}}}footnote")
        footnote.set(f"{{{_WORD_NS}}}type", separator)
        footnote.set(f"{{{_WORD_NS}}}id", str(footnote_id))
        paragraph = etree.SubElement(footnote, f"{{{_WORD_NS}}}p")
        separator_run = etree.SubElement(paragraph, f"{{{_WORD_NS}}}r")
        etree.SubElement(separator_run, f"{{{_WORD_NS}}}{separator}")

    footnote = etree.SubElement(footnotes, f"{{{_WORD_NS}}}footnote")
    footnote.set(f"{{{_WORD_NS}}}id", "1")
    paragraph = etree.SubElement(footnote, f"{{{_WORD_NS}}}p")
    paragraph_properties = etree.SubElement(paragraph, f"{{{_WORD_NS}}}pPr")
    paragraph_style = etree.SubElement(paragraph_properties, f"{{{_WORD_NS}}}pStyle")
    paragraph_style.set(f"{{{_WORD_NS}}}val", "FootnoteText")
    reference_run = etree.SubElement(paragraph, f"{{{_WORD_NS}}}r")
    reference_properties = etree.SubElement(reference_run, f"{{{_WORD_NS}}}rPr")
    reference_style = etree.SubElement(reference_properties, f"{{{_WORD_NS}}}rStyle")
    reference_style.set(f"{{{_WORD_NS}}}val", "FootnoteReference")
    etree.SubElement(reference_run, f"{{{_WORD_NS}}}footnoteRef")
    text_run = etree.SubElement(paragraph, f"{{{_WORD_NS}}}r")
    text_node = etree.SubElement(text_run, f"{{{_WORD_NS}}}t")
    text_node.text = text
    parts["word/footnotes.xml"] = _xml_bytes(footnotes)

    relationships = etree.fromstring(parts["word/_rels/document.xml.rels"])
    used_ids = [int(item.get("Id", "rId0")[3:]) for item in relationships if item.get("Id", "").startswith("rId")]
    relationship = etree.SubElement(relationships, f"{{{_REL_NS}}}Relationship")
    relationship.set("Id", f"rId{max(used_ids, default=0) + 1}")
    relationship.set("Type", _FOOTNOTE_REL)
    relationship.set("Target", "footnotes.xml")
    parts["word/_rels/document.xml.rels"] = _xml_bytes(relationships)

    content_types = etree.fromstring(parts["[Content_Types].xml"])
    override = etree.SubElement(content_types, f"{{{_CONTENT_NS}}}Override")
    override.set("PartName", "/word/footnotes.xml")
    override.set("ContentType", "application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml")
    parts["[Content_Types].xml"] = _xml_bytes(content_types)

    styles = etree.fromstring(parts["word/styles.xml"])
    footnote_text_style = etree.SubElement(styles, f"{{{_WORD_NS}}}style")
    footnote_text_style.set(f"{{{_WORD_NS}}}type", "paragraph")
    footnote_text_style.set(f"{{{_WORD_NS}}}styleId", "FootnoteText")
    footnote_text_name = etree.SubElement(footnote_text_style, f"{{{_WORD_NS}}}name")
    footnote_text_name.set(f"{{{_WORD_NS}}}val", "Footnote Text")
    footnote_reference_style = etree.SubElement(styles, f"{{{_WORD_NS}}}style")
    footnote_reference_style.set(f"{{{_WORD_NS}}}type", "character")
    footnote_reference_style.set(f"{{{_WORD_NS}}}styleId", "FootnoteReference")
    footnote_reference_name = etree.SubElement(footnote_reference_style, f"{{{_WORD_NS}}}name")
    footnote_reference_name.set(f"{{{_WORD_NS}}}val", "Footnote Reference")
    parts["word/styles.xml"] = _xml_bytes(styles)

    settings = etree.fromstring(parts["word/settings.xml"])
    footnote_properties = etree.Element(f"{{{_WORD_NS}}}footnotePr")
    number_format = etree.SubElement(footnote_properties, f"{{{_WORD_NS}}}numFmt")
    number_format.set(f"{{{_WORD_NS}}}val", "decimal")
    compatibility = settings.find(f"{{{_WORD_NS}}}compat")
    if compatibility is None:
        settings.append(footnote_properties)
    else:
        settings.insert(settings.index(compatibility), footnote_properties)
    parts["word/settings.xml"] = _xml_bytes(settings)


def _save(document: Document, *, footnote_text: str | None = None) -> bytes:
    source = BytesIO()
    document.save(source)
    with ZipFile(BytesIO(source.getvalue())) as original:
        parts = {name: original.read(name) for name in original.namelist()}

    if footnote_text is not None:
        _add_footnote(parts, footnote_text)

    output = BytesIO()
    with ZipFile(output, "w", compression=ZIP_STORED) as archive:
        for name, content in sorted(parts.items()):
            info = ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = ZIP_STORED
            info.create_system = 0
            info.external_attr = 0
            info.internal_attr = 0
            info.extra = b""
            info.comment = b""
            archive.writestr(info, content)
    return output.getvalue()


def generate(root: Path) -> dict[str, bytes]:
    garden = _source(root, "vietnamese-garden.md")
    river = _source(root, "vietnamese-river.md")
    repair_cafe = _source(root, "english-repair-cafe.md")
    map_room = _source(root, "english-map-room.md")
    return {
        "fixtures/docx/vi-styled-report.docx": _styled_report(garden),
        "fixtures/docx/en-numbered-checklist.docx": _numbered_checklist(repair_cafe),
        "fixtures/docx/vi-route-table.docx": _route_table(river),
        "fixtures/docx/en-footnotes.docx": _footnote_report(map_room),
        "fixtures/docx/edge-case-merged-table.docx": _merged_table_case(),
        "fixtures/docx/edge-case-two-column.docx": _two_column_case(map_room),
    }
