"""Generate PDFs with divergent inline and XObject image dictionaries."""
from pathlib import Path

cases = {
    'inline-indirect-key.pdf': (
        b'BI /W 5 0 R /H 1 /BPC 8 /CS /G ID A EI',
        b'1', b'<< >>',
    ),
    'xobject-abbreviated-keys.pdf': (
        b'/Im Do',
        b'<< /Type /XObject /Subtype /Image /W 1 /H 1 /IM true /Length 1 >>\nstream\nA\nendstream',
        b'<< /XObject << /Im 5 0 R >> >>',
    ),
}
for name, (content, image, resources) in cases.items():
    objects = [
        b'<< /Type /Catalog /Pages 2 0 R >>',
        b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
        b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources ' + resources + b' /Contents 4 0 R >>',
        b'<< /Length %d >>\nstream\n' % len(content) + content + b'\nendstream',
        image,
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
    Path(__file__).resolve().parents[1].joinpath('pdf', name).write_bytes(out)
