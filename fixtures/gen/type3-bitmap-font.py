"""Generate a small Type3 bitmap glyph with an inline CCITT image mask."""
from pathlib import Path

glyph = (b'8 0 d0 q 8 0 0 8 0 0 cm BI /IM true /W 8 /H 8 /F /CCF '
         b'/D [1 0] /DP << /K -1 /Columns 8 /Rows 8 /EndOfBlock false >> ID \xff EI Q')
page = b'BT /F1 12 Tf 5 5 Td (A) Tj ET'
def stream(data):
    return b'<< /Length %d >>\nstream\n' % len(data) + data + b'\nendstream'
objects = [
    b'<< /Type /Catalog /Pages 2 0 R >>',
    b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
    b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 30 30] /Resources << /Font << /F1 4 0 R >> >> /Contents 6 0 R >>',
    b'<< /Type /Font /Subtype /Type3 /FontBBox [0 0 8 8] /FontMatrix [.125 0 0 .125 0 0] /CharProcs << /A 5 0 R >> /Encoding << /Type /Encoding /Differences [65 /A] >> /FirstChar 65 /LastChar 65 /Widths [8] >>',
    stream(glyph), stream(page),
]
out = bytearray(b'%PDF-1.4\n%\xe2\xe3\xcf\xd3\n')
offsets = []
for number, body in enumerate(objects, 1):
    offsets.append(len(out))
    out.extend(b'%d 0 obj\n' % number + body + b'\nendobj\n')
xref = len(out)
out.extend(b'xref\n0 7\n0000000000 65535 f \n')
for offset in offsets:
    out.extend(b'%010d 00000 n \n' % offset)
out.extend(b'trailer\n<< /Size 7 /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n' % xref)
Path(__file__).resolve().parents[1].joinpath('pdf/type3-bitmap-font.pdf').write_bytes(out)
