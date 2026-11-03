"""Generate a compressed page that repeatedly invokes an escaped form name."""
from pathlib import Path
import zlib

content = zlib.compress(b'/Im#ZZ Do\n' * 30000)
form = b'0 0 m 1 1 l S'
objects = [
    b'<< /Type /Catalog /Pages 2 0 R >>',
    b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
    b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << /XObject << /Im#00 5 0 R >> >> /Contents 4 0 R >>',
    b'<< /Filter /FlateDecode /Length %d >>\nstream\n' % len(content) + content + b'\nendstream',
    b'<< /Type /XObject /Subtype /Form /BBox [0 0 10 10] /Length %d >>\nstream\n' % len(form) + form + b'\nendstream',
]
out = bytearray(b'%PDF-1.4\n%\xe2\xe3\xcf\xd3\n')
offsets = []
for number, body in enumerate(objects, 1):
    offsets.append(len(out))
    out.extend(b'%d 0 obj\n' % number + body + b'\nendobj\n')
xref = len(out)
out.extend(b'xref\n0 6\n0000000000 65535 f \n')
for offset in offsets:
    out.extend(b'%010d 00000 n \n' % offset)
out.extend(b'trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n' % xref)
Path(__file__).resolve().parents[1].joinpath('pdf/invalid-name-escape.pdf').write_bytes(out)
