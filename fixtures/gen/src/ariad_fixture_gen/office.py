"""Generate reproducible PPTX and XLSX fixtures."""

from __future__ import annotations

from datetime import datetime
import re
from io import BytesIO
from pathlib import Path
from zipfile import ZIP_STORED, ZipFile, ZipInfo

from openpyxl import Workbook
from openpyxl.styles import Font
from pptx import Presentation
from pptx.util import Inches, Pt

_FIXED_TIME = datetime(2000, 1, 1)
_AUTHOR = "AriadShift contributors"


def _source(root: Path, filename: str) -> list[str]:
    path = root / "fixtures" / "gen" / "src" / "ariad_fixture_gen" / "text" / filename
    return [part.strip() for part in path.read_text(encoding="utf-8").split("\n\n") if part.strip()]


_MODIFIED = re.compile(rb"(<dcterms:modified[^>]*>)[^<]*(</dcterms:modified>)")


def _normalized_zip(data: bytes) -> bytes:
    """Rewrites an OOXML package with sorted, stored entries and fixed timestamps."""
    with ZipFile(BytesIO(data)) as original:
        parts = {name: original.read(name) for name in original.namelist()}
    # openpyxl stamps the save time into the core properties.
    parts["docProps/core.xml"] = _MODIFIED.sub(
        rb"\g<1>2000-01-01T00:00:00Z\g<2>", parts["docProps/core.xml"]
    )
    output = BytesIO()
    with ZipFile(output, "w", compression=ZIP_STORED) as archive:
        for name, content in sorted(parts.items()):
            info = ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = ZIP_STORED
            info.create_system = 0
            info.external_attr = 0
            archive.writestr(info, content)
    return output.getvalue()


def _slides(root: Path) -> bytes:
    paragraphs = _source(root, "vietnamese-garden.md")
    presentation = Presentation()
    properties = presentation.core_properties
    properties.author = _AUTHOR
    properties.last_modified_by = _AUTHOR
    properties.title = "Khu vườn đọc sách"
    properties.created = _FIXED_TIME
    properties.modified = _FIXED_TIME
    properties.revision = 1

    title_slide = presentation.slides.add_slide(presentation.slide_layouts[0])
    title_slide.shapes.title.text = "Khu vườn đọc sách"
    title_slide.placeholders[1].text = "Một khoảng sân nhỏ sau thư viện phường"

    bullets = presentation.slides.add_slide(presentation.slide_layouts[1])
    bullets.shapes.title.text = "Lịch chăm vườn"
    body = bullets.placeholders[1].text_frame
    body.text = "Thứ bảy: tưới cây và đổi sách"
    for line in ("Ngày nắng: học bài dưới giàn hoa giấy", "Ngày mưa: chuyển vào mái hiên"):
        body.add_paragraph().text = line

    table_slide = presentation.slides.add_slide(presentation.slide_layouts[5])
    table_slide.shapes.title.text = "Sách được đổi trong tháng"
    rows = [("Loại sách", "Số quyển"), ("Truyện tranh", "24"), ("Sách nấu ăn", "11")]
    shape = table_slide.shapes.add_table(
        len(rows), 2, Inches(1), Inches(2), Inches(6), Inches(1.5)
    )
    for row_index, row in enumerate(rows):
        for column_index, value in enumerate(row):
            cell = shape.table.cell(row_index, column_index)
            cell.text = value
            cell.text_frame.paragraphs[0].runs[0].font.size = Pt(18)

    prose = presentation.slides.add_slide(presentation.slide_layouts[1])
    prose.shapes.title.text = "Ghi chép"
    prose.placeholders[1].text_frame.text = paragraphs[1]

    output = BytesIO()
    presentation.save(output)
    return _normalized_zip(output.getvalue())


def _workbook() -> bytes:
    workbook = Workbook()
    workbook.properties.creator = _AUTHOR
    workbook.properties.lastModifiedBy = _AUTHOR
    workbook.properties.title = "Repair café log"
    workbook.properties.created = _FIXED_TIME
    workbook.properties.modified = _FIXED_TIME

    log = workbook.active
    log.title = "Repairs"
    rows = [
        ("Item", "Fault", "Minutes", "Fixed"),
        ("Desk lamp", "Loose switch", 15, "yes"),
        ("Toaster", "Broken lever", 40, "yes"),
        ("Radio", "No sound", 55, "no"),
        ("Bicycle light", "Corroded battery contacts", 10, "yes"),
    ]
    for row in rows:
        log.append(row)
    for cell in log[1]:
        cell.font = Font(bold=True)

    volunteers = workbook.create_sheet("Tình nguyện viên")
    volunteers.append(("Tên", "Ca trực"))
    volunteers.append(("Mai", "Sáng thứ bảy"))
    volunteers.append(("Hùng", "Chiều chủ nhật"))
    for cell in volunteers[1]:
        cell.font = Font(bold=True)

    output = BytesIO()
    workbook.save(output)
    return _normalized_zip(output.getvalue())


def generate(root: Path) -> dict[str, bytes]:
    return {
        "fixtures/pptx/vi-garden-slides.pptx": _slides(root),
        "fixtures/xlsx/en-repair-log.xlsx": _workbook(),
    }
