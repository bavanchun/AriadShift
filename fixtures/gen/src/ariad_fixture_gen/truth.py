"""Generate authored truth companions in canonical form for DOCX, HTML, and EPUB fixtures.

Truth companions are authored directly from the exact data structures and texts
rendered by each fixture generator, without relying on reader outputs or golden snapshots.
"""

from __future__ import annotations

from dataclasses import dataclass
import json
from pathlib import Path
from typing import Any
import unicodedata


def clean_text(text: str) -> str:
    """Normalize string to Unicode NFC form and collapse contiguous whitespace."""
    return " ".join(unicodedata.normalize("NFC", text).split())


def _build_heading_tree(flat_headings: list[tuple[int, str]]) -> list[dict[str, Any]]:
    """Assemble a sequential list of (level, text) into a nested tree of HeadingNodes."""
    root_nodes: list[dict[str, Any]] = []
    stack: list[tuple[int, dict[str, Any]]] = []

    for level, text in flat_headings:
        node: dict[str, Any] = {"level": level, "text": clean_text(text), "children": []}
        while stack and stack[-1][0] >= level:
            stack.pop()

        if not stack:
            root_nodes.append(node)
        else:
            stack[-1][1]["children"].append(node)

        stack.append((level, node))

    return root_nodes


@dataclass
class HeadingBlock:
    level: int
    text: str


@dataclass
class ParagraphBlock:
    text: str
    quote: bool = False
    link_url: str | None = None
    link_text: str | None = None
    footnote_anchor: str | None = None
    footnote_ref: str | None = None


@dataclass
class ListBlock:
    items: list[str]
    ordered: bool = False
    style: str | None = None


@dataclass
class Cell:
    text: str
    rowspan: int = 1
    colspan: int = 1
    is_header: bool = False
    nested_table: TableBlock | None = None


@dataclass
class TableBlock:
    rows: list[list[Cell]]
    caption: str = ""
    table_style: str | None = None
    alignment: str | None = None


@dataclass
class FigureBlock:
    alt: str
    caption: str = ""
    asset_id: str | None = None
    asset_bytes: bytes | None = None
    media_type: str = "image/png"
    href: str | None = None


@dataclass
class FootnoteBlock:
    text: str
    footnote_id: str | None = None


@dataclass
class PageBreakBlock:
    pass


@dataclass
class ChapterBreakBlock:
    file_name: str
    nav_title: str


@dataclass
class UnscoredBlock:
    reason: str


def block_to_dict(block: Any) -> dict[str, Any]:
    """Serialize a Block instance to a dictionary."""
    if isinstance(block, HeadingBlock):
        return {"type": "heading", "level": block.level, "text": clean_text(block.text)}
    elif isinstance(block, ParagraphBlock):
        d: dict[str, Any] = {"type": "paragraph", "text": clean_text(block.text)}
        if block.quote:
            d["quote"] = True
        if block.link_url:
            d["link_url"] = block.link_url
            d["link_text"] = block.link_text or block.text
        return d
    elif isinstance(block, ListBlock):
        return {
            "type": "list",
            "ordered": block.ordered,
            "items": [clean_text(it) for it in block.items if clean_text(it)],
        }
    elif isinstance(block, TableBlock):
        return {
            "type": "table",
            "caption": clean_text(block.caption),
            "rows": [
                [
                    {
                        "text": clean_text(c.text),
                        "rowspan": max(1, c.rowspan),
                        "colspan": max(1, c.colspan),
                        "is_header": bool(c.is_header),
                    }
                    for c in r
                ]
                for r in block.rows
            ],
        }
    elif isinstance(block, FigureBlock):
        d = {
            "type": "figure",
            "alt": clean_text(block.alt),
            "caption": clean_text(block.caption or block.alt),
            "media_type": block.media_type,
        }
        if block.asset_id:
            d["asset_id"] = block.asset_id
        return d
    elif isinstance(block, FootnoteBlock):
        return {"type": "footnote", "text": clean_text(block.text)}
    elif isinstance(block, PageBreakBlock):
        return {"type": "page_break"}
    elif isinstance(block, ChapterBreakBlock):
        return {"type": "chapter_break", "file_name": block.file_name, "nav_title": block.nav_title}
    elif isinstance(block, UnscoredBlock):
        return {"type": "unscored", "reason": block.reason}
    raise TypeError(f"Unknown block type: {type(block)}")


