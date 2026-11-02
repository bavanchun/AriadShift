"""Generate small PDFs for canonical content validation."""
from pathlib import Path
import hashlib

OUT = Path(__file__).resolve().parents[1] / "pdf"


def stream(data, entries=b""):
    return b"<< /Length %d " % len(data) + entries + b" >>\nstream\n" + data + b"\nendstream"


def save(name, objects):
    out = bytearray(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n")
    offsets = []
    for i, obj in enumerate(objects, 1):
        offsets.append(len(out))
        out += b"%d 0 obj\n" % i + obj + b"\nendobj\n"
    xref = len(out)
    out += b"xref\n0 %d\n0000000000 65535 f \n" % (len(objects) + 1)
    for offset in offsets:
        out += b"%010d 00000 n \n" % offset
    out += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (len(objects) + 1, xref)
    (OUT / name).write_bytes(out)
    print(name, hashlib.sha256(out).hexdigest())


catalog = b"<< /Type /Catalog /Pages 2 0 R >>"
pages = b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"
page = b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << >> /Contents 4 0 R >>"
save("content-non-ascii.pdf", [catalog, pages, page, stream(b"q\x80Q q\xffQ")])
save("content-nested-operands.pdf", [catalog, pages, page, stream(b"[[ ]] << 1 2 >>")])
save("inline-string-filter.pdf", [catalog, pages, page, stream(b"BI /W 1 /H 1 /BPC 8 /CS /G /F (AHx) ID 00> EI")])
page = b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << /Pattern << /P 5 0 R >> /ExtGState << /S << /SMask << /S /Luminosity /G 7 0 R >> >> >> >> /Annots [6 0 R] /Contents 4 0 R >>"
pattern = stream(b"q\x80Q", b"/Type /Pattern /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 10 10] /XStep 10 /YStep 10 /Resources << >>")
annotation = b"<< /Type /Annot /Subtype /Stamp /Rect [0 0 10 10] /AP << /N << /On 8 0 R >> >> >>"
form = lambda: stream(b"q\xffQ", b"/Type /XObject /Subtype /Form /BBox [0 0 10 10] /Resources << >>")
save("content-executable-containers.pdf", [catalog, pages, page, stream(b"q Q"), pattern, annotation, form(), form()])
