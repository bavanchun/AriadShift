"""Generate a mismatched first xref entry and a later shadow object."""
from pathlib import Path

objects = {
    2: b'<< /Type /Catalog /Pages 3 0 R >>',
    3: b'<< /Type /Pages /Kids [4 0 R] /Count 1 >>',
    4: b'<< /Type /Page /Parent 3 0 R /MediaBox [0 0 10 10] /Resources << /XObject << /F 5 0 R >> >> /Contents 6 0 R >>',
    5: b'<< /Type /XObject /Subtype /Form /BBox [0 0 10 10] /Length 0 >>\nstream\n\nendstream',
    6: b'<< /Length 5 >>\nstream\n/F Do\nendstream',
}
out = bytearray(b'%PDF-1.4\n%\xe2\xe3\xcf\xd3\n')
offsets = {}
for number, body in objects.items():
    offsets[number] = len(out)
    out.extend(b'%d 0 obj\n' % number + body + b'\nendobj\n')
out.extend(b'5 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 10 10] /Length 0 >>\nstream\n\nendstream\nendobj\n')
xref = len(out)
out.extend(b'xref\n0 7\n0000000000 65535 f \n')
out.extend(b'%010d 00000 n \n' % offsets[3])
for number in range(2, 7):
    out.extend(b'%010d 00000 n \n' % offsets[number])
out.extend(b'trailer\n<< /Size 7 /Root 2 0 R >>\nstartxref\n%d\n%%%%EOF\n' % xref)
Path(__file__).resolve().parents[1].joinpath('pdf/mismatched-xref-header.pdf').write_bytes(out)
