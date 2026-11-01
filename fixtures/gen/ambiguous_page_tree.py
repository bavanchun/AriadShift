"""Generate a PDF with inconsistent page classifications."""
from pathlib import Path
objects = [b'<< /Type /Catalog /Pages 2 0 R >>', b'<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 1 >>', b'<< /Type /Page /Kids [] /Parent 2 0 R /MediaBox [0 0 10 10] >>', b'<< /Type /Foo /Parent 2 0 R /MediaBox [0 0 10 10] >>']
out = bytearray(b'%PDF-1.4\n%\xe2\xe3\xcf\xd3\n'); offsets = []
for number, body in enumerate(objects, 1):
    offsets.append(len(out)); out.extend(b'%d 0 obj\n' % number + body + b'\nendobj\n')
xref = len(out); out.extend(b'xref\n0 5\n0000000000 65535 f \n')
for offset in offsets: out.extend(b'%010d 00000 n \n' % offset)
out.extend(b'trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n' % xref)
Path(__file__).resolve().parents[1].joinpath('pdf/ambiguous-page-tree.pdf').write_bytes(out)
