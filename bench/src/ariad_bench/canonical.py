"""Canonical document representation and normalization for AriadShift benchmarks.

Normalizes IR JSON, Pandoc JSON, and authored truth files into an invariant canonical form:
- NFC normalized text with whitespace collapsed
- Hierarchical heading tree as (level, text) nodes
- Tables as cell-text grids with spans (rowspan, colspan, is_header)
- List structures (ordered vs unordered with item texts)
- Image and link counts
"""

from __future__ import annotations

from dataclasses import asdict, dataclass, field
import json
from pathlib import Path
import re
from typing import Any
import unicodedata


def strip_html_tags(text: str) -> str:
    """Strip raw HTML markup tags while preserving inner text content."""
    return re.sub(r"<[^>]+>", "", text)


def nfc(text: str) -> str:
    """Normalize string to Unicode NFC form."""
    return unicodedata.normalize("NFC", text)


def collapse_whitespace(text: str) -> str:
    """Collapse contiguous whitespace sequences into a single space and strip edges."""
    return " ".join(text.split())


def clean_text(text: str) -> str:
    """Apply NFC normalization and whitespace collapse."""
    return collapse_whitespace(nfc(text))


@dataclass
class HeadingNode:
    level: int
    text: str
    children: list[HeadingNode] = field(default_factory=list)

    def to_dict(self) -> dict[str, Any]:
        return {
            "level": self.level,
            "text": self.text,
            "children": [c.to_dict() for c in self.children],
        }

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> HeadingNode:
        return cls(
            level=int(data.get("level", 1)),
            text=clean_text(str(data.get("text") or "")),
            children=[cls.from_dict(c) for c in (data.get("children") or [])],
        )


@dataclass
class TableCell:
    text: str
    rowspan: int = 1
    colspan: int = 1
    is_header: bool = False

    def to_dict(self) -> dict[str, Any]:
        return {
            "text": self.text,
            "rowspan": self.rowspan,
            "colspan": self.colspan,
            "is_header": self.is_header,
        }

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> TableCell:
        return cls(
            text=clean_text(str(data.get("text") or "")),
            rowspan=max(1, int(data.get("rowspan", 1))),
            colspan=max(1, int(data.get("colspan", 1))),
            is_header=bool(data.get("is_header", False)),
        )


@dataclass
class TableGrid:
    caption: str = ""
    rows: list[list[TableCell]] = field(default_factory=list)

    def to_dict(self) -> dict[str, Any]:
        return {
            "caption": self.caption,
            "rows": [[cell.to_dict() for cell in row] for row in self.rows],
        }

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> TableGrid:
        return cls(
            caption=clean_text(str(data.get("caption") or "")),
            rows=[[TableCell.from_dict(cell) for cell in (row or [])] for row in (data.get("rows") or [])],
        )


@dataclass
class ListStructure:
    ordered: bool = False
    items: list[str] = field(default_factory=list)

    def to_dict(self) -> dict[str, Any]:
        return {
            "ordered": self.ordered,
            "items": self.items,
        }

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> ListStructure:
        return cls(
            ordered=bool(data.get("ordered", False)),
            items=[clean_text(str(item or "")) for item in (data.get("items") or [])],
        )


@dataclass
class CanonicalDoc:
    text: str = ""
    headings: list[HeadingNode] = field(default_factory=list)
    tables: list[TableGrid] = field(default_factory=list)
    lists: list[ListStructure] = field(default_factory=list)
    image_count: int = 0
    link_count: int = 0

    def to_dict(self) -> dict[str, Any]:
        return {
            "text": self.text,
            "headings": [h.to_dict() for h in self.headings],
            "tables": [t.to_dict() for t in self.tables],
            "lists": [lst.to_dict() for lst in self.lists],
            "image_count": self.image_count,
            "link_count": self.link_count,
        }

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> CanonicalDoc:
        raw_text = str(data.get("text") or "")
        cleaned_lines = [clean_text(line) for line in raw_text.split("\n")]
        return cls(
            text="\n".join(line for line in cleaned_lines if line),
            headings=[HeadingNode.from_dict(h) for h in (data.get("headings") or [])],
            tables=[TableGrid.from_dict(t) for t in (data.get("tables") or [])],
            lists=[ListStructure.from_dict(lst) for lst in (data.get("lists") or [])],
            image_count=max(0, int(data.get("image_count", 0))),
            link_count=max(0, int(data.get("link_count", 0))),
        )