def blocks_to_canonical(
    doc_id: str,
    blocks: list[Any],
    *,
    ir_doc: dict[str, Any] | None = None,
) -> dict[str, Any]:
    """Convert an ordered block list to a canonical truth dictionary."""
    for b in blocks:
        if isinstance(b, UnscoredBlock):
            return {
                "version": "ariad-truth/0",
                "id": doc_id,
                "unscored": True,
                "unscored_reason": b.reason,
            }

    text_pieces: list[str] = []
    flat_headings: list[tuple[int, str]] = []
    tables: list[dict[str, Any]] = []
    lists: list[dict[str, Any]] = []
    deferred_footnotes: list[str] = []
    image_count = 0
    link_count = 0

    for b in blocks:
        if isinstance(b, HeadingBlock):
            c_text = clean_text(b.text)
            if c_text:
                flat_headings.append((b.level, c_text))
                text_pieces.append(c_text)
        elif isinstance(b, ParagraphBlock):
            c_text = clean_text(b.text)
            if c_text:
                text_pieces.append(c_text)
            if b.link_url:
                link_count += 1
        elif isinstance(b, ListBlock):
            items = [clean_text(it) for it in b.items if clean_text(it)]
            if items:
                lists.append({"ordered": b.ordered, "items": items})
                text_pieces.extend(items)
        elif isinstance(b, TableBlock):
            c_caption = clean_text(b.caption)
            if c_caption:
                text_pieces.append(c_caption)
            grid_rows: list[list[dict[str, Any]]] = []
            for row in b.rows:
                grid_row: list[dict[str, Any]] = []
                for cell in row:
                    c_cell = clean_text(cell.text)
                    if c_cell:
                        text_pieces.append(c_cell)
                    grid_row.append({
                        "text": c_cell,
                        "rowspan": max(1, cell.rowspan),
                        "colspan": max(1, cell.colspan),
                        "is_header": bool(cell.is_header),
                    })
                if grid_row:
                    grid_rows.append(grid_row)
            tables.append({"caption": c_caption, "rows": grid_rows})
        elif isinstance(b, FigureBlock):
            image_count += 1
            c_cap = clean_text(b.caption or b.alt)
            if c_cap:
                text_pieces.append(c_cap)
        elif isinstance(b, FootnoteBlock):
            c_text = clean_text(b.text)
            if c_text:
                deferred_footnotes.append(c_text)
        elif isinstance(b, (PageBreakBlock, ChapterBreakBlock)):
            pass

    all_text = list(text_pieces) + deferred_footnotes
    doc_dict: dict[str, Any] = {
        "version": "ariad-truth/0",
        "id": doc_id,
        "canonical": {
            "text": "\n".join(all_text),
            "headings": _build_heading_tree(flat_headings),
            "tables": tables,
            "lists": lists,
            "image_count": image_count,
            "link_count": link_count,
        },
        "blocks": [block_to_dict(b) for b in blocks],
    }

    if ir_doc is not None:
        doc_dict["ir"] = ir_doc

    return doc_dict


