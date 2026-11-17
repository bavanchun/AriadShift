"""Generate PDFs for whitespace-aligned table detection.

`unruled-tables.pdf` holds tables that have no ruling lines, or only booktabs-style horizontal
rules, in a single-column page and beside prose in one column of a two-column page.

`unruled-negatives.pdf` has one page per layout that looks tabular but is not a table, or is too
uncertain to call one: multi-column prose, nested lists, poetry, code, tables of contents, running
headers, justified text with wide gaps, grids that are too small, drifting columns, running text
in aligned cells, and a table that spans both columns of a page. A reader must find no table on
any of them.
"""
from pathlib import Path

REGULAR, BOLD, MONO = "F1", "F2", "F3"

# Helvetica advance widths in 1/1000 em for the printable ASCII range.
HELVETICA = dict(zip(
    map(chr, range(32, 127)),
    [278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278]
    + [556] * 10
    + [278, 278, 584, 584, 584, 556, 1015]
    + [667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778,
       667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611]
    + [278, 278, 278, 469, 556, 333]
    + [556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556,
       556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500]
    + [334, 260, 334, 584],
))

PROSE = (
    "The committee reviewed the proposal and noted that every district would receive a share of "
    "the funds before the end of the year. Members asked for a clearer account of how the "
    "figures were produced, and the secretary promised a short appendix with the sources. "
    "Several speakers pointed out that earlier surveys had covered fewer villages, so the new "
    "totals could not be compared directly with those of previous seasons. The chair closed the "
    "discussion by asking each group to send written comments within two weeks, after which "
    "the revised text would be circulated again. A small working party will check the maps "
    "against the registers, and the treasurer will report on the costs at the next meeting."
)
SENTENCES = [part + "." for part in PROSE.rstrip(".").split(". ")]


def sentences(start, stop=None):
    return " ".join(SENTENCES[start:stop])


def width(value, font=REGULAR, size=10):
    if font == MONO:
        return 0.6 * size * len(value)
    return sum(HELVETICA.get(ch, 556) for ch in value) * size / 1000


def escape(value):
    return value.replace("\\", "\\\\").replace("(", "\\(").replace(")", "\\)")


class Page:
    def __init__(self):
        self.ops = []

    def text(self, x, y, value, font=REGULAR, size=10, spacing=0.0):
        word = f"{spacing:.3f} Tw " if spacing else ""
        self.ops.append(f"BT /{font} {size} Tf {word}{x:.2f} {y:.2f} Td ({escape(value)}) Tj ET\n")

    def rule(self, x0, y0, x1, y1, line=0.5):
        self.ops.append(f"{line} w {x0} {y0} m {x1} {y1} l S\n")

    def cell(self, left, cell_width, y, value, align="l", font=REGULAR, size=10):
        size_width = width(value, font, size)
        x = {"l": left, "r": left + cell_width - size_width, "c": left + (cell_width - size_width) / 2}[align]
        self.text(x, y, value, font, size)

    def encode(self):
        return "".join(self.ops).encode("ascii")


def wrap(value, limit, font=REGULAR, size=10):
    lines, current = [], ""
    for word in value.split():
        trial = f"{current} {word}".strip()
        if current and width(trial, font, size) > limit:
            lines.append(current)
            current = word
        else:
            current = trial
    return lines + [current] if current else lines


def paragraph(page, x, y, limit, value, size=10, leading=None, justify=True, font=REGULAR):
    """Greedy-wrapped text, justified with word spacing; returns the y below the last line."""
    leading = leading or size * 1.25
    lines = wrap(value, limit, font, size)
    for index, line in enumerate(lines):
        gaps = line.count(" ")
        spacing = 0.0
        if justify and index < len(lines) - 1 and gaps:
            spacing = (limit - width(line, font, size)) / gaps
        page.text(x, y, line, font, size, spacing)
        y -= leading
    return y


def table(page, columns, y, rows, pitch=14, size=10, bold_first=False):
    """Rows of cells at `columns` = [(left, width, align)], first baseline `y`; returns the y after."""
    for index, row in enumerate(rows):
        font = BOLD if bold_first and index == 0 else REGULAR
        for (left, cell_width, align), value in zip(columns, row):
            if value:
                page.cell(left, cell_width, y, value, align, font, size)
        y -= pitch
    return y


