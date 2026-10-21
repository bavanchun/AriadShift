"""Generate reproducible EPUB 3 fixtures with structural edge cases."""

from __future__ import annotations

from dataclasses import dataclass
from html import escape
from io import BytesIO
from pathlib import Path
from typing import Any
from zipfile import ZIP_DEFLATED, ZIP_STORED, ZipFile, ZipInfo

from ariad_fixture_gen.truth import (
    Cell,
    ChapterBreakBlock,
    FigureBlock,
    FootnoteBlock,
    HeadingBlock,
    ParagraphBlock,
    TableBlock,
    blocks_to_canonical,
    blocks_to_ir,
)


def _source(root: Path, filename: str) -> list[str]:
    path = root / "fixtures" / "gen" / "src" / "ariad_fixture_gen" / "text" / filename
    return [part.strip() for part in path.read_text(encoding="utf-8").split("\n\n") if part.strip()]


def _build_epub(
    identifier: str,
    title: str,
    language: str,
    documents: list[tuple[str, str, str]],  # (file_name, nav_title, body_html)
    *,
    images: dict[str, tuple[str, bytes]] | None = None,  # relative_href -> (media_type, data)
) -> bytes:
    """Pack an EPUB 3 archive reproducibly using standard library zipfile."""
    images = images or {}
    output = BytesIO()

    # Fixed timestamp for reproducible builds
    fixed_time = (1980, 1, 1, 0, 0, 0)

    with ZipFile(output, "w") as archive:
        # 1. mimetype MUST be first entry and ZIP_STORED (uncompressed)
        mimetype_info = ZipInfo("mimetype", fixed_time)
        mimetype_info.compress_type = ZIP_STORED
        mimetype_info.create_system = 0
        mimetype_info.external_attr = 0o644 << 16
        archive.writestr(mimetype_info, b"application/epub+zip")

        # 2. META-INF/container.xml
        container_xml = (
            '<?xml version="1.0" encoding="UTF-8"?>\n'
            '<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">\n'
            "  <rootfiles>\n"
            '    <rootfile full-path="EPUB/package.opf" media-type="application/oebps-package+xml"/>\n'
            "  </rootfiles>\n"
            "</container>\n"
        )
        container_info = ZipInfo("META-INF/container.xml", fixed_time)
        container_info.compress_type = ZIP_DEFLATED
        container_info.create_system = 0
        container_info.external_attr = 0o644 << 16
        archive.writestr(container_info, container_xml.encode("utf-8"))

        # 3. EPUB/nav.xhtml
        nav_items_html = "\n".join(
            f'      <li><a href="{escape(fname)}">{escape(item_title)}</a></li>'
            for fname, item_title, _ in documents
        )
        nav_xhtml = (
            '<?xml version="1.0" encoding="utf-8"?>\n'
            '<!DOCTYPE html>\n'
            f'<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops" lang="{escape(language)}">\n'
            "<head>\n"
            '  <meta charset="utf-8" />\n'
            f"  <title>{escape(title)}</title>\n"
            "</head>\n"
            "<body>\n"
            '  <nav epub:type="toc" id="toc">\n'
            f"    <h1>{escape(title)}</h1>\n"
            "    <ol>\n"
            f"{nav_items_html}\n"
            "    </ol>\n"
            "  </nav>\n"
            "</body>\n"
            "</html>\n"
        )
        nav_info = ZipInfo("EPUB/nav.xhtml", fixed_time)
        nav_info.compress_type = ZIP_DEFLATED
        nav_info.create_system = 0
        nav_info.external_attr = 0o644 << 16
        archive.writestr(nav_info, nav_xhtml.encode("utf-8"))

        # 4. Content documents
        for fname, doc_title, body_html in documents:
            doc_xhtml = (
                '<?xml version="1.0" encoding="utf-8"?>\n'
                '<!DOCTYPE html>\n'
                f'<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops" lang="{escape(language)}">\n'
                "<head>\n"
                '  <meta charset="utf-8" />\n'
                f"  <title>{escape(doc_title)}</title>\n"
                "</head>\n"
                "<body>\n"
                f"{body_html}\n"
                "</body>\n"
                "</html>\n"
            )
            doc_info = ZipInfo(f"EPUB/{fname}", fixed_time)
            doc_info.compress_type = ZIP_DEFLATED
            doc_info.create_system = 0
            doc_info.external_attr = 0o644 << 16
            archive.writestr(doc_info, doc_xhtml.encode("utf-8"))

        # 5. Image assets
        for href, (media_type, data) in sorted(images.items()):
            img_info = ZipInfo(f"EPUB/{href}", fixed_time)
            img_info.compress_type = ZIP_DEFLATED
            img_info.create_system = 0
            img_info.external_attr = 0o644 << 16
            archive.writestr(img_info, data)

        # 6. EPUB/package.opf
        manifest_entries = [
            '    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>'
        ]
        spine_entries = []
        for idx, (fname, _, _) in enumerate(documents, start=1):
            item_id = f"doc_{idx}"
            manifest_entries.append(
                f'    <item id="{item_id}" href="{escape(fname)}" media-type="application/xhtml+xml"/>'
            )
            spine_entries.append(f'    <itemref idref="{item_id}"/>')

        for idx, (href, (media_type, _)) in enumerate(sorted(images.items()), start=1):
            img_id = f"img_{idx}"
            manifest_entries.append(
                f'    <item id="{img_id}" href="{escape(href)}" media-type="{escape(media_type)}"/>'
            )

        manifest_str = "\n".join(manifest_entries)
        spine_str = "\n".join(spine_entries)

        package_opf = (
            '<?xml version="1.0" encoding="UTF-8"?>\n'
            '<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="pub-id">\n'
            '  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">\n'
            f'    <dc:identifier id="pub-id">{escape(identifier)}</dc:identifier>\n'
            f"    <dc:title>{escape(title)}</dc:title>\n"
            f"    <dc:language>{escape(language)}</dc:language>\n"
            '    <dc:creator>AriadShift contributors</dc:creator>\n'
            '    <meta property="dcterms:modified">2026-10-06T00:00:00Z</meta>\n'
            "  </metadata>\n"
            "  <manifest>\n"
            f"{manifest_str}\n"
            "  </manifest>\n"
            "  <spine>\n"
            f"{spine_str}\n"
            "  </spine>\n"
            "</package>\n"
        )
        opf_info = ZipInfo("EPUB/package.opf", fixed_time)
        opf_info.compress_type = ZIP_DEFLATED
        opf_info.create_system = 0
        opf_info.external_attr = 0o644 << 16
        archive.writestr(opf_info, package_opf.encode("utf-8"))

    return output.getvalue()


