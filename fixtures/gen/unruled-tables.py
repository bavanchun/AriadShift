"""Generate PDFs for whitespace-aligned table detection.

`unruled-tables.pdf` holds tables that have no ruling lines, or only booktabs-style horizontal
rules, in a single-column page and beside prose in one column of a two-column page.

`unruled-negatives.pdf` has one page per layout that looks tabular but is not a table, or is too
uncertain to call one: multi-column prose, nested lists, poetry, code, tables of contents, running
headers, justified text with wide gaps, grids that are too small, drifting columns, running text
in aligned cells, and a table that spans both columns of a page. The last fourteen pages are near
misses whose text is aligned in three or more short columns, as in a table: a table of contents
without leaders, numbered clauses, verse with line numbers, a form of labels and values, a
signature block, a letterhead footer, code with aligned comments, two bibliographies, a two-column
index, numbered equations, a list of abbreviations, quiz options and a transcript with time
codes. A reader must find no table on any of them.
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


# Near misses: layouts that align text in columns but are lists, forms, notes or other prose ----

def contents_without_leaders():
    page = Page()
    page.text(72, 740, "Contents", BOLD, 14)
    entries = [
        ("1", "Introduction", "3"), ("1.1", "Background", "4"), ("1.2", "Scope of the study", "6"),
        ("2", "Methods", "9"), ("2.1", "Sampling", "10"), ("2.2", "Instruments", "13"),
        ("3", "Results", "17"), ("4", "Discussion", "25"),
    ]
    y = 710
    for number, title, first_page in entries:
        page.text(72, y, number)
        page.text(130, y, title)
        page.cell(440, 60, y, first_page, "r")
        y -= 16
    return page


def numbered_clauses():
    """Two numbering levels in two marker columns, with the clause text after them."""
    page = Page()
    page.text(72, 740, "Terms", BOLD, 14)
    clauses = [
        ("1.", ["Deliver to the named address.", "Agree the dates in writing.",
                "Late delivery earns a credit."]),
        ("2.", ["Pay within thirty days.", "Pay by bank transfer only.",
                "Settle disputed sums later."]),
        ("3.", ["End this deal in writing.", "Notice runs from receipt.",
                "Delivered goods stay put."]),
    ]
    y = 710
    for number, items in clauses:
        page.text(72, y, number)
        for index, item in enumerate(items):
            page.text(100, y, f"({'abc'[index]})")
            page.text(132, y, item)
            y -= 14
        y -= 4
    return page


def verse_with_line_numbers():
    page = Page()
    page.text(72, 740, "Verse", BOLD, 14)
    lines = [
        ("the rain began", "and the wind rose"), ("we walked on", "through the grey fields"),
        ("no bird sang", "over the dark water"), ("the lamps were lit", "along the old road"),
        ("a door stood open", "at the end of it"), ("so we went in", "and shut out the cold"),
    ]
    y = 710
    for number, (left, right) in enumerate(lines, 1):
        page.text(72, y, str(number), REGULAR, 10)
        page.text(120, y, left, REGULAR, 10)
        page.text(290, y, right, REGULAR, 10)
        y -= 16
    return page


def label_and_value_form():
    page = Page()
    page.text(72, 740, "Application", BOLD, 14)
    rows = [
        ("Name:", "Jane Doe", "Date:", "12 May 2024"),
        ("Address:", "14 Mill Lane", "City:", "Leeds"),
        ("Phone:", "555 0143", "Email:", "jane@example.org"),
        ("Account:", "00481516", "Branch:", "North"),
    ]
    y = 700
    for label, value, other_label, other_value in rows:
        page.text(72, y, label, BOLD, 10)
        page.text(140, y, value)
        page.text(300, y, other_label, BOLD, 10)
        page.text(360, y, other_value)
        y -= 20
    return page


def signature_block():
    page = Page()
    page.text(72, 740, "Approval", BOLD, 14)
    paragraph(page, 72, 712, 468, sentences(0, 2), 11)
    rows = [
        ("Prepared by", "Reviewed by", "Approved by"),
        ("J. Smith", "A. Jones", "K. Lee"),
        ("Date: 12 May", "Date: 14 May", "Date: 15 May"),
    ]
    y = 560
    for row in rows:
        for x, value in zip((72, 240, 410), row):
            page.text(x, y, value)
        y -= 22
    return page


def letterhead_footer():
    page = Page()
    page.text(72, 740, "Acme Trading Ltd", BOLD, 14)
    paragraph(page, 72, 700, 468, sentences(0, 3), 11)
    rows = [
        ("Acme Trading Ltd", "12 High Street, Leeds", "Tel 0113 555 0100"),
        ("Registered in England 04815162", "Leeds LS1 4AB", "Fax 0113 555 0101"),
        ("VAT GB 123 4567 89", "www.example.com", "info@example.com"),
    ]
    y = 64
    for row in rows:
        for x, value in zip((72, 250, 430), row):
            page.text(x, y, value, REGULAR, 8)
        y -= 10
    return page


def code_with_comments():
    """Code set in a proportional face, with the comments aligned in a column."""
    page = Page()
    page.text(72, 740, "Listing", BOLD, 14)
    rows = [
        ("total", "= sum(items);", "// all together"),
        ("count", "= len(items);", "// how many"),
        ("mean", "= total / count;", "// the average"),
        ("peak", "= max(items);", "// upper end"),
    ]
    y = 710
    for name, statement, comment in rows:
        page.text(90, y, name, REGULAR, 9)
        page.text(150, y, statement, REGULAR, 9)
        page.text(280, y, comment, REGULAR, 9)
        y -= 13
    return page


BIBLIOGRAPHY = [
    ("Nguyen, T.", "2018", "Reading order in digital documents", "Pattern Recognit."),
    ("Garcia, M.", "2020", "A survey of table extraction methods", "ACM Surveys"),
    ("Okafor, A.", "2021", "Layout analysis without ruling lines", "Doc. Anal. Rec."),
    ("Lindqvist, E.", "2022", "Aligned text and its hidden structure", "IJDAR"),
    ("Tanaka, H.", "2023", "Precision over recall in tables", "Proc. ICDAR"),
]


def bibliography_with_years():
    page = Page()
    page.text(72, 740, "References", BOLD, 14)
    y = 710
    for authors, year, title, venue in BIBLIOGRAPHY:
        page.text(72, y, authors, REGULAR, 9)
        page.text(170, y, year, REGULAR, 9)
        page.text(215, y, title, REGULAR, 9)
        page.text(430, y, venue, REGULAR, 9)
        y -= 14
    return page


def bibliography_with_wrapped_lines():
    page = Page()
    page.text(72, 740, "References", BOLD, 14)
    y = 710
    for authors, year, title, venue in BIBLIOGRAPHY:
        words = title.split()
        first, second = " ".join(words[:3]), " ".join(words[3:])
        page.text(72, y, authors, REGULAR, 9)
        page.text(170, y, year, REGULAR, 9)
        page.text(215, y, first, REGULAR, 9)
        page.text(380, y, venue, REGULAR, 9)
        page.text(215, y - 11, second, REGULAR, 9)
        y -= 28
    return page


def two_column_index():
    page = Page()
    page.text(72, 740, "Index", BOLD, 14)
    left = [("Aperture", "12"), ("Bokeh", "14"), ("Contrast", "15"), ("Depth of field", "18"),
            ("Exposure", "21"), ("Focal length", "23"), ("Gamma", "25"), ("Histogram", "27"),
            ("ISO speed", "30"), ("Jitter", "31")]
    right = [("Kernel", "33"), ("Lens hood", "35"), ("Macro", "36"), ("Noise", "38"),
             ("Overexposure", "40"), ("Panorama", "41"), ("Quantization", "44"), ("Raw file", "46"),
             ("Shutter", "48"), ("Tripod", "50")]
    y = 710
    for (term, number), (other, other_number) in zip(left, right):
        page.text(72, y, term)
        page.cell(160, 40, y, number, "r")
        page.text(300, y, other)
        page.cell(400, 40, y, other_number, "r")
        y -= 15
    return page


def numbered_equations():
    page = Page()
    page.text(72, 740, "Derivation", BOLD, 14)
    rows = [
        ("x", "a + b"), ("y", "c - d"), ("z", "2e"), ("u", "f / g"), ("w", "h + i + j"),
    ]
    y = 700
    for number, (left, right) in enumerate(rows, 1):
        page.text(150, y, left, REGULAR, 11)
        page.text(190, y, "=", REGULAR, 11)
        page.text(230, y, right, REGULAR, 11)
        page.cell(440, 60, y, f"({number})", "r", REGULAR, 11)
        y -= 22
    return page


def list_of_abbreviations():
    page = Page()
    page.text(72, 740, "Abbreviations", BOLD, 14)
    rows = [
        ("ACL", "Association for Computational Linguistics", "12"),
        ("CNN", "Convolutional neural network", "14"),
        ("OCR", "Optical character recognition", "21"),
        ("PDF", "Portable document format", "3"),
        ("TEDS", "Tree edit distance based similarity", "45"),
        ("IoU", "Intersection over union", "47"),
    ]
    y = 710
    for abbreviation, expansion, number in rows:
        page.text(72, y, abbreviation, BOLD, 10)
        page.text(130, y, expansion)
        page.cell(440, 60, y, number, "r")
        y -= 16
    return page


def quiz_options():
    """Short questions with four lettered options in columns, as a maths quiz sets them."""
    page = Page()
    page.text(72, 740, "Quiz", BOLD, 14)
    rows = [
        ("7 + 8 =", "(a) 14", "(b) 15", "(c) 16", "(d) 17"),
        ("9 + 6 =", "(a) 13", "(b) 14", "(c) 15", "(d) 16"),
        ("4 + 9 =", "(a) 12", "(b) 13", "(c) 14", "(d) 15"),
        ("8 + 8 =", "(a) 15", "(b) 16", "(c) 17", "(d) 18"),
    ]
    y = 710
    for row in rows:
        for x, value in zip((72, 160, 250, 340, 430), row):
            page.text(x, y, value)
        y -= 20
    return page


def transcript_with_time_codes():
    page = Page()
    page.text(72, 740, "Interview", BOLD, 14)
    rows = [
        ("00:01", "Interviewer:", "Where did you grow up?"),
        ("00:05", "Maya:", "In a small town near Leeds."),
        ("00:09", "Interviewer:", "What did your parents do?"),
        ("00:14", "Maya:", "My father drove a bus."),
        ("00:17", "Interviewer:", "Did you like school?"),
    ]
    y = 710
    for code, speaker, utterance in rows:
        page.text(72, y, code)
        page.text(130, y, speaker, BOLD, 10)
        page.text(220, y, utterance)
        y -= 18
    return page


def negatives():
    return [
        three_column_prose(), lists(), poetry(), code(), contents(), running_text(), wide_gaps(),
        small_grids(), drifting_columns(), running_text_in_cells(), table_across_columns(),
        contents_without_leaders(), numbered_clauses(), verse_with_line_numbers(),
        label_and_value_form(), signature_block(), letterhead_footer(), code_with_comments(),
        bibliography_with_years(), bibliography_with_wrapped_lines(), two_column_index(),
        numbered_equations(), list_of_abbreviations(), quiz_options(),
        transcript_with_time_codes(),
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
