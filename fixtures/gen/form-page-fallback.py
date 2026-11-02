"""Generate nested forms that shadow a page resource."""
from pathlib import Path

objects = [
    b'<< /Type /Catalog /Pages 2 0 R >>',
    b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
    b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << /XObject << /X 7 0 R /G 4 0 R >> >> /Contents 8 0 R >>',
    b'<< /Type /XObject /Subtype /Form /BBox [0 0 10 10] /Resources << /XObject << /X 6 0 R /F 5 0 R >> >> /Length 5 >>\nstream\n/F Do\nendstream',
    b'<< /Type /XObject /Subtype /Form /BBox [0 0 10 10] /Resources << /Font << >> >> /Length 5 >>\nstream\n/X Do\nendstream',
    b'<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /Length 1 >>\nstream\nA\nendstream',
    b'<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /Length 1 >>\nstream\nB\nendstream',
    b'<< /Length 5 >>\nstream\n/G Do\nendstream',
]
out = bytearray(b'%PDF-1.4\n%\xe2\xe3\xcf\xd3\n')
offsets = []
for number, body in enumerate(objects, 1):
    offsets.append(len(out))
    out.extend(b'%d 0 obj\n' % number + body + b'\nendobj\n')
xref = len(out)
out.extend(b'xref\n0 9\n0000000000 65535 f \n')
for offset in offsets:
    out.extend(b'%010d 00000 n \n' % offset)
out.extend(b'trailer\n<< /Size 9 /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n' % xref)
Path(__file__).resolve().parents[1].joinpath('pdf/form-page-fallback.pdf').write_bytes(out)
