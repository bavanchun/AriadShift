#!/usr/bin/env python3
"""Generate deterministic before/after PDF test fixtures for pdf.compare.

Produces:
  - fixtures/pdf-tools/compare-before.pdf (2 pages)
  - fixtures/pdf-tools/compare-after.pdf (3 pages)

Seeded changes between before and after:
  1. Text edit: "brown" -> "red" on page 1
  2. Moved text: "Moved paragraph header target." shifted by dy > 15pt on page 1
  3. Deleted text: "This sentence will be deleted in the after document." removed on page 1
  4. Visual-only change: 50x50 image changed from solid blue to solid green on page 1
  5. Added page: Page 3 added in after document
  6. Unchanged page: Page 2 is completely identical between before and after
"""

from __future__ import annotations

import hashlib
from io import BytesIO
import os
from pathlib import Path
import tempfile

from PIL import Image
import pikepdf
from pikepdf import Name, String
import typst

ROOT = Path(__file__).resolve().parents[2]
FIXTURES_DIR = ROOT / "fixtures" / "pdf-tools"

_AUTHOR = "AriadShift contributors"
_PRODUCER = "AriadShift fixture generator"
_CREATOR = "AriadShift fixture generator"
_CREATION_DATE = "D:20231114221320Z"
_MOD_DATE = "D:20231114221320Z"
_TIMESTAMP = 1_700_000_000

BEFORE_TYPST = """#set page(paper: "a4", margin: (x: 2cm, y: 2.5cm))
#set text(font: "DejaVu Sans Mono", size: 11pt)

#place(top + left, dx: 0pt, dy: 0pt)[Moved paragraph header target.]

#v(40pt)

= Comparison Document

The quick brown fox jumps over the lazy dog.

Anchor paragraph that remains unchanged across both versions of the document.

This sentence will be deleted in the after document.

Final anchor paragraph on page one that ensures clean diff alignment.

#v(20pt)

#image("tint.png", width: 50pt, height: 50pt)

#pagebreak()

= Page Two Unchanged

This is page two. All content on this page remains completely unchanged and identical between before and after versions.
"""

AFTER_TYPST = """#set page(paper: "a4", margin: (x: 2cm, y: 2.5cm))
#set text(font: "DejaVu Sans Mono", size: 11pt)

#place(top + left, dx: 0pt, dy: 50pt)[Moved paragraph header target.]

#v(40pt)

= Comparison Document

The quick red fox jumps over the lazy dog.

Anchor paragraph that remains unchanged across both versions of the document.

Final anchor paragraph on page one that ensures clean diff alignment.

#v(20pt)

#image("tint.png", width: 50pt, height: 50pt)

#pagebreak()

= Page Two Unchanged

This is page two. All content on this page remains completely unchanged and identical between before and after versions.

#pagebreak()

= Page Three Added

This is page three. It was added in the after document and does not exist in the before document.
"""


def compile_pdf(typst_source: str, image_color: tuple[int, int, int], title: str) -> bytes:
    with tempfile.TemporaryDirectory() as td:
        img = Image.new("RGB", (50, 50), color=image_color)
        img.save(os.path.join(td, "tint.png"))

        raw_pdf = typst.compile(
            typst_source.encode("utf-8"),
            root=td,
            format="pdf",
            font_paths=[],
            ignore_system_fonts=True,
            timestamp=_TIMESTAMP,
        )

    # Normalize PDF metadata deterministically
    with pikepdf.open(BytesIO(raw_pdf)) as pdf:
        pdf.docinfo[Name.Title] = String(title)
        pdf.docinfo[Name.Author] = String(_AUTHOR)
        pdf.docinfo[Name.Creator] = String(_CREATOR)
        pdf.docinfo[Name.Producer] = String(_PRODUCER)
        pdf.docinfo[Name.CreationDate] = String(_CREATION_DATE)
        pdf.docinfo[Name.ModDate] = String(_MOD_DATE)

        out = BytesIO()
        pdf.save(out, object_stream_mode=pikepdf.ObjectStreamMode.generate)
        return out.getvalue()


def main() -> None:
    FIXTURES_DIR.mkdir(parents=True, exist_ok=True)

    before_pdf = compile_pdf(
        BEFORE_TYPST,
        image_color=(0, 0, 255),  # Blue
        title="PDF Compare Before Document",
    )
    after_pdf = compile_pdf(
        AFTER_TYPST,
        image_color=(0, 255, 0),  # Green
        title="PDF Compare After Document",
    )

    before_path = FIXTURES_DIR / "compare-before.pdf"
    after_path = FIXTURES_DIR / "compare-after.pdf"

    before_path.write_bytes(before_pdf)
    after_path.write_bytes(after_pdf)

    h_before = hashlib.sha256(before_pdf).hexdigest()
    h_after = hashlib.sha256(after_pdf).hexdigest()

    print(f"compare-before.pdf: {len(before_pdf)} bytes, SHA-256: {h_before}")
    print(f"compare-after.pdf:  {len(after_pdf)} bytes, SHA-256: {h_after}")


if __name__ == "__main__":
    main()