def _build_heading_tree(flat_headings: list[tuple[int, str]]) -> list[HeadingNode]:
    """Assemble a sequential list of (level, text) into a nested tree of HeadingNodes."""
    root_nodes: list[HeadingNode] = []
    # Stack stores (level, node)
    stack: list[tuple[int, HeadingNode]] = []

    for level, text in flat_headings:
        node = HeadingNode(level=level, text=clean_text(text))
        while stack and stack[-1][0] >= level:
            stack.pop()

        if not stack:
            root_nodes.append(node)
        else:
            stack[-1][1].children.append(node)

        stack.append((level, node))

    return root_nodes


def from_ir(doc: dict[str, Any]) -> CanonicalDoc:
    """Extract canonical document representation from an AriadShift IR JSON document."""
    text_pieces: list[str] = []
    deferred_footnotes: list[str] = []
    flat_headings: list[tuple[int, str]] = []
    tables: list[TableGrid] = []
    lists: list[ListStructure] = []
    counts = {"images": 0, "links": 0}

    def process_inlines(inlines: list[dict[str, Any]]) -> str:
        parts: list[str] = []
        for inline in inlines:
            inline_type = inline.get("type", "")
            if inline_type == "text":
                parts.append(inline.get("text", ""))
            elif inline_type in ("emph", "strong", "strikeout", "superscript", "subscript"):
                parts.append(process_inlines(inline.get("content", [])))
            elif inline_type == "code":
                parts.append(inline.get("text", ""))
            elif inline_type == "math":
                parts.append(inline.get("tex", ""))
            elif inline_type == "link":
                counts["links"] += 1
                parts.append(process_inlines(inline.get("content", [])))
            elif inline_type == "image":
                counts["images"] += 1
                alt = inline.get("alt", "")
                if alt:
                    parts.append(alt)
            elif inline_type in ("soft_break", "line_break"):
                parts.append(" ")
            elif inline_type == "footnote_ref":
                pass
            elif inline_type == "raw":
                s = strip_html_tags(inline.get("text", ""))
                if s:
                    parts.append(s)
        return "".join(parts)

    def process_blocks(blocks: list[dict[str, Any]]) -> None:
        for block in blocks:
            block_type = block.get("type", "")
            if block_type == "heading":
                level = int(block.get("level", 1))
                h_text = clean_text(process_inlines(block.get("content", [])))
                if h_text:
                    flat_headings.append((level, h_text))
                    text_pieces.append(h_text)
            elif block_type == "paragraph":
                p_text = clean_text(process_inlines(block.get("content", [])))
                if p_text:
                    text_pieces.append(p_text)
            elif block_type == "code":
                c_text = clean_text(block.get("text", ""))
                if c_text:
                    text_pieces.append(c_text)
            elif block_type == "math":
                m_text = clean_text(block.get("tex", ""))
                if m_text:
                    text_pieces.append(m_text)
            elif block_type == "quote":
                process_blocks(block.get("blocks", []))
            elif block_type == "raw":
                s = clean_text(strip_html_tags(block.get("text", "")))
                if s:
                    text_pieces.append(s)
            elif block_type == "list":
                ordered = bool(block.get("ordered", False))
                items_text: list[str] = []
                list_obj = ListStructure(ordered=ordered, items=items_text)
                lists.append(list_obj)

                for item in block.get("items", []):
                    item_prefix = ""
                    if item.get("checked") is True:
                        item_prefix = "☒ "
                    elif item.get("checked") is False:
                        item_prefix = "☐ "

                    item_blocks = item.get("blocks", [])
                    item_parts: list[str] = []
                    nested_lists_to_process: list[dict[str, Any]] = []

                    for ib in item_blocks:
                        ib_type = ib.get("type")
                        if ib_type == "paragraph":
                            t = clean_text(process_inlines(ib.get("content", [])))
                            if t:
                                item_parts.append(t)
                        elif ib_type == "heading":
                            ht = clean_text(process_inlines(ib.get("content", [])))
                            if ht:
                                flat_headings.append((int(ib.get("level", 1)), ht))
                                item_parts.append(ht)
                        elif ib_type == "code":
                            ct = clean_text(ib.get("text", ""))
                            if ct:
                                item_parts.append(ct)
                        elif ib_type == "list":
                            nested_lists_to_process.append(ib)
                        else:
                            process_blocks([ib])

                    joined_item = clean_text(item_prefix + " ".join(item_parts))
                    if joined_item:
                        items_text.append(joined_item)
                        text_pieces.append(joined_item)

                    # Process nested lists in document order AFTER parent item
                    for nl in nested_lists_to_process:
                        process_blocks([nl])
            elif block_type == "table":
                caption_text = clean_text(process_inlines(block.get("caption") or []))
                if caption_text:
                    text_pieces.append(caption_text)

                grid_rows: list[list[TableCell]] = []

                # Head rows
                for head_row in block.get("head", []):
                    row_cells: list[TableCell] = []
                    for cell in head_row:
                        cell_text_parts: list[str] = []
                        for cb in cell.get("blocks", []):
                            if cb.get("type") == "paragraph":
                                t = clean_text(process_inlines(cb.get("content", [])))
                                if t:
                                    cell_text_parts.append(t)
                        cell_str = clean_text(" ".join(cell_text_parts))
                        if cell_str:
                            text_pieces.append(cell_str)
                        row_cells.append(
                            TableCell(
                                text=cell_str,
                                rowspan=int(cell.get("rowspan", 1)),
                                colspan=int(cell.get("colspan", 1)),
                                is_header=True,
                            )
                        )
                    if row_cells:
                        grid_rows.append(row_cells)

                # Body rows
                for body_row in block.get("body", []):
                    row_cells = []
                    for cell in body_row:
                        cell_text_parts = []
                        for cb in cell.get("blocks", []):
                            if cb.get("type") == "paragraph":
                                t = clean_text(process_inlines(cb.get("content", [])))
                                if t:
                                    cell_text_parts.append(t)
                            elif cb.get("type") == "table":
                                process_blocks([cb])
                        cell_str = clean_text(" ".join(cell_text_parts))
                        if cell_str:
                            text_pieces.append(cell_str)
                        row_cells.append(
                            TableCell(
                                text=cell_str,
                                rowspan=int(cell.get("rowspan", 1)),
                                colspan=int(cell.get("colspan", 1)),
                                is_header=bool(cell.get("header", False)),
                            )
                        )
                    if row_cells:
                        grid_rows.append(row_cells)

                tables.append(TableGrid(caption=caption_text, rows=grid_rows))
            elif block_type == "figure":
                counts["images"] += 1
                fig_caption = clean_text(process_inlines(block.get("caption") or []))
                if fig_caption:
                    text_pieces.append(fig_caption)
            elif block_type == "footnote":
                fn_parts: list[str] = []
                for fb in block.get("blocks") or []:
                    if fb.get("type") == "paragraph":
                        t = clean_text(process_inlines(fb.get("content", [])))
                        if t:
                            fn_parts.append(t)
                    elif fb.get("type") == "code":
                        t = clean_text(fb.get("text", ""))
                        if t:
                            fn_parts.append(t)
                fn_text = " ".join(fn_parts)
                if fn_text:
                    deferred_footnotes.append(fn_text)

    body_blocks = doc.get("body") or []
    process_blocks(body_blocks)

    # Footnotes appended at end of document in document order
    text_pieces.extend(deferred_footnotes)

    combined_text = "\n".join(text_pieces)
    return CanonicalDoc(
        text=combined_text,
        headings=_build_heading_tree(flat_headings),
        tables=tables,
        lists=lists,
        image_count=counts["images"],
        link_count=counts["links"],
    )


