"""Original Apache-2.0 editor corpus, authored independently of conversion output."""

from __future__ import annotations

import base64
from copy import deepcopy
import hashlib
from io import BytesIO
import json
from pathlib import Path

from docx.enum.style import WD_STYLE_TYPE
from docx.shared import Pt, RGBColor
from PIL import Image
import typst

from ariad_fixture_gen.docx import _document, _save

PREFIX = "fixtures/editor/"
CASES = (
    "linked-footnote", "reused-footnote", "nested-footnote", "raw-html-block",
    "raw-tex-block", "inline-raw", "captioned-figure", "contained-figure",
    "table-colspan", "table-rowspan", "table-combined-spans", "table-caption",
    "table-footnotes", "nested-cell-blocks", "running-header", "running-footer",
    "inline-math", "display-math", "styled-nesting", "mixed-document",
)
RAW_HTML = '<aside data-original="yes">  Cafe\u0301\r\n<strong>Keep spacing</strong></aside>\n'
RAW_TEX = "\\newcommand{\\route}[1]{  #1 }% original\r\n\\route{Bến sông}\n"
INLINE_HTML = '<span data-note="x"> A\u0301 </span>'
INLINE_TEX = r"\kern  2pt % exact"
INLINE_MATH = r"x^2 + \alpha  = 5"
DISPLAY_MATH = "\\sum_{i=1}^{3} i\n  = 6"
PAIR_TITLE = "Sổ khảo sát ven sông"
PAIR_TEXT = "Bến sông có ba ghế gỗ. Nhóm đọc sách gặp nhau vào sáng thứ bảy."
PAIR_HEADER = "Ghi chép cộng đồng"
PAIR_FOOTER = "Lưu bản gốc để đối chiếu"
PAIR_ROWS = (("Điểm", "Số ghế"), ("Bến sông", "3"), ("Vườn đọc", "5"))
FONT = {"family": "Libertinus Serif", "fallback": ["Noto Serif"]}
# Frozen word bounds measured from the original authored PDF, never a converted output.
# Reproduce with pdftotext 26.10.0 -bbox-layout fixtures/editor/vi-styled-pair.pdf.
PAIR_PDF_SHA256 = "da0fab6a16d44f9fcb4dbef7dd0b741cc79654f9b165996bb2c6b2288ae131cc"
PAIR_BOXES = (
    (54.0, 49.518, 222.768, 70.038),
    (54.0, 71.264, 354.278, 83.804),
    (59.0, 96.702, 83.299, 109.242),
    (302.5, 96.702, 332.464, 109.242),
    (59.0, 113.940, 100.393, 126.480),
    (302.5, 113.940, 307.615, 126.480),
    (59.0, 131.178, 102.054, 143.718),
    (302.5, 131.178, 307.615, 143.718),
    (54.0, 27.966, 144.222, 40.506),
    (54.0, 801.604, 166.343, 814.144),
)


def _json(value: dict) -> bytes:
    return (json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2) + "\n").encode()


def _text(value: str) -> dict:
    return {"type": "text", "text": value}


def _paragraph(value: str, **extra) -> dict:
    return {"type": "paragraph", "content": [_text(value)], **extra}


def _cell(value: str, *, rowspan: int = 1, colspan: int = 1, header: bool = False) -> dict:
    return {"rowspan": rowspan, "colspan": colspan, "header": header, "blocks": [_paragraph(value)]}


def _table(case: str) -> dict:
    table = {
        "type": "table", "caption": None,
        "columns": [{"align": "left"}, {"align": "right"}, {"align": "center"}],
        "head": [[_cell("Place", header=True), _cell("Seats", header=True), _cell("Note", header=True)]],
        "body": [[_cell("River", header=True), _cell("3"), _cell("Shade")],
                 [_cell("Garden", header=True), _cell("5"), _cell("Books")]],
    }
    if case == "table-colspan":
        table["head"] = [[_cell("Survey totals", colspan=3, header=True)]]
    if case in ("table-rowspan", "table-combined-spans"):
        table["body"] = [[_cell("River", rowspan=2, header=True), _cell("3"), _cell("Shade")],
                         [_cell("5"), _cell("Books")]]
    if case == "table-combined-spans":
        table["head"] = [[_cell("Places and seats", colspan=2, header=True), _cell("Note", header=True)]]
    if case == "table-caption":
        table["caption"] = [_text("Survey: "), {"type": "emph", "content": [_text("morning seats")]}]
    if case == "table-footnotes":
        table["footnotes"] = [_text("Counts checked twice"), {"type": "footnote_ref", "id": "survey-note"}]
    if case == "nested-cell-blocks":
        table["body"][0][2]["blocks"].extend([
            {"type": "quote", "blocks": [_paragraph("Keep this cell quotation.")]},
            {"type": "list", "ordered": False, "start": None, "tight": False,
             "items": [{"checked": None, "blocks": [_paragraph("Check the shade.")]}]},
            {"type": "table", "caption": None, "columns": [{"align": "left"}], "head": [],
             "body": [[_cell("Inner reading")]]},
        ])
    return table


