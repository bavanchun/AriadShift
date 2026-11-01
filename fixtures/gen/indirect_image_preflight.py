"""Generate a PDF whose form has an indirect subtype."""
from pathlib import Path
content = b'BI /W 20000 /H 20000 /BPC 8 /CS /G /F /AHx ID 00> EI'
objects = [
    b'<< /Type /Catalog /Pages 2 0 R >>',
    b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
    b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << /XObject << /Fm 4 0 R >> >> /Contents 6 0 R >>',
    b'<< /Type /XObject /Subtype 5 0 R /BBox [0 0 10 10] /Length %d >>\nstream\n' % len(content) + content + b'\nendstream',
    b'/Form',
    b'<< /Length 6 >>\nstream\n/Fm Do\nendstream',
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
Path(__file__).resolve().parents[1].joinpath('pdf/indirect-image-preflight.pdf').write_bytes(out)