def from_pandoc(doc: dict[str, Any]) -> CanonicalDoc:
    """Extract canonical document representation from a Pandoc AST JSON document."""
    text_pieces: list[str] = []
    deferred_footnotes: list[str] = []
    flat_headings: list[tuple[int, str]] = []
    tables: list[TableGrid] = []
    lists: list[ListStructure] = []
    counts = {"images": 0, "links": 0}

    def process_inlines(inlines: list[dict[str, Any]]) -> str:
        parts: list[str] = []
        for inline in inlines:
            t = inline.get("t", "")
            c = inline.get("c")
            if t == "Str":
                parts.append(str(c))
            elif t in ("Space", "SoftBreak", "LineBreak"):
                parts.append(" ")
            elif t in ("Emph", "Strong", "Underline", "Strikeout", "Superscript", "Subscript", "SmallCaps"):
                if isinstance(c, list):
                    parts.append(process_inlines(c))
            elif t == "Code":
                # c is [attr, text]
                if isinstance(c, list) and len(c) >= 2:
                    parts.append(str(c[1]))
            elif t == "Math":
                # c is [math_type, text]
                if isinstance(c, list) and len(c) >= 2:
                    parts.append(str(c[1]))
            elif t == "Link":
                counts["links"] += 1
                # c is [attr, inlines, target]
                if isinstance(c, list) and len(c) >= 2 and isinstance(c[1], list):
                    parts.append(process_inlines(c[1]))
            elif t == "Image":
                counts["images"] += 1
                # c is [attr, inlines, target]
                if isinstance(c, list) and len(c) >= 2 and isinstance(c[1], list):
                    parts.append(process_inlines(c[1]))
            elif t == "Note":
                # Defer footnote text to end of document
                if isinstance(c, list):
                    fn_parts: list[str] = []
                    for fb in c:
                        if fb.get("t") in ("Para", "Plain") and isinstance(fb.get("c"), list):
                            fn_txt = clean_text(process_inlines(fb.get("c")))
                            if fn_txt:
                                fn_parts.append(fn_txt)
                        elif fb.get("t") == "CodeBlock":
                            fbc = fb.get("c")
                            if isinstance(fbc, list) and len(fbc) >= 2:
                                code_txt = clean_text(str(fbc[1]))
                                if code_txt:
                                    fn_parts.append(code_txt)
                    fn_str = " ".join(fn_parts)
                    if fn_str:
                        deferred_footnotes.append(fn_str)
            elif t == "RawInline":
                if isinstance(c, list) and len(c) >= 2:
                    raw_txt = strip_html_tags(str(c[1]))
                    if raw_txt:
                        parts.append(raw_txt)
            elif t in ("Span", "Quoted"):
                if isinstance(c, list):
                    inline_list = c[1] if len(c) == 2 and isinstance(c[1], list) else c
                    if isinstance(inline_list, list):
                        parts.append(process_inlines(inline_list))
        return "".join(parts)

    def process_blocks(blocks: list[dict[str, Any]]) -> None:
        for block in blocks:
            t = block.get("t", "")
            c = block.get("c")
            if t == "Header":
                # c is [level, attr, inlines]
                if isinstance(c, list) and len(c) >= 3:
                    level = int(c[0])
                    h_text = clean_text(process_inlines(c[2]))
                    if h_text:
                        flat_headings.append((level, h_text))
                        text_pieces.append(h_text)
            elif t in ("Para", "Plain"):
                if isinstance(c, list):
                    p_text = clean_text(process_inlines(c))
                    if p_text:
                        text_pieces.append(p_text)
            elif t == "CodeBlock":
                # c is [attr, text]
                if isinstance(c, list) and len(c) >= 2:
                    code_str = clean_text(str(c[1]))
                    if code_str:
                        text_pieces.append(code_str)
            elif t == "BlockQuote":
                if isinstance(c, list):
                    process_blocks(c)
            elif t == "RawBlock":
                if isinstance(c, list) and len(c) >= 2:
                    raw_txt = clean_text(strip_html_tags(str(c[1])))
                    if raw_txt:
                        text_pieces.append(raw_txt)
            elif t == "DefinitionList":
                if isinstance(c, list):
                    for entry in c:
                        if isinstance(entry, list) and len(entry) >= 2:
                            term_inlines, def_blocks_list = entry[0], entry[1]
                            if isinstance(term_inlines, list):
                                term_txt = clean_text(process_inlines(term_inlines))
                                if term_txt:
                                    text_pieces.append(term_txt)
                            if isinstance(def_blocks_list, list):
                                for def_blocks in def_blocks_list:
                                    if isinstance(def_blocks, list):
                                        process_blocks(def_blocks)
            elif t == "LineBlock":
                if isinstance(c, list):
                    line_parts = []
                    for line_inlines in c:
                        if isinstance(line_inlines, list):
                            ltxt = clean_text(process_inlines(line_inlines))
                            if ltxt:
                                line_parts.append(ltxt)
                    joined_lb = " ".join(line_parts)
                    if joined_lb:
                        text_pieces.append(joined_lb)
            elif t in ("BulletList", "OrderedList"):
                ordered = (t == "OrderedList")
                items_list = c[1] if ordered and isinstance(c, list) and len(c) >= 2 else c
                if isinstance(items_list, list):
                    items_text = []
                    list_obj = ListStructure(ordered=ordered, items=items_text)
                    lists.append(list_obj)

                    for item_blocks in items_list:
                        item_parts = []
                        nested_lists = []
                        if isinstance(item_blocks, list):
                            for b in item_blocks:
                                bt = b.get("t")
                                bc = b.get("c")
                                if bt in ("Para", "Plain") and isinstance(bc, list):
                                    txt = clean_text(process_inlines(bc))
                                    if txt:
                                        item_parts.append(txt)
                                elif bt == "Header" and isinstance(bc, list) and len(bc) >= 3:
                                    htxt = clean_text(process_inlines(bc[2]))
                                    if htxt:
                                        flat_headings.append((int(bc[0]), htxt))
                                        item_parts.append(htxt)
                                elif bt == "CodeBlock" and isinstance(bc, list) and len(bc) >= 2:
                                    ctxt = clean_text(str(bc[1]))
                                    if ctxt:
                                        item_parts.append(ctxt)
                                elif bt in ("BulletList", "OrderedList"):
                                    nested_lists.append(b)
                                else:
                                    process_blocks([b])

                        joined_item = " ".join(item_parts)
                        if joined_item:
                            items_text.append(joined_item)
                            text_pieces.append(joined_item)

                        # Process nested lists in document order AFTER parent item
                        for nb in nested_lists:
                            process_blocks([nb])
            elif t == "Table":
                caption_text = ""
                grid_rows: list[list[TableCell]] = []
                if isinstance(c, list) and len(c) >= 6:
                    caption_spec = c[1]
                    if isinstance(caption_spec, list) and len(caption_spec) >= 2 and isinstance(caption_spec[1], list):
                        for cb in caption_spec[1]:
                            if isinstance(cb, dict) and cb.get("t") in ("Para", "Plain"):
                                caption_text = clean_text(process_inlines(cb.get("c", [])))
                            elif isinstance(cb, list):
                                for nested_b in cb:
                                    if isinstance(nested_b, dict) and nested_b.get("t") in ("Para", "Plain"):
                                        caption_text = clean_text(process_inlines(nested_b.get("c", [])))
                    if caption_text:
                        text_pieces.append(caption_text)

                    def _extract_pandoc_row(row_spec: Any, *, is_header: bool) -> None:
                        if isinstance(row_spec, list) and len(row_spec) >= 2:
                            r_cells: list[TableCell] = []
                            for cell in row_spec[1]:
                                if isinstance(cell, list) and len(cell) >= 5:
                                    rowspan = int(cell[2])
                                    colspan = int(cell[3])
                                    cell_parts = []
                                    for cb in cell[4]:
                                        if cb.get("t") in ("Para", "Plain"):
                                            t_str = clean_text(process_inlines(cb.get("c", [])))
                                            if t_str:
                                                cell_parts.append(t_str)
                                        elif cb.get("t") == "Table":
                                            process_blocks([cb])
                                    cell_str = clean_text(" ".join(cell_parts))
                                    if cell_str:
                                        text_pieces.append(cell_str)
                                    r_cells.append(
                                        TableCell(
                                            text=cell_str,
                                            rowspan=rowspan,
                                            colspan=colspan,
                                            is_header=is_header,
                                        )
                                    )
                            if r_cells:
                                grid_rows.append(r_cells)

                    # 1. Head: head is [attr, [rows]]
                    head_spec = c[3]
                    if isinstance(head_spec, list) and len(head_spec) >= 2:
                        for row in head_spec[1]:
                            _extract_pandoc_row(row, is_header=True)

                    # 2. Bodies: list of [attr, rowhead_cols, interim_head_rows, body_rows]
                    bodies_spec = c[4]
                    if isinstance(bodies_spec, list):
                        for body in bodies_spec:
                            if isinstance(body, list) and len(body) >= 4:
                                # Intermediate head rows
                                if isinstance(body[2], list):
                                    for row in body[2]:
                                        _extract_pandoc_row(row, is_header=True)
                                # Body rows
                                if isinstance(body[3], list):
                                    for row in body[3]:
                                        _extract_pandoc_row(row, is_header=False)

                    # 3. Foot: foot is [attr, [rows]]
                    if len(c) >= 6:
                        foot_spec = c[5]
                        if isinstance(foot_spec, list) and len(foot_spec) >= 2:
                            for row in foot_spec[1]:
                                _extract_pandoc_row(row, is_header=False)

                tables.append(TableGrid(caption=caption_text, rows=grid_rows))
            elif t == "Figure":
                counts["images"] += 1
                if isinstance(c, list) and len(c) >= 3 and isinstance(c[2], list):
                    process_blocks(c[2])
            elif t == "Div":
                if isinstance(c, list) and len(c) >= 2 and isinstance(c[1], list):
                    process_blocks(c[1])

    blocks = doc.get("blocks") or []
    process_blocks(blocks)

    # Footnotes appended at end of document in document order
    text_pieces.extend(deferred_footnotes)

    combined_text = "\n".join(text_pieces)
    return CanonicalDoc(
        text=combined_text,
        headings=_build_heading_tree(flat_headings),
        tables=tables,
        lists=lists,
        image_count=counts["images"],
        link_count=counts["links"],
    )


