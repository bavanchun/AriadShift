"""Generate Type3 glyphs resolving forms from each resource scope."""
from pathlib import Path
import zlib

for scope in ('page', 'font', 'glyph'):
    page_resources = b'/XObject << /F 7 0 R >>' if scope == 'page' else b''
    font_resources = b'/Resources << /XObject << /F 7 0 R >> >>' if scope == 'font' else b''
    glyph_resources = b'/Resources << /XObject << /F 7 0 R >> >>' if scope == 'glyph' else b''
    proc = zlib.compress(b'0 0 d0\n' + b'/F Do\n' * 500)
    content = b'BT /T 1 Tf (' + b'A' * 1000 + b') Tj ET'
    form = b'0 0 m 1 1 l S'
    objects = [
        b'<< /Type /Catalog /Pages 2 0 R >>',
        b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
        b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << /Font << /T 4 0 R >> ' + page_resources + b' >> /Contents 6 0 R >>',
        b'<< /Type /Font /Subtype /Type3 /FontBBox [0 0 10 10] /FontMatrix [1 0 0 1 0 0] /CharProcs << /A 5 0 R >> /Encoding << /Type /Encoding /Differences [65 /A] >> /FirstChar 65 /LastChar 65 /Widths [10] ' + font_resources + b' >>',
        b'<< /Filter /FlateDecode /Length %d ' % len(proc) + glyph_resources + b' >>\nstream\n' + proc + b'\nendstream',
        b'<< /Length %d >>\nstream\n' % len(content) + content + b'\nendstream',
        b'<< /Type /XObject /Subtype /Form /BBox [0 0 10 10] /Length %d >>\nstream\n' % len(form) + form + b'\nendstream',
    ]
    out = bytearray(b'%PDF-1.4\n%\xe2\xe3\xcf\xd3\n')
    offsets = []
    for number, body in enumerate(objects, 1):
        offsets.append(len(out))
        out.extend(b'%d 0 obj\n' % number + body + b'\nendobj\n')
    xref = len(out)
    out.extend(b'xref\n0 8\n0000000000 65535 f \n')
    for offset in offsets:
        out.extend(b'%010d 00000 n \n' % offset)
    out.extend(b'trailer\n<< /Size 8 /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n' % xref)
    Path(__file__).resolve().parents[1].joinpath('pdf', f'type3-{scope}-resource.pdf').write_bytes(out)
