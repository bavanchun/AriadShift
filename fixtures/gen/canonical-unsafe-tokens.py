"""Generate PDFs containing unsafe canonical tokens."""
from pathlib import Path
out_dir = Path(__file__).resolve().parents[1] / "pdf"
HARM = b'<< /Type /XObject /Subtype /Form /BBox [0 0 10 10] /Length 0 >>\nstream\n\nendstream'
EVIL_BODY = b'0 0 1 1 re f 2 2 1 1 re f 4 4 1 1 re f 6 6 1 1 re f'
EVIL = b'<< /Type /XObject /Subtype /Form /BBox [0 0 10 10] /Length %d >>\nstream\n' % len(EVIL_BODY) + EVIL_BODY + b'\nendstream'
BIG = b'1' + b'0' * 45 + b'.0'
def build(extra):
    objs = {2: b'<< /Type /Catalog /Pages 3 0 R >>', 3: b'<< /Type /Pages /Kids [4 0 R] /Count 1 >>',
            4: b'<< /Type /Page /Parent 3 0 R /MediaBox [0 0 10 10] /Resources << /XObject << /F 5 0 R >> >> /Contents 6 0 R ' + extra + b' >>',
            5: HARM, 6: b'<< /Length 5 >>\nstream\n/F Do\nendstream', 7: EVIL}
    out = bytearray(b'%PDF-1.4\n%\xe2\xe3\xcf\xd3\n'); off = {}
    for n, b in objs.items():
        off[n] = len(out); out += b'%d 0 obj\n' % n + b + b'\nendobj\n'
    x = len(out)
    out += b'xref\n0 1\n0000000000 65535 f \n2 6\n' + b''.join(b'%010d 00000 n \n' % off[n] for n in range(2, 8))
    out += b'trailer\n<< /Size 8 /Root 2 0 R >>\nstartxref\n%d\n%%%%EOF\n' % x
    return out
(out_dir / 'canonical-inf-real.pdf').write_bytes(build(b'/Foo [' + BIG + b' /Resources << /XObject << /F 7 0 R >> >>]'))
(out_dir / 'canonical-big-integer.pdf').write_bytes(build(b'/Foo 2147483648'))
