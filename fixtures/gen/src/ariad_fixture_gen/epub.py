"""Generate reproducible EPUB 3 fixtures with structural edge cases."""

from __future__ import annotations

from html import escape
from io import BytesIO
from pathlib import Path
from zipfile import ZIP_DEFLATED, ZIP_STORED, ZipFile, ZipInfo


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
            "  <meta charset=\"utf-8\" />\n"
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
                "  <meta charset=\"utf-8\" />\n"
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


def _vi_chapters(garden: list[str], river: list[str]) -> bytes:
    documents = [
        (
            "chapter1.xhtml",
            "Khu vườn đọc sách",
            f"  <h1>Khu vườn đọc sách</h1>\n"
            f"  <p>{escape(garden[0])}</p>\n"
            f"  <h2>Sinh hoạt hằng tuần</h2>\n"
            f"  <p>{escape(garden[1])}</p>\n"
            f"  <p>{escape(garden[2])}</p>",
        ),
        (
            "chapter2.xhtml",
            "Hành trình ven sông",
            f"  <h1>Hành trình ven sông</h1>\n"
            f"  <p>{escape(river[0])}</p>\n"
            f"  <h2>Đoạn đường men bến đá</h2>\n"
            f"  <p>{escape(river[1])}</p>\n"
            f"  <h3>Ghi chép hằng ngày</h3>\n"
            f"  <p>{escape(river[2])}</p>",
        ),
        (
            "chapter3.xhtml",
            "Không gian sinh hoạt chung",
            "  <h1>Không gian sinh hoạt chung</h1>\n"
            "  <p>Cả khu vườn và lối đi ven sông đều mở cửa cho mọi người ghé thăm bất kỳ lúc nào.</p>\n"
            "  <h2>Gặp gỡ cuối tuần</h2>\n"
            "  <p>Mỗi buổi chiều, người dân trong khu phố lại cùng nhau trò chuyện và chia sẻ những câu chuyện thường nhật.</p>",
        ),
    ]
    return _build_epub(
        "urn:uuid:ariadshift-fixture-vi-epub-chapters",
        "Tuyển tập đọc sách và hành trình",
        "vi",
        documents,
    )


def _en_footnotes(map_room: list[str]) -> bytes:
    body = (
        "  <h1>Notes from the Map Room</h1>\n"
        f'  <p>{escape(map_room[0])} <a epub:type="noteref" href="#fn1">1</a></p>\n'
        f'  <p>{escape(map_room[1])} <a epub:type="noteref" href="#fn2">2</a></p>\n'
        '  <aside epub:type="footnote" id="fn1">\n'
        "    <p>The archive keeps older map editions beside later revisions.</p>\n"
        "  </aside>\n"
        '  <aside epub:type="footnote" id="fn2">\n'
        "    <p>Catalog records are verified during regular review cycles.</p>\n"
        "  </aside>"
    )
    documents = [
        ("chapter1.xhtml", "Notes from the Map Room", body),
    ]
    return _build_epub(
        "urn:uuid:ariadshift-fixture-en-epub-footnotes",
        "Map archive notes with footnotes",
        "en",
        documents,
    )


def _vi_table_image(river: list[str], image_data: bytes) -> bytes:
    table_html = (
        "  <table>\n"
        "    <caption>Bảng hành trình ven sông</caption>\n"
        "    <thead>\n"
        '      <tr><th scope="col">Điểm dừng</th><th scope="col">Phút đi bộ</th><th scope="col">Ghi chú</th></tr>\n'
        "    </thead>\n"
        "    <tbody>\n"
        "      <tr><td>Bến sông</td><td>8</td><td>Bóng mát</td></tr>\n"
        "      <tr><td>Vườn đọc</td><td>14</td><td>Có ghế dài</td></tr>\n"
        '      <tr><td colspan="3">Lịch khảo sát cập nhật mỗi tháng.</td></tr>\n'
        "    </tbody>\n"
        "  </table>"
    )
    body = (
        "  <h1>Hành trình ven sông và sơ đồ</h1>\n"
        f"  <p>{escape(river[0])}</p>\n"
        f"{table_html}\n"
        "  <figure>\n"
        '    <img src="images/route.png" alt="Sơ đồ tuyến đường ven sông" />\n'
        "    <figcaption>Sơ đồ tuyến đường ven sông</figcaption>\n"
        "  </figure>\n"
        f"  <p>{escape(river[1])}</p>"
    )
    documents = [
        ("chapter1.xhtml", "Hành trình ven sông và sơ đồ", body),
    ]
    images = {
        "images/route.png": ("image/png", image_data),
    }
    return _build_epub(
        "urn:uuid:ariadshift-fixture-vi-epub-table-image",
        "Bảng và hình ảnh hành trình ven sông",
        "vi",
        documents,
        images=images,
    )


def _en_minimal() -> bytes:
    body = (
        "  <h1>Minimal Document</h1>\n"
        "  <p>This is a minimal valid EPUB 3 document for boundary testing.</p>"
    )
    documents = [
        ("content.xhtml", "Minimal Document", body),
    ]
    return _build_epub(
        "urn:uuid:ariadshift-fixture-en-epub-minimal",
        "Minimal EPUB 3 document",
        "en",
        documents,
    )


def generate(root: Path) -> dict[str, bytes]:
    garden = _source(root, "vietnamese-garden.md")
    river = _source(root, "vietnamese-river.md")
    map_room = _source(root, "english-map-room.md")
    route_png = (root / "fixtures" / "md" / "assets" / "vietnamese-route.png").read_bytes()

    return {
        "fixtures/epub/vi-epub-chapters.epub": _vi_chapters(garden, river),
        "fixtures/epub/en-epub-footnotes.epub": _en_footnotes(map_room),
        "fixtures/epub/vi-epub-table-image.epub": _vi_table_image(river, route_png),
        "fixtures/epub/en-epub-minimal.epub": _en_minimal(),
    }