def _note(*, nested: bool = False) -> dict:
    blocks = [_paragraph("The survey records seats, not visitors.")]
    if nested:
        blocks.append({"type": "quote", "blocks": [
            _paragraph("Count only usable seats."),
            {"type": "list", "ordered": True, "start": 2, "tight": False,
             "items": [{"checked": None, "blocks": [_paragraph("Check each bench.")]}]},
        ]})
    return {"type": "footnote", "note_id": "survey-note", "blocks": blocks}


def _png() -> bytes:
    image = Image.new("RGB", (16, 12), "#e6f3ef")
    for x in range(3, 13):
        for y in range(4, 8):
            image.putpixel((x, y), (24, 92, 76))
    output = BytesIO()
    image.save(output, format="PNG", optimize=False)
    return output.getvalue()


def _feature_blocks(case: str, asset_id: str) -> list[dict]:
    if case.endswith("footnote"):
        refs = [{"type": "footnote_ref", "id": "survey-note"}]
        if case == "reused-footnote":
            refs.extend([_text(" and the same observation again"), {"type": "footnote_ref", "id": "survey-note"}])
        return [{"type": "paragraph", "content": [_text("Read the survey note"), *refs]},
                _note(nested=case == "nested-footnote")]
    if case in ("raw-html-block", "raw-tex-block"):
        return [{"type": "raw", "format": "html" if case == "raw-html-block" else "tex",
                 "text": RAW_HTML if case == "raw-html-block" else RAW_TEX}]
    if case == "inline-raw":
        return [{"type": "paragraph", "content": [
            _text("Before "), {"type": "raw", "format": "html", "text": INLINE_HTML},
            _text(" between "), {"type": "raw", "format": "tex", "text": INLINE_TEX}, _text(" after.")]}]
    if case in ("captioned-figure", "contained-figure"):
        figure = {"type": "figure", "asset": {"type": "asset", "id": asset_id},
                  "caption": [_text("Original bench diagram"), {"type": "emph", "content": [_text(" at dawn")]}]}
        return [figure if case == "captioned-figure" else {"type": "quote", "blocks": [figure]}]
    if case.startswith("table-") or case == "nested-cell-blocks":
        return [_table(case), *([_note()] if case == "table-footnotes" else [])]
    if case in ("running-header", "running-footer"):
        return [_paragraph("Furniture remains separate from this body paragraph.")]
    if case == "inline-math":
        return [{"type": "paragraph", "content": [_text("Inline formula "),
                {"type": "math", "tex": INLINE_MATH, "display": False}, _text(" ends here.")]}]
    if case == "display-math":
        return [{"type": "math", "tex": DISPLAY_MATH, "display": True}]
    if case == "styled-nesting":
        return [{"type": "quote", "style": "lead", "direct": {"indent_left_pt": 14.0}, "blocks": [
            {"type": "list", "ordered": False, "start": None, "tight": False, "items": [
                {"checked": False, "blocks": [{"type": "paragraph", "style": "body",
                 "direct": {"align": "justify", "size_pt": 12.0}, "content": [
                     {"type": "styled", "style": "accent", "direct": {"italic": False, "weight": 650},
                      "content": [_text("Giữ nguyên tiếng Việt: Nguyễn, đường, ươm."),
                          {"type": "styled", "direct": {"underline": True, "size_pt": 13.0},
                           "content": [_text(" Nhấn mạnh bên trong.")]}]}]}]},
            ]}]}]
    raise ValueError(f"Unknown editor feature: {case}")


