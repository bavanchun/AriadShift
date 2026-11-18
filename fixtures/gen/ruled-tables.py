"""Generate a two-page PDF whose tables are drawn with ruling lines.

Page 1 has prose, a table whose first row sits above a heavier rule, and a table with merged
cells and a bold first row. Page 2 has shapes that must not become tables: a grid whose merged
cells are not rectangles, and a framed box without inner rules.
"""
from pathlib import Path

REGULAR, BOLD = "F1", "F2"


def rule(x0, y0, x1, y1, width=0.5):
    return f"{width} w {x0} {y0} m {x1} {y1} l S\n"


def text(x, y, value, font=REGULAR, size=10):
    return f"BT /{font} {size} Tf {x} {y} Td ({value}) Tj ET\n"


def grid(xs, ys, skip=(), widths=None):
    """Horizontal and vertical rules over the full grid, except the `skip` pairs.

    A skipped entry is `("h", row, column)` or `("v", row, column)`: the horizontal edge above
    cell `(row, column)`, or the vertical edge left of it.
    """
    widths = widths or {}
    out = ""
    for r, y in enumerate(ys):
        for c in range(len(xs) - 1):
            if ("h", r, c) not in skip:
                out += rule(xs[c], y, xs[c + 1], y, widths.get(r, 0.5))
    for c, x in enumerate(xs):
        for r in range(len(ys) - 1):
            if ("v", r, c) not in skip:
                out += rule(x, ys[r], x, ys[r + 1])
    return out


def cell(xs, ys, row, column, value, font=REGULAR, lines=None):
    """Text near the left edge of a cell, on the baseline of its bottom line."""
    x = xs[column] + 6
    y = ys[row + 1] + 7
    if lines:
        return "".join(text(x, y + 12 * (len(lines) - 1 - i), line, font) for i, line in enumerate(lines))
    return text(x, y, value, font)


# Page 1.
XS = [72, 192, 312, 432]
page1 = text(72, 748, "Quarterly figures are listed in the tables below.", size=11)
page1 += text(72, 734, "Each table is ruled on every side.", size=11)

# Table A: a heavier rule under the first row marks the head. The Central row is two lines tall.
ys_a = [700, 678, 656, 626, 604]
page1 += grid(XS, ys_a, widths={1: 2.0})
for column, value in enumerate(["Region", "Units", "Revenue"]):
    page1 += cell(XS, ys_a, 0, column, value)
for row, values in enumerate([["North", "120", "4,500"], ["Central region", "95", "3,210"], ["East", "143", "5,008"]], 1):
    for column, value in enumerate(values):
        if row == 2 and column == 0:
            page1 += cell(XS, ys_a, row, column, None, lines=["Central", "region"])
        else:
            page1 += cell(XS, ys_a, row, column, value)

page1 += text(72, 580, "Merged cells follow.", size=11)

# Table B: bold first row; "Plan" spans two columns and "Team A" spans two rows.
ys_b = [560, 538, 516, 494]
skip_b = {("v", 0, 1), ("h", 2, 0)}
page1 += grid(XS, ys_b, skip=skip_b)
page1 += cell(XS, ys_b, 0, 0, "Plan", BOLD) + cell(XS, ys_b, 0, 2, "Actual", BOLD)
page1 += cell(XS, ys_b, 2, 0, "Team A")
page1 += cell(XS, ys_b, 1, 1, "10") + cell(XS, ys_b, 1, 2, "12")
page1 += cell(XS, ys_b, 2, 1, "11") + cell(XS, ys_b, 2, 2, "13")

# Page 2.
page2 = text(72, 748, "Shapes that are not tables.", size=11)

# Three cells join into an L: the rule under the top-middle cell and the rule between the two
# right-hand cells of the middle row are both missing.
ys_c = [700, 678, 656, 634]
skip_c = {("h", 1, 1), ("v", 1, 2)}
page2 += grid(XS, ys_c, skip=skip_c)
for r in range(3):
    for c in range(3):
        page2 += cell(XS, ys_c, r, c, f"L{r}{c}")

# A framed note: one rectangle, no inner rules.
page2 += "1 w 72 520 360 80 re S\n"
page2 += text(84, 575, "Note: figures are provisional.")
page2 += text(84, 560, "Final numbers follow in the annual report.")

content = [page1.encode("ascii"), page2.encode("ascii")]
objects = [
    b"<< /Type /Catalog /Pages 2 0 R >>",
    b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>",
]
for index in range(2):
    objects.append(
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents %d 0 R "
        b"/Resources << /Font << /F1 7 0 R /F2 8 0 R >> >> >>" % (5 + index)
    )
for body in content:
    objects.append(b"<< /Length %d >>\nstream\n" % len(body) + body + b"endstream")
objects.append(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>")
objects.append(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>")

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
Path(__file__).resolve().parents[1].joinpath("pdf/ruled-tables.pdf").write_bytes(out)