WIDE_WORDS = [
    "a", "to", "of", "in", "we", "and", "the", "but", "with", "that", "from", "into", "than",
    "because", "between", "through", "everything", "remarkable", "nevertheless", "international",
]


def narrow_lines(count, limit):
    """Lines of three or four words that fill `limit` points when justified.

    The words come from a small linear congruential sequence, so the file is the same everywhere.
    """
    state, lines = 12345, []
    while len(lines) < count:
        words = []
        for _ in range(3 + len(lines) % 2):
            state = (state * 1103515245 + 12345) % 2**31
            words.append(WIDE_WORDS[(state >> 8) % len(WIDE_WORDS)])
        if width(" ".join(words)) < limit * 0.75:
            lines.append(" ".join(words))
    return lines


# Positive fixture --------------------------------------------------------------------------

def positives():
    one = Page()
    one.text(72, 740, "Measurement summary", BOLD, 14)
    y = paragraph(one, 72, 716, 468, sentences(0, 2), 11)
    columns = [(72, 150, "l"), (230, 70, "r"), (310, 70, "r"), (400, 60, "l")]
    # Booktabs style: horizontal rules only, bold head, no vertical rules.
    top = y - 6
    one.rule(72, top, 460, top, 1.0)
    y = table(one, columns, top - 16, [
        ["Instrument", "Range", "Error", "Unit"],
        ["Caliper", "150", "0.02", "mm"],
        ["Micrometer", "25", "0.004", "mm"],
        ["Dial gauge", "10", "0.01", "mm"],
        ["Height gauge", "300", "0.03", "mm"],
        ["Depth rod", "200", "0.05", "mm"],
    ], bold_first=True)
    one.rule(72, top - 20, 460, top - 20, 0.5)
    one.rule(72, y + 4, 460, y + 4, 1.0)
    one.text(72, y - 12, "Table 1: Instruments and their stated error.", REGULAR, 10)
    y = paragraph(one, 72, y - 40, 468, sentences(2, 5), 11)
    # No rules at all, with a missing cell in two rows.
    columns = [(72, 90, "l"), (180, 60, "r"), (260, 60, "r"), (340, 60, "r")]
    table(one, columns, y - 10, [
        ["Region", "Spring", "Summer", "Autumn"],
        ["North", "12", "18", ""],
        ["South", "20", "25", "14"],
        ["East", "9", "", "11"],
        ["West", "15", "22", "13"],
    ], pitch=16)
    one.text(72, y - 100, "Table 2: Rainfall by season, in millimetres.", REGULAR, 10)

    two = Page()
    two.text(72, 740, "Two columns", BOLD, 14)
    paragraph(two, 72, 712, 216, sentences(0) + " " + sentences(0), 9)
    right = paragraph(two, 324, 712, 216, sentences(0, 3), 9)
    # A spanning label over two columns is too uncertain to join the table and stays text.
    two.cell(450, 100, right - 6, "Throughput", "c", REGULAR, 9)
    columns = [(324, 60, "l"), (390, 40, "r"), (440, 40, "r"), (490, 50, "r")]
    y = table(two, columns, right - 20, [
        ["Name", "Size", "Speed", "Score"],
        ["Alpha", "12", "340", "0.91"],
        ["Beta", "48", "210", "0.87"],
        ["Gamma", "96", "120", "0.95"],
        ["Delta", "7", "410", "0.78"],
    ], pitch=13, size=9, bold_first=True)
    two.text(324, y - 6, "Table 3: Model comparison.", REGULAR, 9)
    paragraph(two, 324, y - 30, 216, sentences(3), 9)
    return [one, two]


# Negative fixture --------------------------------------------------------------------------

def three_column_prose():
    page = Page()
    page.text(72, 740, "Three columns", BOLD, 14)
    text = " ".join([PROSE] * 6)
    for column in range(3):
        offset = column * 60
        paragraph(page, 72 + column * 168, 712, 150, text[offset:], 9)
    return page