@dataclass
class EpubSpec:
    path: str
    doc_id: str
    identifier: str
    title: str
    lang: str
    blocks: list[Any]
    default_fname: str = "chapter1.xhtml"
    default_nav_title: str = ""


def _render_epub(spec: EpubSpec) -> bytes:
    """Render EPUB binary bytes directly from the spec's ordered block list."""
    documents: list[tuple[str, str, str]] = []
    current_fname = spec.default_fname
    current_nav_title = spec.default_nav_title or spec.title
    current_lines: list[str] = []
    images: dict[str, tuple[str, bytes]] = {}

    for block in spec.blocks:
        if isinstance(block, ChapterBreakBlock):
            if current_lines:
                documents.append((current_fname, current_nav_title, "\n".join(current_lines)))
                current_lines = []
            current_fname = block.file_name
            current_nav_title = block.nav_title
        elif isinstance(block, HeadingBlock):
            current_lines.append(f"  <h{block.level}>{escape(block.text)}</h{block.level}>")
        elif isinstance(block, ParagraphBlock):
            if block.footnote_ref:
                fn_num = block.footnote_ref.replace("fn", "")
                current_lines.append(
                    f'  <p>{escape(block.text)} <a epub:type="noteref" href="#{block.footnote_ref}">{fn_num}</a></p>'
                )
            else:
                current_lines.append(f"  <p>{escape(block.text)}</p>")
        elif isinstance(block, FootnoteBlock):
            fn_id = block.footnote_id or "fn1"
            current_lines.append(
                f'  <aside epub:type="footnote" id="{fn_id}">\n    <p>{escape(block.text)}</p>\n  </aside>'
            )
        elif isinstance(block, TableBlock):
            table_lines = ["  <table>"]
            if block.caption:
                table_lines.append(f"    <caption>{escape(block.caption)}</caption>")
            if block.rows:
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

                head_rows = [r for r in block.rows if any(c.is_header for c in r)]
                body_rows = [r for r in block.rows if not any(c.is_header for c in r)]
                if head_rows:
                    table_lines.append("    <thead>")
                    for row in head_rows:
                        row_html = "".join(_format_cell(c) for c in row)
                        table_lines.append(f"      <tr>{row_html}</tr>")
                    table_lines.append("    </thead>")
                if body_rows:
                    table_lines.append("    <tbody>")
                    for row in body_rows:
                        row_html = "".join(_format_cell(c) for c in row)
                        table_lines.append(f"      <tr>{row_html}</tr>")
                    table_lines.append("    </tbody>")
                elif not head_rows:
                    table_lines.append("    <tbody>")
                    for row in block.rows:
                        row_html = "".join(_format_cell(c) for c in row)
                        table_lines.append(f"      <tr>{row_html}</tr>")
                    table_lines.append("    </tbody>")
            table_lines.append("  </table>")
            current_lines.append("\n".join(table_lines))
        elif isinstance(block, FigureBlock):
            img_href = block.href or "images/route.png"
            fig_lines = [
                "  <figure>",
                f'    <img src="{img_href}" alt="{escape(block.alt)}" />',
                f"    <figcaption>{escape(block.caption or block.alt)}</figcaption>",
                "  </figure>",
            ]
            current_lines.append("\n".join(fig_lines))
            if block.asset_bytes:
                images[img_href] = (block.media_type, block.asset_bytes)

    if current_lines:
        documents.append((current_fname, current_nav_title, "\n".join(current_lines)))

    return _build_epub(spec.identifier, spec.title, spec.lang, documents, images=images)