def blocks_to_ir(
    title: str,
    lang: str | None,
    blocks: list[Any],
) -> dict[str, Any]:
    """Convert an ordered block list to an AriadShift IR JSON document in document order."""
    import base64

    body: list[dict[str, Any]] = []
    assets: dict[str, dict[str, Any]] = {}
    footnote_id = 0

    for b in blocks:
        if isinstance(b, HeadingBlock):
            body.append({
                "type": "heading",
                "level": max(1, min(6, b.level)),
                "content": [{"type": "text", "text": clean_text(b.text)}],
            })
        elif isinstance(b, ParagraphBlock):
            p_content: list[dict[str, Any]] = []
            if b.link_url:
                p_content.append({
                    "type": "link",
                    "url": b.link_url,
                    "content": [{"type": "text", "text": b.link_text or b.text}],
                })
            else:
                p_content.append({"type": "text", "text": clean_text(b.text)})

            p_block: dict[str, Any] = {"type": "paragraph", "content": p_content}
            if b.quote:
                body.append({"type": "quote", "blocks": [p_block]})
            else:
                body.append(p_block)
        elif isinstance(b, ListBlock):
            items = []
            for item_text in b.items:
                c_it = clean_text(item_text)
                if c_it:
                    items.append({
                        "blocks": [{
                            "type": "paragraph",
                            "content": [{"type": "text", "text": c_it}],
                        }]
                    })
            if items:
                body.append({
                    "type": "list",
                    "ordered": b.ordered,
                    "tight": True,
                    "items": items,
                })
        elif isinstance(b, TableBlock):
            caption_str = clean_text(b.caption)
            caption = [{"type": "text", "text": caption_str}] if caption_str else []
            max_cols = max((len(r) for r in b.rows), default=1)
            columns = [{"align": "default"} for _ in range(max_cols)]
            head: list[list[dict[str, Any]]] = []
            tbody: list[list[dict[str, Any]]] = []
            for row in b.rows:
                is_head = any(cell.is_header for cell in row)
                row_cells: list[dict[str, Any]] = []
                for cell in row:
                    c_text = clean_text(cell.text)
                    cell_blocks = [{"type": "paragraph", "content": [{"type": "text", "text": c_text}]}] if c_text else []
                    cell_dict: dict[str, Any] = {
                        "rowspan": max(1, cell.rowspan),
                        "colspan": max(1, cell.colspan),
                        "blocks": cell_blocks,
                    }
                    if cell.is_header:
                        cell_dict["header"] = True
                    row_cells.append(cell_dict)
                if is_head:
                    head.append(row_cells)
                else:
                    tbody.append(row_cells)
            body.append({
                "type": "table",
                "caption": caption,
                "columns": columns,
                "head": head,
                "body": tbody,
            })
        elif isinstance(b, FigureBlock):
            asset_id = b.asset_id or "figure-asset-1"
            if b.asset_bytes:
                b64 = base64.b64encode(b.asset_bytes).decode("ascii")
                assets[asset_id] = {
                    "bytes": b64,
                    "media_type": b.media_type,
                }
            cap_text = clean_text(b.caption or b.alt)
            caption = [{"type": "text", "text": cap_text}] if cap_text else []
            body.append({
                "type": "figure",
                "asset": {"type": "asset", "id": asset_id},
                "caption": caption,
            })
        elif isinstance(b, FootnoteBlock):
            footnote_id += 1
            body.append({
                "type": "footnote",
                "id": str(footnote_id),
                "blocks": [{
                    "type": "paragraph",
                    "content": [{"type": "text", "text": clean_text(b.text)}],
                }],
            })
        elif isinstance(b, PageBreakBlock):
            body.append({"type": "page_break"})
        elif isinstance(b, ChapterBreakBlock):
            pass

    return {
        "version": "ariad-ir/0",
        "meta": {
            "title": title,
            "language": lang,
            "authors": ["AriadShift contributors"],
            "date": None,
            "keywords": [],
            "source_format": None,
            "subject": None,
        },
        "body": body,
        "assets": assets,
        "furniture": [],
        "layout": None,
        "provenance": None,
    }


def build_unscored(doc_id: str, reason: str) -> dict[str, Any]:
    """Emit an unscored companion placeholder for fixtures whose structure exceeds canonical form."""
    return {
        "version": "ariad-truth/0",
        "id": doc_id,
        "unscored": True,
        "unscored_reason": reason,
    }


def generate(root: Path) -> dict[str, bytes]:
    """Generate authored truth companion files for all relevant fixtures."""
    from ariad_fixture_gen import docx, epub, html

    results: dict[str, bytes] = {}

    for gen in (html.generate_truth, docx.generate_truth, epub.generate_truth):
        for rel_path, doc_dict in gen(root).items():
            content = json.dumps(doc_dict, indent=2, ensure_ascii=False) + "\n"
            results[rel_path] = content.encode("utf-8")

    return results