def lists():
    page = Page()
    page.text(72, 740, "Lists", BOLD, 14)
    y = 712
    items = [
        (0, "1.", "First point, which continues over a second line to show a hanging indent"),
        (1, "(a)", "A nested point with a similar amount of text so that it wraps as well"),
        (2, "(i)", "A deeper point"),
        (2, "(ii)", "Another deeper point with enough words to wrap onto the next line"),
        (1, "(b)", "Back to the middle level"),
        (0, "2.", "Second point"),
    ]
    for level, marker, value in items:
        x = 72 + 28 * level
        page.text(x, y, marker, REGULAR, 10)
        for line in wrap(value, 340 - 28 * level, REGULAR, 10):
            page.text(x + 30, y, line, REGULAR, 10)
            y -= 13
        y -= 4
    y -= 20
    for term, meaning in [
        ("Aperture", "the opening that lets light reach the sensor"),
        ("Exposure", "the amount of light collected during one capture"),
        ("Focal length", "the distance between lens and sensor when focused"),
        ("Shutter", "the part that controls how long light is admitted"),
    ]:
        page.text(72, y, term, BOLD, 10)
        page.text(180, y, meaning, REGULAR, 10)
        y -= 16
    return page


def poetry():
    page = Page()
    page.text(72, 740, "Poem", BOLD, 14)
    lines = [
        "The river keeps its own slow hours", "and carries what the hills let go;",
        "   a leaf, a feather, quiet flowers,", "   the small blue sky it learned to show.",
        "We watched it from the bridge at noon", "and counted ripples one by one,",
        "   until the swallows came back soon", "   and every count was lost in sun.",
        "So let the water have its way,", "and let the evening carry on;",
    ]
    y = 700
    for index, line in enumerate(lines, 1):
        x = 150 + (30 if line.startswith(" ") else 0)
        page.text(x, y, line.strip(), REGULAR, 11)
        if index % 5 == 0:
            page.text(470, y, str(index), REGULAR, 9)
        y -= 17
        if index % 4 == 0:
            y -= 12
    # A line with a pause in the middle, repeated: two parts, one gap.
    y -= 20
    for left, right in [("Morning came early", "and left the same way"), ("Evening came late", "and stayed a while"), ("Night came softly", "and said nothing at all")]:
        page.text(150, y, left, REGULAR, 11)
        page.text(310, y, right, REGULAR, 11)
        y -= 17
    return page


def code():
    page = Page()
    page.text(72, 740, "Listing", BOLD, 14)
    lines = [
        "count      = items.len();        // how many",
        "total      = items.sum();        // all together",
        "average    = total / count;      // the mean",
        "largest    = items.max();        // upper end",
        "smallest   = items.min();        // lower end",
        "spread     = largest - smallest; // the range",
    ]
    y = 710
    for line in lines:
        page.text(90, y, line, MONO, 9)
        y -= 12
    return page


def contents():
    page = Page()
    page.text(72, 740, "Contents", BOLD, 14)
    entries = [
        ("1", "Introduction", 0, "3"), ("1.1", "Background", 1, "4"), ("1.2", "Scope of the study", 1, "6"),
        ("2", "Methods", 0, "9"), ("2.1", "Sampling", 1, "10"), ("2.2", "Instruments", 1, "13"),
        ("3", "Results", 0, "17"), ("4", "Discussion", 0, "25"),
    ]
    y = 710
    for number, title, level, number_page in entries:
        x = 72 + 18 * level
        page.text(x, y, number, REGULAR, 10)
        title_x = 110 + 18 * level
        page.text(title_x, y, title, REGULAR, 10)
        start = title_x + width(title) + 4
        dots = int((470 - start) / width(".", REGULAR, 10))
        page.text(start, y, "." * dots, REGULAR, 10)
        page.text(500 - width(number_page), y, number_page, REGULAR, 10)
        y -= 16
    return page


def running_text():
    page = Page()
    page.text(72, 760, "Journal of Examples", REGULAR, 9)
    page.text(280, 760, "A Short Title", REGULAR, 9)
    page.text(500, 760, "Page 3", REGULAR, 9)
    paragraph(page, 72, 700, 468, " ".join([PROSE] * 3), 11)
    page.text(72, 40, "Received 3 March", REGULAR, 9)
    page.text(290, 40, "3", REGULAR, 9)
    page.text(480, 40, "Accepted 9 May", REGULAR, 9)
    return page


def wide_gaps():
    """Justified lines of very few words: wide gaps, but at a different place on every line."""
    page = Page()
    page.text(72, 740, "Narrow column", BOLD, 14)
    y = 710
    for value in narrow_lines(12, 150):
        spacing = (150 - width(value)) / value.count(" ")
        page.text(72, y, value, REGULAR, 10, spacing)
        y -= 13
    return page


