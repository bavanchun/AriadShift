"""Generate a PDF with a long escaped resource name in an object stream."""
from pathlib import Path
import struct
import zlib

name = b'A' * 253 + b'#41tail'
resource = b'<< /XObject << /' + name + b' 5 0 R >> >>'
embedded = b'7 0 ' + resource
compressed = zlib.compress(embedded)
objects = [
    b'<< /Type /Catalog /Pages 2 0 R >>',
    b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
    b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources 7 0 R /Contents 4 0 R >>',
    b'<< /Length 3 >>\nstream\nq Q\nendstream',
    b'<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /Length 1 >>\nstream\nA\nendstream',
    b'<< /Type /ObjStm /N 1 /First 4 /Filter /FlateDecode /Length %d >>\nstream\n' % len(compressed) + compressed + b'\nendstream',
]
out = bytearray(b'%PDF-1.5\n%\xe2\xe3\xcf\xd3\n')
offsets = []
for number, body in enumerate(objects, 1):
    offsets.append(len(out))
    out.extend(b'%d 0 obj\n' % number + body + b'\nendobj\n')
xref = len(out)
entries = [(0, 0, 65535)] + [(1, offset, 0) for offset in offsets] + [(2, 6, 0), (1, xref, 0)]
data = b''.join(struct.pack('>BIH', *entry) for entry in entries)
out.extend(b'8 0 obj\n<< /Type /XRef /Size 9 /Root 1 0 R /W [1 4 2] /Length %d >>\nstream\n' % len(data) + data + b'\nendstream\nendobj\n')
out.extend(b'startxref\n%d\n%%%%EOF\n' % xref)
Path(__file__).resolve().parents[1].joinpath('pdf/compressed-resource-name.pdf').write_bytes(out)
