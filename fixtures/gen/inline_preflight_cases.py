"""Generate inline-image preflight regression PDFs."""
from pathlib import Path
import zlib

cases = {
    'inline-conflicting-keys.pdf': b'BI /Width 1 /W 20000 /H 20000 /BPC 8 /CS /G /F /AHx ID 00> EI',
    'inline-flate-oversized.pdf': b'BI /W 1 /H 1 /BPC 8 /CS /G /F /Fl ID ' + zlib.compress(bytes(4096)) + b' EI',
}
for name, content in cases.items():
    objects = [b'<< /Type /Catalog /Pages 2 0 R >>', b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>', b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Contents 4 0 R >>', b'<< /Length %d >>\nstream\n' % len(content) + content + b'\nendstream']
    out = bytearray(b'%PDF-1.4\n%\xe2\xe3\xcf\xd3\n')
    offsets = []
    for number, body in enumerate(objects, 1):
        offsets.append(len(out))
        out.extend(b'%d 0 obj\n' % number + body + b'\nendobj\n')
    xref = len(out)
    out.extend(b'xref\n0 5\n0000000000 65535 f \n')
    for offset in offsets:
        out.extend(b'%010d 00000 n \n' % offset)
    out.extend(b'trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n' % xref)
    Path(__file__).resolve().parents[1].joinpath('pdf', name).write_bytes(out)