def small_grids():
    page = Page()
    page.text(72, 740, "Small grids", BOLD, 14)
    # Two rows of four columns.
    table(page, [(72, 80, "l"), (170, 60, "r"), (250, 60, "r"), (330, 60, "r")], 700, [
        ["Item", "One", "Two", "Three"], ["Value", "1", "2", "3"],
    ])
    # Five rows of two columns.
    table(page, [(72, 140, "l"), (230, 80, "r")], 600, [
        ["Width", "12 cm"], ["Height", "30 cm"], ["Depth", "8 cm"], ["Weight", "2 kg"], ["Colour", "grey"],
    ])
    return page


def drifting_columns():
    page = Page()
    page.text(72, 740, "Drifting", BOLD, 14)
    y = 700
    for row in range(6):
        drift = row * 11
        page.text(72 + drift, y, "alpha", REGULAR, 10)
        page.text(190 + drift * 2, y, "beta", REGULAR, 10)
        page.text(320 + drift // 2, y, "gamma", REGULAR, 10)
        y -= 16
    return page


def running_text_in_cells():
    page = Page()
    page.text(72, 740, "Aligned sentences", BOLD, 14)
    rows = [
        ["The committee met in the old hall", "and agreed to publish the list of", "names before the next vote was held"],
        ["Several members asked for more time", "so the secretary offered to send the", "papers by post to each of the villages"],
        ["The treasurer reported a small surplus", "which would be used to repair the roof", "of the hall during the coming spring"],
        ["Nobody objected to the final proposal", "and the meeting closed a little before", "five o'clock after a short vote of thanks"],
    ]
    table(page, [(72, 150, "l"), (232, 150, "l"), (392, 150, "l")], 700, rows, pitch=16, size=8)
    return page


def table_across_columns():
    page = Page()
    page.text(72, 740, "Full width", BOLD, 14)
    top = paragraph(page, 72, 712, 216, PROSE, 9)
    paragraph(page, 324, 712, 216, PROSE, 9)
    columns = [(72, 60, "l"), (150, 40, "r"), (210, 40, "r"), (270, 40, "r"),
               (340, 40, "r"), (400, 40, "r"), (460, 40, "r"), (520, 20, "r")]
    y = min(top, 600) - 20
    y = table(page, columns, y, [
        ["Site", "A1", "A2", "A3", "B1", "B2", "B3", "C"],
        ["North", "11", "12", "13", "21", "22", "23", "1"],
        ["South", "14", "15", "16", "24", "25", "26", "2"],
        ["East", "17", "18", "19", "27", "28", "29", "3"],
        ["West", "31", "32", "33", "34", "35", "36", "4"],
    ], pitch=14, size=9)
    paragraph(page, 72, y - 30, 216, PROSE, 9)
    paragraph(page, 324, y - 30, 216, PROSE, 9)
    return page


def negatives():
    return [
        three_column_prose(), lists(), poetry(), code(), contents(), running_text(), wide_gaps(),
        small_grids(), drifting_columns(), running_text_in_cells(), table_across_columns(),
    ]


# PDF writer --------------------------------------------------------------------------------

def write(path, pages):
    count = len(pages)
    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [%s] /Count %d >>" % (
            b" ".join(b"%d 0 R" % (3 + index) for index in range(count)), count),
    ]
    font_base = 3 + 2 * count
    for index in range(count):
        objects.append(
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents %d 0 R "
            b"/Resources << /Font << /F1 %d 0 R /F2 %d 0 R /F3 %d 0 R >> >> >>"
            % (3 + count + index, font_base, font_base + 1, font_base + 2)
        )
    for page in pages:
        body = page.encode()
        objects.append(b"<< /Length %d >>\nstream\n" % len(body) + body + b"endstream")
    for name in (b"Helvetica", b"Helvetica-Bold", b"Courier"):
        objects.append(b"<< /Type /Font /Subtype /Type1 /BaseFont /%s /Encoding /WinAnsiEncoding >>" % name)

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
    Path(path).write_bytes(out)


target = Path(__file__).resolve().parents[1] / "pdf"
write(target / "unruled-tables.pdf", positives())
write(target / "unruled-negatives.pdf", negatives())