def _identify(value, path: str) -> None:
    if isinstance(value, list):
        for index, item in enumerate(value):
            _identify(item, f"{path}/{index}")
    elif isinstance(value, dict):
        # Block nodes occur in body/furniture/blocks, never in inline content.
        if path.rsplit("/", 2)[-2:-1] in (["body"], ["furniture"], ["blocks"]):
            payload = json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
            value["id"] = "b" + hashlib.sha256(f"ariad-editor-fixture\0{path}\0{payload}".encode()).hexdigest()
            value.setdefault("direct", {})
        for key, item in list(value.items()):
            _identify(item, f"{path}/{key}")


def _walk_blocks(value):
    if isinstance(value, dict):
        if "id" in value and "direct" in value:
            yield value
        for item in value.values():
            yield from _walk_blocks(item)
    elif isinstance(value, list):
        for item in value:
            yield from _walk_blocks(item)


def _position(top: float, page: int = 1) -> dict:
    return {"page_no": page, "bbox": {"left": 54.0, "top": top, "right": 540.0, "bottom": top + 16.0}}


def authored_document(case: str) -> dict:
    """Construct IR from original authoring, never from reading a generated file."""
    asset = _png()
    asset_id = hashlib.sha256(asset).hexdigest()
    features = [item for item in CASES[:-1] if item not in ("linked-footnote", "nested-footnote", "table-footnotes")]
    body = [_paragraph(f"Editable neighbour before {case}."),
            {"type": "heading", "level": 1, "content": [_text(f"Editor case: {case}")]}]
    if case == "mixed-document":
        for feature in features:
            feature_blocks = _feature_blocks(feature, asset_id)
            if feature == "reused-footnote":
                feature_blocks[-1] = _note(nested=True)
            body.extend(feature_blocks)
        # A second note reference belongs to the table; there is one definition.
        table = _table("table-footnotes")
        body.append(table)
    else:
        body.extend(_feature_blocks(case, asset_id))
    body.append(_paragraph(f"Editable neighbour after {case}."))
    document = {
        "version": "ariad-ir/2", "meta": {"title": f"Editor preservation: {case}", "authors": ["AriadShift contributors"],
            "language": "vi" if case in ("styled-nesting", "mixed-document") else "en",
            "date": None, "subject": "Original editor preservation corpus", "keywords": [], "source_format": "ariad-ir+json"},
        "body": body, "assets": {}, "layout": None,
        "provenance": {"org.ariadshift.editor-corpus": {"source": "original-authoring", "case": case,
            "font_permission_evidence": {"Libertinus Serif": "No embedded font asset is supplied; family availability grants no embedding permission."}}},
    }
    if case in ("captioned-figure", "contained-figure", "mixed-document"):
        document["assets"][asset_id] = {"media_type": "image/png", "bytes": base64.b64encode(asset).decode()}
    if case in ("styled-nesting", "mixed-document"):
        document["styles"] = {"paragraph": {"body": {"font": FONT, "size_pt": 11.0, "line_height": 1.25},
            "lead": {"based_on": "body", "space_after_pt": 6.0, "color": "#185c4c"}},
            "character": {"accent": {"weight": 700, "italic": True, "color": "#185c4c"}},
            "table": {"survey": {"paragraph": {"size_pt": 10.0}, "character": {"weight": 400}, "background": "#e6f3ef"}},
            "defaults": {"paragraph": {"size_pt": 11.0}, "character": {"weight": 400}, "table": {"paragraph": {}, "character": {}}}}
    if case in ("running-header", "running-footer", "mixed-document"):
        document["furniture"] = [_paragraph(PAIR_HEADER), _paragraph(PAIR_FOOTER)]
    _identify(document["body"], "body")
    if "styles" in document:
        for block in _walk_blocks(document):
            if block["type"] == "table":
                block["style"] = "survey"
    if "furniture" in document:
        _identify(document["furniture"], "furniture")
        header, footer = (block["id"] for block in document["furniture"])
        document["sections"] = [{"page_size": {"width_pt": 595.0, "height_pt": 842.0},
            "margins": {"top_pt": 54.0, "right_pt": 54.0, "bottom_pt": 54.0, "left_pt": 54.0}, "columns": 1,
            **({"header": header} if case != "running-footer" else {}),
            **({"footer": footer} if case != "running-header" else {})}]
        if case == "mixed-document":
            second = deepcopy(document["sections"][0])
            second.update(start=document["body"][-1]["id"], columns=2)
            document["sections"].append(second)
    blocks = list(_walk_blocks(document))
    document["layout"] = {"pages": [{"page_no": 1, "width": 595.0, "height": 842.0}],
                          "blocks": {block["id"]: _position(60.0 + index * 4) for index, block in enumerate(blocks)}}
    document["source_map"] = {block["id"]: [_position(60.0 + index * 4)] for index, block in enumerate(blocks)}
    if case == "mixed-document":
        document["layout"]["pages"].append({"page_no": 2, "width": 595.0, "height": 842.0})
        document["source_map"][document["body"][0]["id"]].append(_position(70.0, page=2))
    return document


