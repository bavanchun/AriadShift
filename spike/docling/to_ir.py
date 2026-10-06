"""DoclingDocument to AriadShift IR v0 converter.

Maps a DoclingDocument to the draft ariad-ir/0 schema.
Records which Docling fields/structures cannot be represented in IR v0.
"""

from __future__ import annotations

import base64
import hashlib
import io
import json
import sys
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

from docling_core.types.doc import (
    DoclingDocument,
    DocItem,
    DocItemLabel,
    FloatingItem,
    GroupItem,
    PictureItem,
    SectionHeaderItem,
    TableItem,
    TextItem,
    TitleItem,
    CodeItem,
    FormulaItem,
    ListItem,
)


def docling_to_ir(
    doc: DoclingDocument,
    source_format: Optional[str] = None,
) -> Tuple[Dict[str, Any], List[Dict[str, Any]]]:
    """Converts a DoclingDocument into an ariad-ir/0 dict and a list of lost fields."""
    lost_fields: List[Dict[str, Any]] = []
    assets: Dict[str, Dict[str, Any]] = {}

    # Extract metadata
    # Note: Format in ariad-core only allows markdown, html, docx, ariad_ir_json, pandoc_json.
    # Until Format::Pdf is added in Phase 3, source_format for PDF/image must be None in IR JSON.
    valid_formats = {"markdown", "html", "docx", "ariad_ir_json", "pandoc_json"}
    ir_source_format = source_format if source_format in valid_formats else None

    meta = {
        "title": getattr(doc, "name", None),
        "authors": [],
        "language": None,
        "date": None,
        "subject": None,
        "keywords": [],
        "source_format": ir_source_format,
    }

    body: List[Dict[str, Any]] = []
    current_list_items: Optional[List[Dict[str, Any]]] = None
    current_list_ordered: bool = False

    def flush_list():
        nonlocal current_list_items, current_list_ordered
        if current_list_items is not None and len(current_list_items) > 0:
            body.append({
                "type": "list",
                "ordered": current_list_ordered,
                "start": None,
                "tight": True,
                "items": current_list_items,
            })
        current_list_items = None
        current_list_ordered = False

    for item, level in doc.iterate_items():
        # Check for furniture (page headers, footers)
        item_label = getattr(item, "label", None)
        item_layer = getattr(item, "content_layer", None)
        if item_label in (DocItemLabel.PAGE_HEADER, DocItemLabel.PAGE_FOOTER) or str(item_layer) == "furniture":
            lost_fields.append({
                "field": "furniture",
                "item_type": type(item).__name__,
                "label": str(item_label),
                "text": getattr(item, "text", "")[:50],
                "reason": "IR v0 has no furniture field (dropped)",
            })
            continue

        # Check for page provenance
        if hasattr(item, "prov") and item.prov:
            for p in item.prov:
                lost_fields.append({
                    "field": "provenance_bbox",
                    "item_type": type(item).__name__,
                    "page_no": p.page_no,
                    "bbox": str(getattr(p, "bbox", None)),
                    "reason": "IR v0 blocks have no provenance/bounding-box field",
                })
                break  # Record once per item

        if isinstance(item, ListItem):
            is_ordered = getattr(item, "enumerated", False)
            if current_list_items is None or current_list_ordered != is_ordered:
                flush_list()
                current_list_items = []
                current_list_ordered = bool(is_ordered)

            inlines = text_to_inlines(item)
            current_list_items.append({
                "checked": None,
                "blocks": [{
                    "type": "paragraph",
                    "content": inlines,
                }],
            })
            continue

        # Not a list item, flush any pending list
        flush_list()

        if isinstance(item, TitleItem):
            body.append({
                "type": "heading",
                "level": 1,
                "content": text_to_inlines(item),
            })
        elif isinstance(item, SectionHeaderItem):
            level_val = getattr(item, "level", 2) or 2
            body.append({
                "type": "heading",
                "level": min(max(int(level_val), 1), 6),
                "content": text_to_inlines(item),
            })
        elif isinstance(item, CodeItem):
            body.append({
                "type": "code",
                "lang": None,
                "text": getattr(item, "text", ""),
            })
        elif isinstance(item, FormulaItem):
            tex = getattr(item, "text", "")
            if not tex:
                lost_fields.append({
                    "field": "formula_tex",
                    "item_type": "FormulaItem",
                    "reason": "Formula has empty LaTeX text without formula enrichment",
                })
            body.append({
                "type": "math",
                "tex": tex,
                "display": True,
            })
        elif isinstance(item, TableItem):
            table_block, table_losses = table_to_ir_block(doc, item)
            body.append(table_block)
            lost_fields.extend(table_losses)
        elif isinstance(item, PictureItem):
            figure_block, pic_assets, pic_losses = picture_to_ir_block(doc, item)
            body.append(figure_block)
            assets.update(pic_assets)
            lost_fields.extend(pic_losses)
        elif isinstance(item, TextItem):
            body.append({
                "type": "paragraph",
                "content": text_to_inlines(item),
            })

    # Flush any trailing list
    flush_list()

    # Check for furniture stored in doc.furniture
    if getattr(doc, "furniture", None):
        for item, _ in doc.iterate_items(root=doc.furniture):
            item_label = getattr(item, "label", None)
            lost_fields.append({
                "field": "furniture",
                "item_type": type(item).__name__,
                "label": str(item_label),
                "text": getattr(item, "text", "")[:50],
                "reason": "Docling furniture layer item dropped (IR v0 has no furniture field)",
            })


    ir_doc = {
        "version": "ariad-ir/0",
        "meta": meta,
        "body": body,
        "assets": assets,
        "layout": None,
        "provenance": None,
    }

    return ir_doc, lost_fields