def normalize(source: Any) -> CanonicalDoc:
    """Produce the canonical document representation from IR JSON, Pandoc JSON, or truth files."""
    if isinstance(source, CanonicalDoc):
        return source

    if isinstance(source, Path):
        if not source.is_file():
            raise FileNotFoundError(f"File not found: {source}")
        text = source.read_text(encoding="utf-8")
        data = json.loads(text)
    elif isinstance(source, str):
        path = Path(source)
        if path.is_file():
            text = path.read_text(encoding="utf-8")
            data = json.loads(text)
        else:
            data = json.loads(source)
    elif isinstance(source, dict):
        data = source
    else:
        raise TypeError(f"Unsupported source type for canonical normalization: {type(source)}")

    if isinstance(data, dict):
        # 1. Authored truth file containing "canonical"
        if "canonical" in data and isinstance(data["canonical"], dict):
            return CanonicalDoc.from_dict(data["canonical"])

        # 1b. Authored truth file containing "ir"
        if "ir" in data and isinstance(data["ir"], dict):
            return from_ir(data["ir"])

        # 2. AriadShift IR JSON document
        if "version" in data and "body" in data:
            return from_ir(data)

        # 3. Pandoc JSON AST document
        if "pandoc-api-version" in data and "blocks" in data:
            return from_pandoc(data)

        # 4. Canonical form dict directly (e.g. from truth companion)
        if "text" in data and "headings" in data and "tables" in data:
            return CanonicalDoc.from_dict(data)

    raise ValueError(f"Unable to determine document format for canonical normalization: {data.keys() if isinstance(data, dict) else type(data)}")