def _truth(case: str) -> dict:
    # Explicit author expectations: no reader, generated IR traversal or output read-back.
    mixed = case == "mixed-document"
    semantics = {"editable_before": f"Editable neighbour before {case}.",
                 "editable_after": f"Editable neighbour after {case}."}
    if case.endswith("footnote") or case == "table-footnotes" or mixed:
        semantics.update(note_id="survey-note", note_text="The survey records seats, not visitors.",
            note_reference_count=3 if mixed else 2 if case == "reused-footnote" else 1)
    for feature, values in {
        "raw-html-block": {"raw_html": RAW_HTML}, "raw-tex-block": {"raw_tex": RAW_TEX},
        "inline-raw": {"inline_html": INLINE_HTML, "inline_tex": INLINE_TEX},
        "inline-math": {"inline_math": INLINE_MATH}, "display-math": {"display_math": DISPLAY_MATH},
        "captioned-figure": {"figure_caption": "Original bench diagram at dawn"},
        "contained-figure": {"figure_caption": "Original bench diagram at dawn", "container": "quote"},
        "table-caption": {"table_caption": "Survey: morning seats"},
        "table-footnotes": {"table_note": "Counts checked twice"},
        "nested-cell-blocks": {"nested_cell_types": ["paragraph", "quote", "list", "table"]},
        "nested-footnote": {"note_nested_types": ["paragraph", "quote", "list"]},
        "running-header": {"header": PAIR_HEADER}, "running-footer": {"footer": PAIR_FOOTER},
    }.items():
        if case == feature or mixed:
            semantics.update(values)
    if case.startswith("table-") or case == "nested-cell-blocks" or mixed:
        semantics["spans"] = {"max_colspan": 3 if mixed or case == "table-colspan" else 2 if case == "table-combined-spans" else 1,
            "max_rowspan": 2 if mixed or case in ("table-rowspan", "table-combined-spans") else 1, "row_header": True}
    styles = {"paragraph": {"family": "Libertinus Serif", "base_size_pt": 11.0, "direct_size_pt": 12.0,
            "line_height": 1.25, "align": "justify"},
            "character": {"named_weight": 700, "direct_weight": 650, "italic": False, "color": "#185c4c",
                "nested_size_pt": 13.0, "nested_underline": True},
            "font_embedding": {"supplied_font_assets": 0, "restricted": "do-not-embed", "unknown": "do-not-embed"}}
    return {"schema": "ariad-editor-truth/1", "case": case, "origin": "independent-original-specification",
        "features": list(CASES[:-1]) if mixed else [case], "semantics": semantics,
        "ir": authored_document(case),
        "styles": styles if mixed or case == "styled-nesting" else {"applicable": False},
        "layout": {"reference_kind": "authored-ir-geometry", "page_width_pt": 595.0, "page_height_pt": 842.0,
            "first_bbox": {"left": 54.0, "top": 60.0, "right": 540.0, "bottom": 76.0},
            "section_count": 2 if case == "mixed-document" else 1 if case.startswith("running-") else 0,
            "first_block_source_fragments": 2 if case == "mixed-document" else 1},
        "preservation": ["adjacent-text-edit", "stable-id-reorder", "opaque-byte-equality", "explicit-delete-removes-payload", "ambiguous-orphan-is-reported"],
        "future_gates": ["tiptap-native-roundtrip", "textkit-native-roundtrip", "edit-export-fidelity"]}


