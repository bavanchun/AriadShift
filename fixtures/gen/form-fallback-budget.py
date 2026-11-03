"""Generate nested forms that repeatedly invoke a page resource through fallback."""
from pathlib import Path
import zlib

nested = zlib.compress(b'/F Do\n' * 30000)
form = b'0 0 m 1 1 l S'
objects = [
    b'<< /Type /Catalog /Pages 2 0 R >>',
    b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
    b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << /XObject << /G 5 0 R /F 6 0 R >> >> /Contents 4 0 R >>',
    b'<< /Length 5 >>\nstream\n/G Do\nendstream',
    b'<< /Type /XObject /Subtype /Form /BBox [0 0 10 10] /Resources << /Font << >> >> /Filter /FlateDecode /Length %d >>\nstream\n' % len(nested) + nested + b'\nendstream',
    b'<< /Type /XObject /Subtype /Form /BBox [0 0 10 10] /Length %d >>\nstream\n' % len(form) + form + b'\nendstream',
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
Path(__file__).resolve().parents[1].joinpath('pdf/form-fallback-budget.pdf').write_bytes(out)
