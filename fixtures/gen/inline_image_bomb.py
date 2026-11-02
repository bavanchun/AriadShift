"""Generate a small PDF with oversized inline-image declarations."""

from pathlib import Path
import zlib

content = b"q BI /W 20000 /H 20000 /BPC 8 /CS /G /F /AHx ID 00> EI Q\n" * 40
compressed = zlib.compress(content, level=9)
objects = [
    b"<< /Type /Catalog /Pages 2 0 R >>",
    b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
    b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Contents 4 0 R >>",
    b"<< /Length %d /Filter /FlateDecode >>\nstream\n" % len(compressed)
    + compressed
    + b"\nendstream",
]
out = bytearray(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n")
offsets = []
for number, body in enumerate(objects, 1):
    offsets.append(len(out))
    out.extend(b"%d 0 obj\n" % number + body + b"\nendobj\n")
xref = len(out)
out.extend(b"xref\n0 5\n0000000000 65535 f \n")
for offset in offsets:
    out.extend(b"%010d 00000 n \n" % offset)
out.extend(b"trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % xref)
Path(__file__).resolve().parents[1].joinpath("pdf/inline-image-bomb.pdf").write_bytes(out)
