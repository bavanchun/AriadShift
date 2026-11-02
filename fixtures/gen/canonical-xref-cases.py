"""Generate PDFs with ambiguous original cross-reference interpretations."""
import sys
from pathlib import Path
out_dir = Path(__file__).resolve().parents[1] / "pdf"
HARM = b'<< /Type /XObject /Subtype /Form /BBox [0 0 10 10] /Length 0 >>\nstream\n\nendstream'
EVIL_BODY = b'0 0 1 1 re f 2 2 1 1 re f 4 4 1 1 re f 6 6 1 1 re f'
EVIL = b'<< /Type /XObject /Subtype /Form /BBox [0 0 10 10] /Length %d >>\nstream\n' % len(EVIL_BODY) + EVIL_BODY + b'\nendstream'
def base(gen5=0):
    objs = {2: b'<< /Type /Catalog /Pages 3 0 R >>', 3: b'<< /Type /Pages /Kids [4 0 R] /Count 1 >>',
            4: b'<< /Type /Page /Parent 3 0 R /MediaBox [0 0 10 10] /Resources << /XObject << /F 5 0 R >> >> /Contents 6 0 R >>',
            6: b'<< /Length 5 >>\nstream\n/F Do\nendstream'}
    out = bytearray(b'%PDF-1.4\n%\xe2\xe3\xcf\xd3\n'); off = {}
    for n, b in objs.items():
        off[n] = len(out); out += b'%d 0 obj\n' % n + b + b'\nendobj\n'
    return out, off
def table(off, nums, size, extra=b'', short=False):
    t = b'xref\n0 1\n0000000000 65535 f \n'
    for n in nums:
        o, g = off[n] if isinstance(off[n], tuple) else (off[n], 0)
        t += b'%d 1\n' % n + ((b'%d %d n\r\n' % (o, g)) if short else (b'%010d %05d n \n' % (o, g)))
    return t
# 1) /Prev self-cycle + unreferenced later shadow 5
out, off = base(); off[5] = len(out); out += b'5 0 obj\n' + HARM + b'\nendobj\n'
out += b'5 0 obj\n' + EVIL + b'\nendobj\n'
x = len(out); out += table(off, [2,3,4,5,6], 7) + b'trailer\n<< /Size 7 /Root 2 0 R /Prev %d >>\nstartxref\n%d\n%%%%EOF\n' % (x, x)
(out_dir/'canonical-prev-cycle.pdf').write_bytes(out)
# 2) short (non-20-byte) entries + shadow
out, off = base(); off[5] = len(out); out += b'5 0 obj\n' + HARM + b'\nendobj\n'
out += b'5 0 obj\n' + EVIL + b'\nendobj\n'
x = len(out); out += table(off, [2,3,4,5,6], 7, short=True) + b'trailer\n<< /Size 7 /Root 2 0 R >>\nstartxref\n%d\n%%%%EOF\n' % x
(out_dir/'canonical-short-entries.pdf').write_bytes(out)
# 3) generation precedence: old section 5 gen1 evil, new section 5 gen0 harmless
out, off = base(); e = len(out); out += b'5 1 obj\n' + EVIL + b'\nendobj\n'
off5_old = (e, 1)
o2 = dict(off); o2[5] = off5_old
x1 = len(out); out += table(o2, [2,3,4,5,6], 7) + b'trailer\n<< /Size 7 /Root 2 0 R >>\nstartxref\n%d\n%%%%EOF\n' % x1
h = len(out); out += b'5 0 obj\n' + HARM + b'\nendobj\n'
o3 = {5: (h, 0)}
x2 = len(out); out += b'xref\n5 1\n%010d 00000 n \n' % h + b'trailer\n<< /Size 7 /Root 2 0 R /Prev %d >>\nstartxref\n%d\n%%%%EOF\n' % (x1, x2)
(out_dir/'canonical-gen-precedence.pdf').write_bytes(out)
# control: harmless only
out, off = base(); off[5] = len(out); out += b'5 0 obj\n' + HARM + b'\nendobj\n'
x = len(out); out += table(off, [2,3,4,5,6], 7) + b'trailer\n<< /Size 7 /Root 2 0 R >>\nstartxref\n%d\n%%%%EOF\n' % x
(out_dir/'canonical-control.pdf').write_bytes(out)

