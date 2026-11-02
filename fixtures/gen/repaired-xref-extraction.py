"""Generate a PDF whose cross-reference entries require reconstruction."""
from pathlib import Path

objects = [
    b'<< /Type /Catalog /Pages 2 0 R >>',
    b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
    b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] >>',
]
out = bytearray(b'%PDF-1.4\n%\xe2\xe3\xcf\xd3\n')
for number, body in enumerate(objects, 1):
    out.extend(b'%d 0 obj\n' % number + body + b'\nendobj\n')
out.extend(b'xref\n0 4\n0000000000 65535 f \n')
out.extend(b'0000000001 00000 n \n' * 3)
out.extend(b'trailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n' % 999999)
Path(__file__).resolve().parents[1].joinpath('pdf/repaired-xref-extraction.pdf').write_bytes(out)