def canonical_to_ir(source: CanonicalDoc | dict[str, Any], title: str = "document") -> dict[str, Any]:
    """Convert a canonical document, dictionary, or truth companion into an AriadShift IR JSON document."""
    raw_blocks: list[dict[str, Any]] | None = None
    assets: dict[str, Any] = {}

    if isinstance(source, CanonicalDoc):
        data = source.to_dict()
    elif isinstance(source, dict):
        if "blocks" in source and isinstance(source["blocks"], list):
            raw_blocks = source["blocks"]
        if "canonical" in source and isinstance(source["canonical"], dict):
            data = source["canonical"]
        else:
            data = source
        if raw_blocks is None and "blocks" in data and isinstance(data["blocks"], list):
            raw_blocks = data["blocks"]
    else:
        raise TypeError(f"Unsupported source for canonical_to_ir: {type(source)}")

    blocks: list[dict[str, Any]] = []

    if raw_blocks is not None:
        footnote_id = 0
        fig_idx = 0
        for b in raw_blocks:
            b_type = b.get("type", "")
            if b_type == "heading":
                t = clean_text(str(b.get("text") or ""))
                if t:
                    level = max(1, min(6, int(b.get("level", 1))))
                    blocks.append({
                        "type": "heading",
                        "level": level,
                        "content": [{"type": "text", "text": t}],
                    })
            elif b_type == "paragraph":
                t = clean_text(str(b.get("text") or ""))
                if t:
                    blocks.append({
                        "type": "paragraph",
                        "content": [{"type": "text", "text": t}],
                    })
            elif b_type == "list":
                items: list[dict[str, Any]] = []
                for item_str in b.get("items") or []:
                    c_it = clean_text(str(item_str))
                    if c_it:
                        items.append({
                            "blocks": [{
                                "type": "paragraph",
                                "content": [{"type": "text", "text": c_it}],
                            }]
                        })
                if items:
                    blocks.append({
                        "type": "list",
                        "ordered": bool(b.get("ordered", False)),
                        "tight": True,
                        "items": items,
                    })
            elif b_type == "table":
                caption_str = clean_text(str(b.get("caption", "")))
                caption = [{"type": "text", "text": caption_str}] if caption_str else []
                rows = b.get("rows", [])
                max_cols = max((len(r) for r in rows), default=1)
                columns = [{"align": "default"} for _ in range(max_cols)]
                head: list[list[dict[str, Any]]] = []
                body: list[list[dict[str, Any]]] = []
                for row in rows:
                    is_head = any(bool(cell.get("is_header", False)) for cell in row)
                    row_cells: list[dict[str, Any]] = []
                    for cell in row:
                        c_text = clean_text(str(cell.get("text", "")))
                        cell_blocks = [{"type": "paragraph", "content": [{"type": "text", "text": c_text}]}] if c_text else []
                        cell_dict: dict[str, Any] = {
                            "rowspan": max(1, int(cell.get("rowspan", 1))),
                            "colspan": max(1, int(cell.get("colspan", 1))),
                            "blocks": cell_blocks,
                        }
                        if cell.get("is_header"):
                            cell_dict["header"] = True
                        row_cells.append(cell_dict)
                    if is_head:
                        head.append(row_cells)
                    else:
                        body.append(row_cells)
                blocks.append({
                    "type": "table",
                    "caption": caption,
                    "columns": columns,
                    "head": head,
                    "body": body,
                })
            elif b_type == "figure":
                fig_idx += 1
                asset_id = str(b.get("asset_id") or f"figure-asset-{fig_idx}")
                b64 = b.get("asset_bytes_b64") or b.get("asset_b64")
                if b64:
                    assets[asset_id] = {
                        "bytes": b64,
                        "media_type": b.get("media_type", "image/png"),
                    }
                cap_text = clean_text(str(b.get("caption") or b.get("alt") or ""))
                caption = [{"type": "text", "text": cap_text}] if cap_text else []
                blocks.append({
                    "type": "figure",
                    "asset": {"type": "asset", "id": asset_id},
                    "caption": caption,
                })
            elif b_type == "footnote":
                footnote_id += 1
                t = clean_text(str(b.get("text") or ""))
                blocks.append({
                    "type": "footnote",
                    "id": str(footnote_id),
                    "blocks": [{
                        "type": "paragraph",
                        "content": [{"type": "text", "text": t}],
                    }],
                })
            elif b_type == "page_break":
                blocks.append({"type": "page_break"})
    else:
        # Fallback when raw blocks are not available
        emitted_texts: set[str] = set()

        # 1. Headings in tree order
        def walk_headings(nodes: list[dict[str, Any]]) -> list[dict[str, Any]]:
            res: list[dict[str, Any]] = []
            for n in nodes:
                t = clean_text(str(n.get("text") or ""))
                level = max(1, min(6, int(n.get("level", 1))))
                if t:
                    res.append({
                        "type": "heading",
                        "level": level,
                        "content": [{"type": "text", "text": t}],
                    })
                    emitted_texts.add(t)
                res.extend(walk_headings(n.get("children") or []))
            return res

        blocks.extend(walk_headings(data.get("headings") or []))

        # 2. Lists
        for l in data.get("lists") or []:
            items = []
            for item_str in l.get("items") or []:
                c_it = clean_text(str(item_str))
                if c_it:
                    items.append({
                        "blocks": [{
                            "type": "paragraph",
                            "content": [{"type": "text", "text": c_it}],
                        }]
                    })
                    emitted_texts.add(c_it)
            if items:
                blocks.append({
                    "type": "list",
                    "ordered": bool(l.get("ordered", False)),
                    "tight": True,
                    "items": items,
                })

        # 3. Tables
        for tbl in data.get("tables") or []:
            caption_str = clean_text(str(tbl.get("caption", ""))) if isinstance(tbl, dict) else ""
            caption = [{"type": "text", "text": caption_str}] if caption_str else []
            if caption_str:
                emitted_texts.add(caption_str)

            rows = tbl.get("rows", []) if isinstance(tbl, dict) else tbl
            max_cols = max((len(r) for r in rows), default=1)
            columns = [{"align": "default"} for _ in range(max_cols)]
            head = []
            body = []

            for row in rows:
                row_cells = []
                is_head = False
                for cell in row:
                    c_text = clean_text(cell.text if hasattr(cell, "text") else str(cell.get("text", "")))
                    r_span = cell.rowspan if hasattr(cell, "rowspan") else int(cell.get("rowspan", 1))
                    c_span = cell.colspan if hasattr(cell, "colspan") else int(cell.get("colspan", 1))
                    h_flag = cell.is_header if hasattr(cell, "is_header") else bool(cell.get("is_header", False))
                    if h_flag:
                        is_head = True
                    if c_text:
                        emitted_texts.add(c_text)
                    cell_blocks = [{"type": "paragraph", "content": [{"type": "text", "text": c_text}]}] if c_text else []
                    row_cells.append({
                        "rowspan": max(1, r_span),
                        "colspan": max(1, c_span),
                        "header": h_flag,
                        "blocks": cell_blocks,
                    })
                if is_head:
                    head.append(row_cells)
                else:
                    body.append(row_cells)

            blocks.append({
                "type": "table",
                "caption": caption,
                "columns": columns,
                "head": head,
                "body": body,
            })

        # 4. Paragraphs from remaining text
        for line in str(data.get("text") or "").splitlines():
            line_clean = clean_text(line)
            if line_clean and line_clean not in emitted_texts:
                blocks.append({
                    "type": "paragraph",
                    "content": [{"type": "text", "text": line_clean}],
                })
                emitted_texts.add(line_clean)

    return {
        "version": "ariad-ir/0",
        "meta": {
            "title": title,
            "authors": [],
            "language": None,
            "date": None,
            "subject": None,
            "keywords": [],
            "source_format": None,
        },
        "body": blocks,
        "furniture": [],
        "assets": assets,
        "layout": None,
        "provenance": None,
    }
