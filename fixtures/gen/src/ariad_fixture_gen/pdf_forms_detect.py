"""Generate deterministic synthetic field-detection evaluation documents."""

from __future__ import annotations

from pathlib import Path

from pikepdf import Array, Dictionary, Name, String

from .manifest import update_generated_hashes
from .pdf_tools import (
    _default_page_info,
    _new_pdf,
    _save_deterministic,
    _truth_envelope,
)


def _case(stem: str) -> tuple[bytes, bytes]:
    title = f"Synthetic form detection: {stem}"
    pdf, resources = _new_pdf(title, language="vi" if stem == "vietnamese" else "en")
    page = pdf.add_blank_page(page_size=(595.28, 841.89))
    page.obj[Name.Resources] = resources
    page.obj[Name.Annots] = Array()
    expected: list[dict] = []
    ops: list[str] = ["q 0 G 0 g 1 w"]

    def label(text: str, x: float, y: float) -> None:
        # ASCII cases use a standard 14 font; the Vietnamese case installs a ToUnicode map below.
        encoded = "".join(f"{char_map.get(ch, ord(ch)):02X}" for ch in text)
        ops.append(f"BT /F1 12 Tf 1 0 0 1 {x} {y} Tm <{encoded}> Tj ET")

    char_map: dict[str, int] = {}
    if stem == "vietnamese":
        chars = sorted({c for c in "Họ tên Chữ ký" if ord(c) > 127})
        char_map = {ch: 128 + index for index, ch in enumerate(chars)}
        mappings = [(i, chr(i)) for i in range(32, 127)] + [
            (value, key) for key, value in char_map.items()
        ]
        cmap = [
            "/CIDInit /ProcSet findresource begin",
            "12 dict begin",
            "begincmap",
            "/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def",
            "/CMapName /AriadShiftUnicode def",
            "/CMapType 2 def",
            "1 begincodespacerange",
            "<00> <FF>",
            "endcodespacerange",
            f"{len(mappings)} beginbfchar",
        ]
        cmap.extend(f"<{code:02X}> <{ord(ch):04X}>" for code, ch in mappings)
        cmap.extend(
            [
                "endbfchar",
                "endcmap",
                "CMapName currentdict /CMap defineresource pop",
                "end",
                "end",
            ]
        )
        font = resources[Name.Font][Name.F1]
        font[Name.ToUnicode] = pdf.make_stream("\n".join(cmap).encode("ascii"))
        differences = Array()
        for ch, code in char_map.items():
            differences.extend([code, Name(f"/uni{ord(ch):04X}")])
        font[Name.Encoding] = Dictionary(
            Type=Name.Encoding,
            BaseEncoding=Name.WinAnsiEncoding,
            Differences=differences,
        )

    def line(name: str, title_text: str, x: int, y: int, width: int = 180) -> None:
        label(title_text, x - 105, y + 2)
        ops.append(f"{x} {y} m {x + width} {y} l S")
        expected.append(
            {
                "name": name,
                "type": "signature" if "signature" in name or "ky" in name else "text",
                "rect": [x, y, x + width, y + 14.4],
            }
        )

    def box(
        name: str,
        title_text: str,
        x: int,
        y: int,
        width: int,
        height: int,
        kind: str = "text",
    ) -> None:
        label(title_text, x - 105, y + 3)
        ops.append(f"{x} {y} {width} {height} re S")
        expected.append(
            {
                "name": name,
                "type": kind,
                "rect": [x + 1, y + 1, x + width - 1, y + height - 1],
            }
        )

    if stem == "underlines":
        for name, text, y in [
            ("name", "Name", 700),
            ("email", "Email", 650),
            ("signature", "Signature", 600),
        ]:
            line(name, text, 200, y)
        label("Nickname", 95, 552)
        label("__________", 200, 550)
        expected.append(
            {"name": "nickname", "type": "text", "rect": [200, 548, 267, 562.4]}
        )
    elif stem == "boxes":
        for name, text, y in [
            ("name", "Name", 700),
            ("address", "Address", 650),
            ("city", "City", 600),
        ]:
            box(name, text, 200, y, 180, 24)
        label("Phone", 95, 503)
        ops.extend(
            [
                "200 500 m 380 500 l S",
                "200 524 m 380 524 l S",
                "200 500 m 200 524 l S",
                "380 500 m 380 524 l S",
            ]
        )
        expected.append({"name": "phone", "type": "text", "rect": [201, 501, 379, 523]})
        label("Occupied", 95, 553)
        ops.append("200 550 180 24 re S")
        label("filled", 205, 557)
    elif stem == "checkboxes":
        for name, text, y in [
            ("yes", "Yes", 700),
            ("no", "No", 660),
            ("maybe", "Maybe", 620),
        ]:
            label(text, 225, y + 3)
            ops.append(f"200 {y} 14 14 re S")
            expected.append(
                {"name": name, "type": "checkbox", "rect": [201, y + 1, 213, y + 13]}
            )
        ops.append("200 580 8 8 re f")
        label("Bullet", 225, 582)
    elif stem == "table":
        for row, text in enumerate(["Item", "Count", "Price"]):
            y = 700 - 30 * row
            label(text, 95, y + 7)
            for col in range(2):
                x = 200 + col * 120
                ops.append(f"{x} {y} 120 30 re S")
                expected.append(
                    {
                        "name": text.lower()
                        if col == 0
                        else f"{text.lower()}_{col + 1}",
                        "type": "text",
                        "rect": [x + 1, y + 1, x + 119, y + 29],
                    }
                )
    elif stem == "vietnamese":
        line("ho_ten", "Họ tên", 200, 700)
        line("chu_ky", "Chữ ký", 200, 650)
    elif stem == "existing-widgets":
        line("name", "Name", 200, 700)
        line("email", "Email", 200, 650)
        widget = pdf.make_indirect(
            Dictionary(
                Type=Name.Annot,
                Subtype=Name.Widget,
                FT=Name.Tx,
                T=String("existing"),
                Rect=Array([200, 700, 380, 715]),
                P=page.obj,
            )
        )
        page.obj[Name.Annots].append(widget)
        pdf.Root[Name.AcroForm] = pdf.make_indirect(Dictionary(Fields=Array([widget])))
        expected = expected[1:]
    elif stem == "rotated":
        page.obj[Name.Rotate] = 90
        page.obj[Name.CropBox] = Array([20, 30, 575, 812])
        line("name", "Name", 200, 700)
        box("email", "Email", 200, 650, 180, 24)
    elif stem == "prose":
        label("This is ordinary prose with no form fields.", 72, 700)
        label("A second sentence contains no empty boxes.", 72, 675)
        ops.append("BT /F1 12 Tf 3 Tr 1 0 0 1 72 650 Tm (Hidden ______) Tj ET")
        glyph = pdf.make_stream(b"500 0 d0 80 0 340 650 re f")
        resources[Name.Font][Name.F3] = pdf.make_indirect(
            Dictionary(
                Type=Name.Font,
                Subtype=Name.Type3,
                FontBBox=Array([0, 0, 500, 700]),
                FontMatrix=Array([0.001, 0, 0, 0.001, 0, 0]),
                CharProcs=Dictionary(A=glyph),
                Encoding=Dictionary(
                    Type=Name.Encoding, Differences=Array([65, Name.A])
                ),
                FirstChar=65,
                LastChar=65,
                Widths=Array([500]),
                Resources=Dictionary(),
            )
        )
        ops.append("BT /F3 12 Tf 1 0 0 1 72 625 Tm (AAA) Tj ET")
    else:
        raise ValueError(stem)
    ops.append("Q")
    page.obj[Name.Contents] = pdf.make_stream(("\n".join(ops) + "\n").encode("ascii"))
    truth = _truth_envelope(
        doc_id=f"detect-{stem}",
        title=title,
        language="vi" if stem == "vietnamese" else "en",
        page_count=1,
        pages=[_default_page_info(1, rotation=90 if stem == "rotated" else 0)],
        has_forms=stem == "existing-widgets",
        extra={"detected_fields": expected},
    )
    return _save_deterministic(pdf), truth


def generate() -> dict[str, bytes]:
    outputs: dict[str, bytes] = {}
    for stem in [
        "underlines",
        "boxes",
        "checkboxes",
        "table",
        "vietnamese",
        "existing-widgets",
        "rotated",
        "prose",
    ]:
        pdf, truth = _case(stem)
        outputs[f"fixtures/pdf-forms-detect/{stem}.pdf"] = pdf
        outputs[f"fixtures/pdf-forms-detect/{stem}.truth.json"] = truth
    if sum(map(len, outputs.values())) > 50_000:
        raise ValueError("detection corpus exceeds 50 KB")
    return outputs


def main() -> None:
    root = Path(__file__).resolve().parents[4]
    outputs = generate()
    for relative, content in sorted(outputs.items()):
        path = root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        if not path.exists() or path.read_bytes() != content:
            path.write_bytes(content)
    update_generated_hashes(root, set(outputs))
    print(
        f"Generated {len(outputs)} detection files ({sum(map(len, outputs.values()))} bytes)."
    )


if __name__ == "__main__":
    main()
