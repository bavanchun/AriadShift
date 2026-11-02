"""Generate a valid image stream containing bytes resembling an overlong name."""
from pathlib import Path

image = b'/' + b'A' * 300
content = b'/Im Do'
objects = [
    b'<< /Type /Catalog /Pages 2 0 R >>',
    b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
    b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 301 1] /Resources << /XObject << /Im 5 0 R >> >> /Contents 4 0 R >>',
    b'<< /Length %d >>\nstream\n' % len(content) + content + b'\nendstream',
    b'<< /Type /XObject /Subtype /Image /Width 301 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /Length %d >>\nstream\n' % len(image) + image + b'\nendstream',
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
Path(__file__).resolve().parents[1].joinpath('pdf/name-like-image-bytes.pdf').write_bytes(out)