def text_to_inlines(item: Any) -> List[Dict[str, Any]]:
    """Converts a TextItem or similar into a list of IR Inlines."""
    text = getattr(item, "text", "")
    if not text:
        return []

    inline: Dict[str, Any] = {"type": "text", "text": text}

    # Formatting
    formatting = getattr(item, "formatting", None)
    if formatting:
        if getattr(formatting, "bold", False):
            inline = {"type": "strong", "content": [inline]}
        if getattr(formatting, "italic", False):
            inline = {"type": "emph", "content": [inline]}
        if getattr(formatting, "strike", False):
            inline = {"type": "strikeout", "content": [inline]}
        if getattr(formatting, "superscript", False):
            inline = {"type": "superscript", "content": [inline]}
        if getattr(formatting, "subscript", False):
            inline = {"type": "subscript", "content": [inline]}

    # Hyperlink
    hyperlink = getattr(item, "hyperlink", None)
    if hyperlink:
        inline = {
            "type": "link",
            "url": str(hyperlink),
            "title": None,
            "content": [inline],
        }

    return [inline]


def table_to_ir_block(
    doc: DoclingDocument,
    table: TableItem,
) -> Tuple[Dict[str, Any], List[Dict[str, Any]]]:
    """Converts a TableItem to an IR Table block, tracking lost features."""
    losses: List[Dict[str, Any]] = []

    # Table caption
    caption_text = table.caption_text(doc)
    caption = [{"type": "text", "text": caption_text}] if caption_text else None

    # Table footnotes
    if hasattr(table, "footnotes") and table.footnotes:
        losses.append({
            "field": "table_footnotes",
            "count": len(table.footnotes),
            "reason": "IR v0 Table has no footnotes field (dropped)",
        })

    num_rows = table.data.num_rows
    num_cols = table.data.num_cols

    columns = [{"align": "default"} for _ in range(num_cols)]

    # Distinguish header vs body cells
    # Docling cells have start_row_offset_idx, start_col_offset_idx, row_span, col_span, column_header, row_header
    grid = table.data.grid

    head_rows: List[List[Dict[str, Any]]] = []
    body_rows: List[List[Dict[str, Any]]] = []

    # Build rows
    cells_by_row: Dict[int, List[Any]] = {}
    for cell in table.data.table_cells:
        r = cell.start_row_offset_idx
        cells_by_row.setdefault(r, []).append(cell)
        if getattr(cell, "row_header", False):
            losses.append({
                "field": "row_header_cell",
                "row": r,
                "col": cell.start_col_offset_idx,
                "reason": "IR v0 Table distinguishes head vs body rows, but cannot mark individual row_header cells",
            })

    for r in range(num_rows):
        cells = sorted(cells_by_row.get(r, []), key=lambda c: c.start_col_offset_idx)
        ir_cells = []
        is_head_row = False
        for cell in cells:
            if getattr(cell, "column_header", False):
                is_head_row = True
            cell_text = getattr(cell, "text", "")
            ir_cells.append({
                "rowspan": max(1, getattr(cell, "row_span", 1)),
                "colspan": max(1, getattr(cell, "col_span", 1)),
                "blocks": [{
                    "type": "paragraph",
                    "content": [{"type": "text", "text": cell_text}],
                }],
            })

        if is_head_row and len(body_rows) == 0:
            head_rows.append(ir_cells)
        else:
            body_rows.append(ir_cells)

    table_block = {
        "type": "table",
        "caption": caption,
        "columns": columns,
        "head": head_rows,
        "body": body_rows,
    }

    return table_block, losses