def _get_specs(root: Path) -> list[EpubSpec]:
    garden = _source(root, "vietnamese-garden.md")
    river = _source(root, "vietnamese-river.md")
    map_room = _source(root, "english-map-room.md")
    route_png_path = root / "fixtures" / "md" / "assets" / "vietnamese-route.png"
    if route_png_path.is_file():
        route_png = route_png_path.read_bytes()
    else:
        from ariad_fixture_gen.markdown import _route_image
        route_png = _route_image()

    return [
        EpubSpec(
            path="fixtures/epub/vi-epub-chapters.epub",
            doc_id="vi-epub-chapters",
            identifier="urn:uuid:ariadshift-fixture-vi-epub-chapters",
            title="Tuyển tập đọc sách và hành trình",
            lang="vi",
            blocks=[
                ChapterBreakBlock("chapter1.xhtml", "Khu vườn đọc sách"),
                HeadingBlock(level=1, text="Khu vườn đọc sách"),
                ParagraphBlock(text=garden[0]),
                HeadingBlock(level=2, text="Sinh hoạt hằng tuần"),
                ParagraphBlock(text=garden[1]),
                ParagraphBlock(text=garden[2]),
                ChapterBreakBlock("chapter2.xhtml", "Hành trình ven sông"),
                HeadingBlock(level=1, text="Hành trình ven sông"),
                ParagraphBlock(text=river[0]),
                HeadingBlock(level=2, text="Đoạn đường men bến đá"),
                ParagraphBlock(text=river[1]),
                HeadingBlock(level=3, text="Ghi chép hằng ngày"),
                ParagraphBlock(text=river[2]),
                ChapterBreakBlock("chapter3.xhtml", "Không gian sinh hoạt chung"),
                HeadingBlock(level=1, text="Không gian sinh hoạt chung"),
                ParagraphBlock(text="Cả khu vườn và lối đi ven sông đều mở cửa cho mọi người ghé thăm bất kỳ lúc nào."),
                HeadingBlock(level=2, text="Gặp gỡ cuối tuần"),
                ParagraphBlock(text="Mỗi buổi chiều, người dân trong khu phố lại cùng nhau trò chuyện và chia sẻ những câu chuyện thường nhật."),
            ],
        ),
        EpubSpec(
            path="fixtures/epub/en-epub-footnotes.epub",
            doc_id="en-epub-footnotes",
            identifier="urn:uuid:ariadshift-fixture-en-epub-footnotes",
            title="Map archive notes with footnotes",
            lang="en",
            blocks=[
                ChapterBreakBlock("chapter1.xhtml", "Notes from the Map Room"),
                HeadingBlock(level=1, text="Notes from the Map Room"),
                ParagraphBlock(text=map_room[0], footnote_ref="fn1"),
                ParagraphBlock(text=map_room[1], footnote_ref="fn2"),
                FootnoteBlock(text="The archive keeps older map editions beside later revisions.", footnote_id="fn1"),
                FootnoteBlock(text="Catalog records are verified during regular review cycles.", footnote_id="fn2"),
            ],
        ),
        EpubSpec(
            path="fixtures/epub/vi-epub-table-image.epub",
            doc_id="vi-epub-table-image",
            identifier="urn:uuid:ariadshift-fixture-vi-epub-table-image",
            title="Bảng và hình ảnh hành trình ven sông",
            lang="vi",
            blocks=[
                ChapterBreakBlock("chapter1.xhtml", "Hành trình ven sông và sơ đồ"),
                HeadingBlock(level=1, text="Hành trình ven sông và sơ đồ"),
                ParagraphBlock(text=river[0]),
                TableBlock(
                    caption="Bảng hành trình ven sông",
                    rows=[
                        [Cell(text="Điểm dừng", is_header=True), Cell(text="Phút đi bộ", is_header=True), Cell(text="Ghi chú", is_header=True)],
                        [Cell(text="Bến sông"), Cell(text="8"), Cell(text="Bóng mát")],
                        [Cell(text="Vườn đọc"), Cell(text="14"), Cell(text="Có ghế dài")],
                        [Cell(text="Lịch khảo sát cập nhật mỗi tháng.", colspan=3)],
                    ],
                ),
                FigureBlock(
                    alt="Sơ đồ tuyến đường ven sông",
                    caption="Sơ đồ tuyến đường ven sông",
                    asset_id="6bca795730900c41e97ac33d04db2b054019dcd598e0e57f98dbc958e94291fb",
                    asset_bytes=route_png,
                    media_type="image/png",
                    href="images/route.png",
                ),
                ParagraphBlock(text=river[1]),
            ],
        ),
        EpubSpec(
            path="fixtures/epub/en-epub-minimal.epub",
            doc_id="en-epub-minimal",
            identifier="urn:uuid:ariadshift-fixture-en-epub-minimal",
            title="Minimal EPUB 3 document",
            lang="en",
            blocks=[
                ChapterBreakBlock("content.xhtml", "Minimal Document"),
                HeadingBlock(level=1, text="Minimal Document"),
                ParagraphBlock(text="This is a minimal valid EPUB 3 document for boundary testing."),
            ],
        ),
    ]


def generate(root: Path) -> dict[str, bytes]:
    """Generate all EPUB fixtures directly from their ordered block specifications."""
    specs = _get_specs(root)
    return {spec.path: _render_epub(spec) for spec in specs}


def generate_truth(root: Path) -> dict[str, dict[str, Any]]:
    """Build canonical truth companions directly from the ordered block specifications."""
    specs = _get_specs(root)
    truth_dict: dict[str, dict[str, Any]] = {}
    companion_name_map = {
        "vi-epub-chapters": "fixtures/epub/vi-epub-chapters.truth.json",
        "en-epub-footnotes": "fixtures/epub/en-epub-footnotes.truth.json",
        "vi-epub-table-image": "fixtures/epub/vi-epub-table-image.truth.json",
        "en-epub-minimal": "fixtures/epub/en-epub-minimal.truth.json",
    }
    for spec in specs:
        truth_path = companion_name_map[spec.doc_id]
        ir_doc = blocks_to_ir(spec.title, spec.lang, spec.blocks)
        truth_dict[truth_path] = blocks_to_canonical(spec.doc_id, spec.blocks, ir_doc=ir_doc)
    return truth_dict