import sys, struct
from pathlib import Path
out_dir = Path(__file__).resolve().parents[1] / "pdf"
HARM = b'<< /Type /XObject /Subtype /Form /BBox [0 0 10 10] /Length 0 >>\nstream\n\nendstream'
EB = b'0 0 1 1 re f 2 2 1 1 re f 4 4 1 1 re f 6 6 1 1 re f'
EVIL = b'<< /Type /XObject /Subtype /Form /BBox [0 0 10 10] /Length %d >>\nstream\n' % len(EB) + EB + b'\nendstream'
out = bytearray(b'%PDF-1.5\n%\xe2\xe3\xcf\xd3\n'); off = {}
def obj(n, b):
    off[n] = len(out); out.extend(b'%d 0 obj\n' % n + b + b'\nendobj\n')
obj(2, b'<< /Type /Catalog /Pages 3 0 R >>')
obj(3, b'<< /Type /Pages /Kids [4 0 R] /Count 1 >>')
obj(4, b'<< /Type /Page /Parent 3 0 R /MediaBox [0 0 10 10] /Resources 8 0 R /Contents 6 0 R >>')
obj(5, HARM); obj(9, EVIL)
obj(6, b'<< /Length 5 >>\nstream\n/F Do\nendstream')
a = b'<< /XObject << /F 9 0 R >> >>'; b_ = b'<< /XObject << /F 5 0 R >> >>'
hdr = b'8 0 8 %d ' % (len(a) + 1)
body = hdr + a + b' ' + b_
obj(7, b'<< /Type /ObjStm /N 2 /First %d /Length %d >>\nstream\n' % (len(hdr), len(body)) + body + b'\nendstream')
x = len(out)
rows = {0: (0,0,65535)}
for n in (2,3,4,5,6,7,9): rows[n] = (1, off[n], 0)
rows[8] = (2, 7, 0); rows[10] = (1, x, 0)
data = b''.join(struct.pack('>BIH', *rows.get(n, (0,0,0))) for n in range(11))
out.extend(b'10 0 obj\n<< /Type /XRef /Size 11 /Root 2 0 R /W [1 4 2] /Length %d >>\nstream\n' % len(data) + data + b'\nendstream\nendobj\nstartxref\n%d\n%%%%EOF\n' % x)
(out_dir/'canonical-objstm-index.pdf').write_bytes(out)

import sys, struct
from pathlib import Path
HARM = b'<< /Type /XObject /Subtype /Form /BBox [0 0 10 10] /Length 0 >>\nstream\n\nendstream'
EB = b'0 0 1 1 re f 2 2 1 1 re f 4 4 1 1 re f 6 6 1 1 re f'
EVIL = b'<< /Type /XObject /Subtype /Form /BBox [0 0 10 10] /Length %d >>\nstream\n' % len(EB) + EB + b'\nendstream'
out = bytearray(b'%PDF-1.5\n%\xe2\xe3\xcf\xd3\n'); off = {}
def obj(n, b):
    off[n] = len(out); out.extend(b'%d 0 obj\n' % n + b + b'\nendobj\n')
obj(2, b'<< /Type /Catalog /Pages 3 0 R >>')
obj(3, b'<< /Type /Pages /Kids [4 0 R] /Count 1 >>')
obj(4, b'<< /Type /Page /Parent 3 0 R /MediaBox [0 0 10 10] /Resources << /XObject << /F 5 0 R >> >> /Contents 6 0 R >>')
obj(5, HARM)
obj(6, b'<< /Length 5 >>\nstream\n/F Do\nendstream')
out.extend(b'5 0 obj\n' + EVIL + b'\nendobj\n')
s = len(out)
data = struct.pack('>BIH', 1, off[2], 0)
out.extend(b'7 0 obj\n<< /Type /XRef /Size 8 /W [1 4 2] /Index [2 1] /Length %d >>\nstream\n' % len(data) + data + b'\nendstream\nendobj\n')
x = len(out)
t = b'xref\n0 1\n0000000000 65535 f \n'
for n in (3, 4, 5, 6): t += b'%d 1\n%010d 00000 n \n' % (n, off[n])
t += b'7 1\n%010d 00000 n \n' % s
out += t + b'trailer\n<< /Size 8 /Root 2 0 R /XRefStm %d >>\nstartxref\n%d\n%%%%EOF\n' % (s, x)
(Path(__file__).resolve().parents[1] / 'pdf/canonical-hybrid-root.pdf').write_bytes(out)
