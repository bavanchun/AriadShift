"""Generate small PDFs for Type3 resource and inherited-font budgeting."""
from pathlib import Path
import zlib


def stream(attrs, data):
    data = zlib.compress(data)
    return b'<< /Filter /FlateDecode /Length %d ' % len(data) + attrs + b' >>\nstream\n' + data + b'\nendstream'


def write(name, objects):
    out = bytearray(b'%PDF-1.4\n%\xe2\xe3\xcf\xd3\n')
    offsets = []
    for number, body in enumerate(objects, 1):
        offsets.append(len(out))
        out.extend(b'%d 0 obj\n' % number + body + b'\nendobj\n')
    xref = len(out)
    out.extend(b'xref\n0 %d\n0000000000 65535 f \n' % (len(objects) + 1))
    for offset in offsets:
        out.extend(b'%010d 00000 n \n' % offset)
    out.extend(b'trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n' % (len(objects) + 1, xref))
    Path(__file__).resolve().parents[1].joinpath('pdf', name).write_bytes(out)


catalog = b'<< /Type /Catalog /Pages 2 0 R >>'
pages = b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>'
font = b'<< /Type /Font /Subtype /Type3 /FontBBox [0 0 10 10] /FontMatrix [1 0 0 1 0 0] /CharProcs << /A 5 0 R >> /Encoding << /Type /Encoding /Differences [65 /A] >> /FirstChar 65 /LastChar 65 /Widths [10] '
page = b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << /Font << /T 4 0 R >> /XObject << /F 7 0 R >> >> /Contents 6 0 R >>'
write('type3-charproc-resource-budget.pdf', [catalog, pages, page.replace(b' /XObject << /F 7 0 R >>', b''),
    font + b'/Resources << /Pattern << /P 7 0 R >> >> >>',
    stream(b'/Resources << >>', b'0 0 d0 ' + b'/Pattern cs /P scn 0 0 1 1 re f ' * 50),
    stream(b'', b'BT /T 1 Tf (' + b'A' * 100 + b') Tj ET'),
    stream(b'/Type /Pattern /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 1 1] /XStep 1 /YStep 1 /Resources << >>', b'0 0 1 1 re f ' * 50)])
write('type3-inherited-font-budget.pdf', [catalog, pages,
    b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << >> /Contents 6 0 R /Annots [8 0 R] >>',
    font + b'>>', stream(b'', b'0 0 d0 ' + b'0 0 1 1 re f ' * 750),
    stream(b'', b'q Q'),
    stream(b'/Type /XObject /Subtype /Form /BBox [0 0 10 10] /Resources << /Font << >> >>', b'(' + b'A' * 100 + b') Tj'),
    b'<< /Type /Annot /Subtype /Stamp /Rect [0 0 10 10] /AP << /N 9 0 R >> >>',
    stream(b'/Type /XObject /Subtype /Form /BBox [0 0 10 10] /Resources << /Font << /T 4 0 R >> /XObject << /F 7 0 R >> >>', b'BT /T 1 Tf /F Do ET')])
