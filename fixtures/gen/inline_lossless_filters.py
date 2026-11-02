"""Generate small and excessive inline RunLength/LZW streams."""
from pathlib import Path

def lzw(n):
    codes = []
    while n:
        chunk = min(n, 200)
        codes.extend([256] + [0] * chunk)
        n -= chunk
    codes.append(257)
    bits = ''.join(f'{code:09b}' for code in codes)
    bits += '0' * (-len(bits) % 8)
    return bytes(int(bits[i:i+8], 2) for i in range(0, len(bits), 8))

def runlength(n):
    data = bytearray()
    while n:
        size = min(n, 128)
        data.extend((257-size, 0)) if size > 1 else data.extend((0, 0))
        n -= size
    return bytes(data) + b'\x80'

for filt, encode in [('lzw', lzw), ('rl', runlength)]:
    for variant, count in [('small', 1), ('large', 2048)]:
        content = b'BI /W 1 /H 1 /BPC 8 /CS /G /F /' + filt.upper().encode() + b' ID ' + encode(count) + b' EI'
        objects = [b'<< /Type /Catalog /Pages 2 0 R >>', b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>', b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Contents 4 0 R >>', b'<< /Length %d >>\nstream\n' % len(content) + content + b'\nendstream']
        out = bytearray(b'%PDF-1.4\n%\xe2\xe3\xcf\xd3\n'); offsets = []
        for number, body in enumerate(objects, 1): offsets.append(len(out)); out.extend(b'%d 0 obj\n' % number + body + b'\nendobj\n')
        xref = len(out); out.extend(b'xref\n0 5\n0000000000 65535 f \n')
        for offset in offsets: out.extend(b'%010d 00000 n \n' % offset)
        out.extend(b'trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n' % xref)
        Path(__file__).resolve().parents[1].joinpath('pdf', f'inline-{filt}-{variant}.pdf').write_bytes(out)