def _paired_ir() -> dict:
    """Keep authored semantics/styles and frozen geometry from the original PDF."""
    document = {"version": "ariad-ir/2", "meta": {"title": PAIR_TITLE, "language": "vi"},
        "body": [
            {"type": "heading", "level": 1, "direct": {"font": FONT, "size_pt": 18.0, "color": "#185c4c"},
             "content": [{"type": "styled", "direct": {"font": FONT, "size_pt": 18.0, "weight": 700, "color": "#185c4c"},
                          "content": [_text(PAIR_TITLE)]}]},
            _paragraph(PAIR_TEXT, direct={"font": FONT, "size_pt": 11.0}),
            {"type": "table", "caption": None, "columns": [{"align": "left"}, {"align": "left"}],
             "head": [], "body": [[_cell(value) for value in row] for row in PAIR_ROWS]},
        ], "furniture": [_paragraph(PAIR_HEADER), _paragraph(PAIR_FOOTER)], "assets": {},
        "styles": {"paragraph": {}, "character": {}, "table": {},
            "defaults": {"paragraph": {"font": FONT, "size_pt": 11.0}, "character": {"weight": 400},
                         "table": {"paragraph": {}, "character": {}}}},
        "layout": {"pages": [{"page_no": 1, "width": 595.0, "height": 842.0}]},
        "provenance": {"org.ariadshift.editor-corpus": {"source": "original-authoring", "case": "vi-styled-pair"}},
    }
    _identify(document["body"], "body")
    _identify(document["furniture"], "furniture")
    table = document["body"][2]
    cells = [block for row in table["body"] for cell in row for block in cell["blocks"]]
    leaves = [*document["body"][:2], *cells, *document["furniture"]]
    boxes = {block["id"]: {"page_no": 1, "bbox": dict(zip(
        ("left", "top", "right", "bottom"), coordinates, strict=True))}
        for block, coordinates in zip(leaves, PAIR_BOXES, strict=True)}
    # A table's text bounds are the union of its independently positioned cells.
    cell_boxes = [boxes[block["id"]]["bbox"] for block in cells]
    boxes[table["id"]] = {"page_no": 1, "bbox": {
        "left": min(box["left"] for box in cell_boxes), "top": min(box["top"] for box in cell_boxes),
        "right": max(box["right"] for box in cell_boxes), "bottom": max(box["bottom"] for box in cell_boxes)}}
    document["layout"]["blocks"] = boxes
    document["sections"] = [{"page_size": {"width_pt": 595.0, "height_pt": 842.0},
        "margins": {"top_pt": 54.0, "right_pt": 54.0, "bottom_pt": 54.0, "left_pt": 54.0},
        "columns": 1, "header": document["furniture"][0]["id"], "footer": document["furniture"][1]["id"]}]
    return document


def _pair_truth() -> dict:
    return {"schema": "ariad-editor-truth/1", "case": "vi-styled-pair", "origin": "independent-original-specification",
        "semantics": {"title": PAIR_TITLE, "paragraph": PAIR_TEXT, "header": PAIR_HEADER, "footer": PAIR_FOOTER,
            "table": [list(row) for row in PAIR_ROWS], "headings": [PAIR_TITLE]},
        "styles": {"font_family": "Libertinus Serif", "body_size_pt": 11.0, "body_weight": 400,
            "heading_size_pt": 18.0, "heading_weight": 700, "heading_color": "#185c4c"},
        "layout": {"reference_kind": "authored-pdf-word-bounds", "page_width_pt": 595.0,
            "page_height_pt": 842.0, "margins_pt": 54.0, "columns": 1,
            "source_pdf_sha256": PAIR_PDF_SHA256,
            "measurement": "pdftotext 26.10.0 -bbox-layout; top-left points; table is cell-text union"},
        "ir": _paired_ir(),
        "canonical": {"text": "\n".join([PAIR_TITLE, PAIR_TEXT, *(value for row in PAIR_ROWS for value in row)]),
            "headings": [{"level": 1, "text": PAIR_TITLE, "children": []}],
            "tables": [{"caption": "", "rows": [[{"text": value, "rowspan": 1, "colspan": 1, "is_header": False}
                for value in row] for row in PAIR_ROWS]}], "lists": [], "image_count": 0, "link_count": 0},
        "tolerances": {"size_pt": 0.5, "weight": 0, "rgb_channel": 2},
        "routes": ["pdf->docx", "docx->pdf"]}


