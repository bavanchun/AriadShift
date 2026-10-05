"""Generate small semantic HTML fixtures from original source text."""

from __future__ import annotations

from html import escape
from pathlib import Path


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


def generate(root: Path) -> dict[str, bytes]:
    garden = _source(root, "vietnamese-garden.md")
    river = _source(root, "vietnamese-river.md")
    repair_cafe = _source(root, "english-repair-cafe.md")
    map_room = _source(root, "english-map-room.md")

    return {
        "fixtures/html/vi-garden-notice.html": _document(
            "vi",
            "Lịch chăm vườn đọc",
            "<main>\n"
            "  <h1>Lịch chăm vườn đọc</h1>\n"
            f"  <p>{escape(garden[0])}</p>\n"
            f"  <p>{escape(garden[1])}</p>\n"
            "</main>",
        ),
        "fixtures/html/vi-route-table.html": _document(
            "vi",
            "Các điểm dừng ven sông",
            "<article>\n"
            "  <h1>Các điểm dừng ven sông</h1>\n"
            f"  <p>{escape(river[0])}</p>\n"
            "  <table>\n"
            "    <caption>Thời gian đi bộ</caption>\n"
            "    <thead><tr><th scope=\"col\">Điểm dừng</th><th scope=\"col\">Phút</th></tr></thead>\n"
            "    <tbody><tr><td>Bến sông</td><td>8</td></tr><tr><td>Vườn đọc</td><td>14</td></tr></tbody>\n"
            "  </table>\n"
            "</article>",
        ),
        "fixtures/html/en-repair-cafe.html": _document(
            "en",
            "The Repair Café",
            "<main>\n"
            "  <h1>The Repair Café</h1>\n"
            f"  <p>{escape(repair_cafe[0])}</p>\n"
            f"  <p>{escape(repair_cafe[1])}</p>\n"
            f"  <blockquote><p>{escape(repair_cafe[2].split('. ')[0] + '.')}</p></blockquote>\n"
            "</main>",
        ),
        "fixtures/html/en-map-archive.html": _document(
            "en",
            "Notes from the Map Room",
            "<article>\n"
            "  <h1>Notes from the Map Room</h1>\n"
            f"  <p>{escape(map_room[0])}</p>\n"
            f"  <p>{escape(map_room[1])}</p>\n"
            '  <a href="https://example.org/map-index">Browse the map index</a>\n'
            "</article>",
        ),
    }