def picture_to_ir_block(
    doc: DoclingDocument,
    picture: PictureItem,
) -> Tuple[Dict[str, Any], Dict[str, Any], List[Dict[str, Any]]]:
    """Converts a PictureItem to an IR Figure block with asset bytes."""
    losses: List[Dict[str, Any]] = []
    assets: Dict[str, Any] = {}

    caption_text = picture.caption_text(doc)
    caption = [{"type": "text", "text": caption_text}] if caption_text else []

    img = picture.get_image(doc)
    if img is not None:
        buf = io.BytesIO()
        img.save(buf, format="PNG")
        png_bytes = buf.getvalue()
        sha256 = hashlib.sha256(png_bytes).hexdigest()
        b64 = base64.b64encode(png_bytes).decode("ascii")

        assets[sha256] = {
            "media_type": "image/png",
            "bytes": b64,
        }
        asset_ref = {"type": "asset", "id": sha256}
    else:
        losses.append({
            "field": "picture_image",
            "reason": "Picture has no image bytes (generate_picture_images was false or unrendered)",
        })
        # Empty placeholder asset
        placeholder = b""
        sha256 = hashlib.sha256(placeholder).hexdigest()
        assets[sha256] = {
            "media_type": "image/png",
            "bytes": "",
        }
        asset_ref = {"type": "asset", "id": sha256}

    figure_block = {
        "type": "figure",
        "asset": asset_ref,
        "caption": caption,
    }

    return figure_block, assets, losses


if __name__ == "__main__":
    if len(sys.argv) < 3:
        print("Usage: python to_ir.py <input.docling.json> <output.ir.json>")
        sys.exit(1)

    in_path = Path(sys.argv[1])
    out_path = Path(sys.argv[2])

    with open(in_path, "r", encoding="utf-8") as f:
        data = json.load(f)

    doc = DoclingDocument.model_validate(data)
    ir, lost = docling_to_ir(doc)

    out_path.parent.mkdir(parents=True, exist_ok=True)
    with open(out_path, "w", encoding="utf-8") as f:
        json.dump(ir, f, indent=2, ensure_ascii=False)

    if len(sys.argv) > 3:
        losses_path = Path(sys.argv[3])
        losses_path.parent.mkdir(parents=True, exist_ok=True)
        with open(losses_path, "w", encoding="utf-8") as f:
            json.dump(lost, f, indent=2, ensure_ascii=False)

    print(f"Mapped {in_path} -> {out_path} ({len(ir['body'])} blocks, {len(ir['assets'])} assets)")
    print(f"Recorded {len(lost)} dropped/unmapped field instances")
