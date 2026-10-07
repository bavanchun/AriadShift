"""Generate small semantic HTML fixtures from original source text and ordered blocks."""

from __future__ import annotations

from html import escape
from pathlib import Path
from typing import Any

from ariad_fixture_gen.truth import (
    Cell,
    HeadingBlock,
    ParagraphBlock,
    TableBlock,
    blocks_to_canonical,
    blocks_to_ir,
)


def _source(root: Path, filename: str) -> list[str]:
    source_path = root / "fixtures" / "gen" / "src" / "ariad_fixture_gen" / "text" / filename
    return [line for line in source_path.read_text(encoding="utf-8").splitlines() if line]


def _document(language: str, title: str, body: str) -> bytes:
    markup = (
        "<!doctype html>\n"
        f'<html lang="{language}">\n'
        "<head>\n"
        '  <meta charset="utf-8">\n'
        '  <meta name="viewport" content="width=device-width, initial-scale=1">\n'
        f"  <title>{escape(title)}</title>\n"
        "</head>\n"
        "<body>\n"
        f"{body}\n"
        "</body>\n"
        "</html>\n"
    )
    return markup.encode("utf-8")


def _get_specs(root: Path) -> list[tuple[str, str, str, str, list[Any]]]:
    """Return (fixture_path, fixture_id, title, lang, blocks) for all HTML fixtures."""
    garden = _source(root, "vietnamese-garden.md")
    river = _source(root, "vietnamese-river.md")
    repair_cafe = _source(root, "english-repair-cafe.md")
    map_room = _source(root, "english-map-room.md")

    return [
        (
            "fixtures/html/vi-garden-notice.html",
            "vi-garden-notice",
            "Lịch chăm vườn đọc",
            "vi",
            [
                HeadingBlock(level=1, text="Lịch chăm vườn đọc"),
                ParagraphBlock(text=garden[0]),
                ParagraphBlock(text=garden[1]),
            ],
        ),
        (
            "fixtures/html/vi-route-table.html",
            "vi-route-table",
            "Các điểm dừng ven sông",
            "vi",
            [
                HeadingBlock(level=1, text="Các điểm dừng ven sông"),
                ParagraphBlock(text=river[0]),
                TableBlock(
                    caption="Thời gian đi bộ",
                    rows=[
                        [Cell(text="Điểm dừng", is_header=True), Cell(text="Phút", is_header=True)],
                        [Cell(text="Bến sông"), Cell(text="8")],
                        [Cell(text="Vườn đọc"), Cell(text="14")],
                    ],
                ),
            ],
        ),
        (
            "fixtures/html/en-repair-cafe.html",
            "en-repair-cafe",
            "The Repair Café",
            "en",
            [
                HeadingBlock(level=1, text="The Repair Café"),
                ParagraphBlock(text=repair_cafe[0]),
                ParagraphBlock(text=repair_cafe[1]),
                ParagraphBlock(text=repair_cafe[2].split(". ")[0] + ".", quote=True),
            ],
        ),
        (
            "fixtures/html/en-map-archive.html",
            "en-map-archive",
            "Notes from the Map Room",
            "en",
            [
                HeadingBlock(level=1, text="Notes from the Map Room"),
                ParagraphBlock(text=map_room[0]),
                ParagraphBlock(text=map_room[1]),
                ParagraphBlock(
                    text="Browse the map index",
                    link_url="https://example.org/map-index",
                    link_text="Browse the map index",
                ),
            ],
        ),
    ]


def _render_html(doc_id: str, title: str, lang: str, blocks: list[Any]) -> bytes:
    """Render HTML document bytes from the ordered block list."""
    body_lines: list[str] = []
    wrapper = "article" if "route" in doc_id or "map" in doc_id else "main"
    body_lines.append(f"<{wrapper}>")

    for b in blocks:
        if isinstance(b, HeadingBlock):
            body_lines.append(f"  <h{b.level}>{escape(b.text)}</h{b.level}>")
        elif isinstance(b, ParagraphBlock):
            if b.quote:
                body_lines.append(f"  <blockquote><p>{escape(b.text)}</p></blockquote>")
            elif b.link_url:
                body_lines.append(f'  <a href="{escape(b.link_url)}">{escape(b.link_text or b.text)}</a>')
            else:
                body_lines.append(f"  <p>{escape(b.text)}</p>")
        elif isinstance(b, TableBlock):
            body_lines.append("  <table>")
            if b.caption:
                body_lines.append(f"    <caption>{escape(b.caption)}</caption>")
            if b.rows:
                def _format_cell(c: Cell) -> str:
                    tag = "th" if c.is_header else "td"
                    attrs: list[str] = []
                    if c.is_header:
                        attrs.append('scope="col"')
                    if c.colspan > 1:
                        attrs.append(f'colspan="{c.colspan}"')
                    if c.rowspan > 1:
                        attrs.append(f'rowspan="{c.rowspan}"')
                    attr_str = (" " + " ".join(attrs)) if attrs else ""
                    return f"<{tag}{attr_str}>{escape(c.text)}</{tag}>"

                head_rows = [r for r in b.rows if any(c.is_header for c in r)]
                body_rows = [r for r in b.rows if not any(c.is_header for c in r)]
                if head_rows:
                    head_inner = "".join(
                        "<tr>" + "".join(_format_cell(c) for c in r) + "</tr>"
                        for r in head_rows
                    )
                    body_lines.append(f"    <thead>{head_inner}</thead>")
                if body_rows:
                    tbody_inner = "".join(
                        "<tr>" + "".join(_format_cell(c) for c in row) + "</tr>"
                        for row in body_rows
                    )
                    body_lines.append(f"    <tbody>{tbody_inner}</tbody>")
                elif not head_rows:
                    tbody_inner = "".join(
                        "<tr>" + "".join(_format_cell(c) for c in row) + "</tr>"
                        for row in b.rows
                    )
                    body_lines.append(f"    <tbody>{tbody_inner}</tbody>")
            body_lines.append("  </table>")

    body_lines.append(f"</{wrapper}>")
    body = "\n".join(body_lines)
    return _document(lang, title, body)


def generate(root: Path) -> dict[str, bytes]:
    """Generate all HTML fixtures directly from their ordered block specifications."""
    specs = _get_specs(root)
    return {
        path: _render_html(doc_id, title, lang, blocks)
        for path, doc_id, title, lang, blocks in specs
    }


def generate_truth(root: Path) -> dict[str, dict[str, Any]]:
    """Build canonical truth companions from the exact same ordered block specifications."""
    specs = _get_specs(root)
    truth_dict: dict[str, dict[str, Any]] = {}
    for path, doc_id, title, lang, blocks in specs:
        truth_path = path.replace(".html", ".truth.json")
        ir_doc = blocks_to_ir(title, lang, blocks)
        truth_dict[truth_path] = blocks_to_canonical(doc_id, blocks, ir_doc=ir_doc)
    return truth_dict
