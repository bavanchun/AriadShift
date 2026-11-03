"""Generate PDFs exercising content-stream resource names and glyph counts."""
from pathlib import Path
import zlib

out_dir = Path(__file__).resolve().parents[1] / "pdf"

def stream(data, compress=False):
    encoded = zlib.compress(data) if compress else data
    filt = b" /Filter /FlateDecode" if compress else b""
    return b"<< /Length %d" % len(encoded) + filt + b" >>\nstream\n" + encoded + b"\nendstream"

def save(name, objects):
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
    (out_dir / name).write_bytes(out)

catalog = b"<< /Type /Catalog /Pages 2 0 R >>"
pages = b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"
page = b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << /XObject << /F 5 0 R /Fm#0D1 6 0 R >> >> /Contents 4 0 R >>"
def form(data):
    encoded = zlib.compress(data)
    return b"<< /Type /XObject /Subtype /Form /BBox [0 0 10 10] /Filter /FlateDecode /Length %d >>\nstream\n" % len(encoded) + encoded + b"\nendstream"
save("resource-string-cr-do.pdf", [catalog, pages, page, stream(b"/F Do"), form(b"(Fm\r1) Do\n" * 1100), form(b"0 0 m 1 1 l S\n" * 100)])

page = b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << /Font << /F 5 0 R >> >> /Contents 4 0 R >>"
font = b"<< /Type /Font /Subtype /Type3 /FontBBox [0 0 1 1] /FontMatrix [1 0 0 1 0 0] /CharProcs << /a 6 0 R >> /Encoding << /Type /Encoding /Differences [97 /a] >> /FirstChar 97 /LastChar 97 /Widths [1] >>"
save("type3-crlf-count.pdf", [catalog, pages, page, stream(b"BT /F 1 Tf (" + b"a\r\n" * 20000 + b") Tj ET", True), font, stream(b"q Q")])
