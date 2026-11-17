"""Generate a PDF whose pages draw rules that must not become tables, and one that must.

Page 1: a frame drawn as four segments around two columns of prose, with header, footer and
column rules. Page 2: the same page furniture with the frame stroked as one rectangle and 50 pt
margins. Page 3: a 3 by 3 table drawn sideways, under a quarter-turn matrix.
"""
from pathlib import Path

REGULAR = "F1"

LEFT = [
    "The harbour committee met on the first",
    "morning of the month to review the",
    "tide tables for the coming season.",
    "Members agreed that the northern pier",
    "needs new mooring rings before the",
    "autumn storms arrive in earnest.",
    "A survey of the lower slipway showed",
    "that the planks are sound but the",
    "iron fittings have rusted through.",
]
RIGHT = [
    "The treasurer reported a small surplus",
    "from the summer ferry fares, which the",
    "committee voted to set aside for the",
    "repair of the signal lamp on the point.",
    "Volunteers will paint the boathouse on",
    "the last two weekends of the month.",
    "The next meeting is planned for the",
    "week after the lamp has been tested",
    "and the mooring rings are delivered.",
]


def rule(x0, y0, x1, y1, width=0.5):
    return f"{width} w {x0} {y0} m {x1} {y1} l S\n"


def text(x, y, value, size=10):
    return f"BT /{REGULAR} {size} Tf {x} {y} Td ({value}) Tj ET\n"


def column(x, top, lines):
    return "".join(text(x, top - 14 * i, line) for i, line in enumerate(lines))


def furniture(left, bottom, right, top, header_y, footer_y, middle):
    """Header and footer text, and the two column blocks, inside a frame box."""
    out = text(left + 12, top - 18, "Harbour committee bulletin", 11)
    out += text(left + 12, bottom + 12, "Page 1")
    out += column(left + 12, header_y - 24, LEFT)
    out += column(middle + 12, header_y - 24, RIGHT)
    return out


def banded_rules(left, bottom, right, top, header_y, footer_y, middle):
    """Header rule, footer rule and the column rule between them."""
    return (
        rule(left, header_y, right, header_y)
        + rule(left, footer_y, right, footer_y)
        + rule(middle, footer_y, middle, header_y)
    )


# Page 1: the frame is four separate segments, 36 pt from the page edge.
L, B, R, T = 36, 36, 576, 756
page1 = rule(L, T, R, T, 1) + rule(L, B, R, B, 1) + rule(L, B, L, T, 1) + rule(R, B, R, T, 1)
page1 += banded_rules(L, B, R, T, 720, 72, 306)
page1 += furniture(L, B, R, T, 720, 72, 306)

# Page 2: one stroked rectangle, 50 pt from every page edge.
L, B, R, T = 50, 50, 562, 742
page2 = f"1 w {L} {B} {R - L} {T - B} re S\n"
page2 += banded_rules(L, B, R, T, 706, 86, 306)
page2 += furniture(L, B, R, T, 706, 86, 306)

# Page 3: a complete 3 by 3 grid with a letter in every cell, turned a quarter turn
# counter-clockwise by the matrix. Read upright it would be a table; on the page it is sideways.
XS = [100, 160, 220, 280]
YS = [100, 120, 140, 160]
page3 = "q 0 1 -1 0 612 0 cm\n"
page3 += "".join(rule(XS[0], y, XS[-1], y) for y in YS)
page3 += "".join(rule(x, YS[0], x, YS[-1]) for x in XS)
for r in range(3):
    for c in range(3):
        page3 += text(XS[c] + 6, YS[r] + 6, f"{'ABC'[r]}{c + 1}")
page3 += "Q\n"
page3 += text(72, 740, "The table on this page is drawn sideways.", 11)

content = [page1.encode("ascii"), page2.encode("ascii"), page3.encode("ascii")]
count = len(content)
objects = [
    b"<< /Type /Catalog /Pages 2 0 R >>",
    b"<< /Type /Pages /Kids [%s] /Count %d >>"
    % (" ".join(f"{3 + i} 0 R" for i in range(count)).encode(), count),
]
font_number = 3 + 2 * count
for index in range(count):
    objects.append(
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents %d 0 R "
        b"/Resources << /Font << /F1 %d 0 R >> >> >>" % (3 + count + index, font_number)
    )
for body in content:
    objects.append(b"<< /Length %d >>\nstream\n" % len(body) + body + b"endstream")
objects.append(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>")

out = bytearray(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n")
offsets = []
for number, body in enumerate(objects, 1):
    offsets.append(len(out))
    out.extend(b"%d 0 obj\n" % number + body + b"\nendobj\n")
xref = len(out)
out.extend(b"xref\n0 %d\n0000000000 65535 f \n" % (len(objects) + 1))
for offset in offsets:
    out.extend(b"%010d 00000 n \n" % offset)
out.extend(b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (len(objects) + 1, xref))
Path(__file__).resolve().parents[1].joinpath("pdf/ruled-table-traps.pdf").write_bytes(out)
