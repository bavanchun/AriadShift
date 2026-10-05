"""Generate deterministic Vietnamese page and English receipt PNG fixtures."""

from __future__ import annotations

from io import BytesIO
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

from ariad_fixture_gen.scan import _source, render_page

_MAX_FILE_BYTES = 2 * 1024 * 1024


def _png(image: Image.Image) -> bytes:
    output = BytesIO()
    image.save(output, format="PNG", optimize=True, compress_level=9)
    data = output.getvalue()
    if len(data) > _MAX_FILE_BYTES:
        raise ValueError(f"Generated image exceeds 2 MiB: {len(data)} bytes")
    return data


def _vietnamese_page(root: Path) -> bytes:
    paragraphs = _source(root, "vietnamese-garden.md")[:1]
    page = render_page(
        title="Khu vườn đọc sách",
        paragraphs=paragraphs,
        language="vi",
        ppi=180,
    )
    with Image.open(BytesIO(page)) as rendered:
        return _png(rendered.convert("RGB"))


def _receipt() -> bytes:
    image = Image.new("RGB", (1000, 1320), "#fffdf8")
    draw = ImageDraw.Draw(image)
    title = ImageFont.load_default(size=38)
    heading = ImageFont.load_default(size=26)
    body = ImageFont.load_default(size=22)
    small = ImageFont.load_default(size=18)

    ink = "#202c35"
    rule = "#87939a"
    draw.rectangle((44, 44, 956, 1276), outline=ink, width=4)
    draw.text((92, 88), "NEIGHBORHOOD REPAIR CAFE", font=title, fill=ink)
    draw.text((94, 150), "RECEIPT 0186  |  NO CHARGE", font=heading, fill=ink)
    draw.line((92, 208, 908, 208), fill=ink, width=3)

    draw.text((94, 248), "VISIT DETAILS", font=heading, fill=ink)
    draw.text((94, 302), "Date: 12 Apr 2026", font=body, fill=ink)
    draw.text((94, 348), "Item: Desk lamp", font=body, fill=ink)
    draw.text((94, 394), "Service: Cleaned the switch contact", font=body, fill=ink)
    draw.line((92, 454, 908, 454), fill=rule, width=2)

    draw.text((94, 496), "CHECK BEFORE RETURN", font=heading, fill=ink)
    checklist = (
        ("Lamp turns on", True),
        ("Cord is intact", True),
        ("Shade is secure", False),
    )
    for index, (label, checked) in enumerate(checklist):
        top = 554 + index * 56
        draw.rectangle((98, top, 128, top + 30), outline=ink, width=2)
        if checked:
            draw.line((104, top + 14, 114, top + 24), fill=ink, width=3)
            draw.line((114, top + 24, 124, top + 7), fill=ink, width=3)
        draw.text((150, top - 2), label, font=body, fill=ink)

    draw.line((92, 750, 908, 750), fill=rule, width=2)
    draw.text((94, 792), "PARTS AND NOTES", font=heading, fill=ink)
    draw.text((94, 850), "Parts used: None", font=body, fill=ink)
    draw.text((94, 900), "Next step: Replace the bulb if it fails", font=body, fill=ink)
    draw.line((94, 1012, 496, 1012), fill=rule, width=2)
    draw.line((548, 1012, 908, 1012), fill=rule, width=2)
    draw.text((94, 1028), "Volunteer", font=small, fill=ink)
    draw.text((548, 1028), "Visitor", font=small, fill=ink)

    draw.rounded_rectangle((648, 1114, 892, 1206), radius=14, outline="#2f765b", width=4)
    draw.text((685, 1144), "REPAIRED", font=heading, fill="#2f765b")
    return _png(image)


def generate(root: Path) -> dict[str, bytes]:
    return {
        "fixtures/image/vi-garden-page.png": _vietnamese_page(root),
        "fixtures/image/en-repair-receipt.png": _receipt(),
    }
