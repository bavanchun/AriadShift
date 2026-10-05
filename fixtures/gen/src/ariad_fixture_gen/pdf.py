"""Generate deterministic digital PDF fixtures with Typst."""

from __future__ import annotations

from pathlib import Path

import typst


def _typst_string(value: str) -> str:
    return '"' + value.replace("\\", "\\\\").replace('"', '\\"') + '"'


def _content(value: str) -> str:
    return f"#text({_typst_string(value)})"


def _document(
    *,
    title: str,
    language: str,
    paragraphs: list[str],
    table: list[tuple[str, str]],
    footnote: str | None = None,
) -> bytes:
    source = [
        f"#set document(title: {_typst_string(title)}, author: \"AriadShift contributors\", date: none)",
        '#set page(paper: "a4", margin: 22mm)',
        f'#set text(font: "Libertinus Serif", size: 11pt, lang: "{language}")',
        "#set par(justify: true, leading: 0.72em)",
        "#set heading(numbering: none)",
        "#align(center)[#text(size: 18pt, weight: \"bold\")[" + _content(title) + "]]",
        "#v(5mm)",
    ]
    source.extend(_content(paragraph) + "\n#parbreak()" for paragraph in paragraphs)
    if footnote is not None:
        source.extend(("#v(2mm)", _content("Archive note") + f"#footnote[{_content(footnote)}]"))
    source.extend(("#v(3mm)", "#table(columns: (1fr, 0.7fr), inset: 5pt, stroke: 0.5pt + gray,"))
    source.extend(f"  [{_content(label)}], [{_content(value)}]," for label, value in table)
    source.append(")")
    return typst.compile("\n".join(source).encode("utf-8"), format="pdf", timestamp=1_700_000_000)


def _source(root: Path, filename: str) -> list[str]:
    path = root / "fixtures" / "gen" / "src" / "ariad_fixture_gen" / "text" / filename
    return [paragraph.strip() for paragraph in path.read_text(encoding="utf-8").split("\n\n") if paragraph.strip()]


def generate(root: Path) -> dict[str, bytes]:
    garden = _source(root, "vietnamese-garden.md")
    map_room = _source(root, "english-map-room.md")
    repair_cafe = _source(root, "english-repair-cafe.md")

    return {
        "fixtures/pdf/vi-garden-route.pdf": _document(
            title="Khu vườn đọc sách",
            language="vi",
            paragraphs=garden,
            table=[
                ("Điểm dừng", "Phút đi bộ"),
                ("Bến sông", "8"),
                ("Vườn đọc", "14"),
                ("Ghi chú", "Ước lượng trong ngày nắng"),
            ],
        ),
        "fixtures/pdf/en-map-room.pdf": _document(
            title="Notes from the Map Room",
            language="en",
            paragraphs=map_room + repair_cafe[:1],
            table=[
                ("Archive item", "Recorded detail"),
                ("Street plan", "Survey date"),
                ("Overlay", "Transparent sheet"),
                ("Map weight", "Brass"),
            ],
            footnote="The archive keeps older map editions beside later revisions.",
        ),
    }