def _paired_docx() -> bytes:
    document = _document(PAIR_TITLE)
    normal = document.styles["Normal"]
    normal.font.name = "Libertinus Serif"
    normal.font.size = Pt(11)
    normal.font.bold = False
    heading = document.styles.add_style("Survey Heading", WD_STYLE_TYPE.PARAGRAPH)
    heading.base_style = document.styles["Heading 1"]
    heading.font.name = "Libertinus Serif"
    heading.font.size = Pt(18)
    heading.font.bold = True
    heading.font.color.rgb = RGBColor.from_string("185C4C")
    section = document.sections[0]
    section.page_width, section.page_height = Pt(595), Pt(842)
    section.top_margin = section.bottom_margin = section.left_margin = section.right_margin = Pt(54)
    section.header.paragraphs[0].text = PAIR_HEADER
    section.footer.paragraphs[0].text = PAIR_FOOTER
    document.add_paragraph(PAIR_TITLE, style="Survey Heading")
    document.add_paragraph(PAIR_TEXT)
    table = document.add_table(rows=3, cols=2)
    for row_index, row in enumerate(PAIR_ROWS):
        for col_index, value in enumerate(row):
            table.cell(row_index, col_index).text = value
    return _save(document)


def _paired_typst() -> bytes:
    quote = lambda value: json.dumps(value, ensure_ascii=False)
    source = [
        '#set document(author: "AriadShift contributors", date: none)',
        f'#set page(width: 595pt, height: 842pt, margin: 54pt, header: text({quote(PAIR_HEADER)}), footer: text({quote(PAIR_FOOTER)}))',
        '#set text(font: "Libertinus Serif", size: 11pt, weight: 400, lang: "vi")',
        f'#heading(level: 1)[#text(size: 18pt, weight: 700, fill: rgb("#185c4c"), {quote(PAIR_TITLE)})]',
        f'#text({quote(PAIR_TEXT)})', '#parbreak()',
        '#table(columns: (1fr, 1fr), inset: 5pt,',
        *[", ".join(f'[{"#text(" + quote(value) + ")"}]' for value in row) + "," for row in PAIR_ROWS], ")",
    ]
    return ("\n".join(source) + "\n").encode()


def generate(root: Path) -> dict[str, bytes]:
    """Generate only new editor paths, with fixed PDF and ZIP timestamps."""
    outputs = {}
    for case in CASES:
        outputs[PREFIX + case + ".ir.json"] = _json(authored_document(case))
        outputs[PREFIX + case + ".truth.json"] = _json(_truth(case))
    outputs[PREFIX + "bench-diagram.png"] = _png()
    outputs[PREFIX + "vi-field-notes.md"] = (
        "# Ghi chép bên sông\n\nBuổi sáng có gió nhẹ.[^survey]\n\n"
        "> Nhóm đọc sách kiểm tra ghế.\n>\n> - Đếm ghế gỗ\n>   - Ghi lại bóng mát\n\n"
        "Công thức $x^2 + 1 = 5$.\n\n[^survey]: Ba ghế còn dùng tốt.\n").encode()
    outputs[PREFIX + "vi-field-notes.html"] = (
        '<!doctype html><html lang="vi"><head><meta charset="utf-8"><title>Bảng ghế bên sông</title></head>'
        '<body><h1 style="font-size:18pt;color:#185c4c">Bảng ghế bên sông</h1>'
        '<p style="font-size:11pt">Có bóng mát vào buổi sáng.</p><table><caption>Ghế và bóng mát</caption>'
        '<thead><tr><th colspan="2">Khảo sát</th></tr></thead>'
        '<tbody><tr><th rowspan="2" scope="row">Bến sông</th><td>3 ghế</td></tr>'
        '<tr><td>Bóng mát</td></tr></tbody></table></body></html>\n').encode()
    for extension, title, paragraph in (
        ("md", "Ghi chép bên sông", "Buổi sáng có gió nhẹ."),
        ("html", "Bảng ghế bên sông", "Có bóng mát vào buổi sáng."),
    ):
        outputs[PREFIX + f"vi-field-notes-{extension}.truth.json"] = _json({
            "schema": "ariad-editor-truth/1", "case": f"vi-field-notes-{extension}",
            "origin": "independent-original-specification", "semantics": {"title": title, "paragraph": paragraph},
            "styles": {"heading_size_pt": 18.0, "heading_color": "#185c4c"} if extension == "html" else {"applicable": False},
            "layout": {"reference_kind": "reflow-no-box-reference"}})
    source = _paired_typst()
    outputs[PREFIX + "vi-styled-pair.typ"] = source
    outputs[PREFIX + "vi-styled-pair.docx"] = _paired_docx()
    outputs[PREFIX + "vi-styled-pair.pdf"] = typst.compile(source, format="pdf", timestamp=1_700_000_000)
    for extension in ("docx", "pdf"):
        outputs[PREFIX + f"vi-styled-pair-{extension}.truth.json"] = _json(_pair_truth())
    return outputs
