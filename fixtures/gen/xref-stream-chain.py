"""Generate incremental and hybrid PDFs with hidden xref-stream mismatches."""
from pathlib import Path
import struct
import zlib

root = Path(__file__).resolve().parents[1] / 'pdf'
for kind, valid in (('incremental', False), ('hybrid', False), ('incremental', True), ('hybrid', True), ('overlapping', False)):
    out = bytearray(b'%PDF-1.5\n%\xe2\xe3\xcf\xd3\n')
    offsets = {}
    for number, body in [(1, b'<< >>'), (2, b'<< /Type /Catalog /Pages 3 0 R >>'),
                         (3, b'<< /Type /Pages /Kids [4 0 R] /Count 1 >>'),
                         (4, b'<< /Type /Page /Parent 3 0 R /MediaBox [0 0 10 10] >>')]:
        offsets[number] = len(out)
        out.extend(b'%d 0 obj\n' % number + body + b'\nendobj\n')
    old_offset = len(out)
    # Object 1's entry points to object 2; the newest table replaces it.
    entry = struct.pack('>BIH', 1, offsets[1 if valid else 2], 0)
    index = b'1 1'
    if kind == 'overlapping':
        index = b'1 1 1 1'
        entry += struct.pack('>BIH', 1, offsets[1], 0)
    stream_filter = b''
    if kind == 'incremental':
        entry = zlib.compress(entry)
        stream_filter = b'/Filter /FlateDecode '
    out.extend(b'5 0 obj\n<< /Type /XRef /Size 6 /Root 2 0 R /W [1 4 2] /Index [' + index + b'] ' + stream_filter + b'/Length %d >>\nstream\n' % len(entry) + entry + b'\nendstream\nendobj\n')
    xref = len(out)
    out.extend(b'xref\n0 6\n0000000000 65535 f \n')
    for n in range(1, 5):
        out.extend(b'%010d 00000 n \n' % offsets[n])
    out.extend(b'%010d 00000 n \n' % old_offset)
    link = b'/Prev' if kind == 'incremental' else b'/XRefStm'
    out.extend(b'trailer\n<< /Size 6 /Root 2 0 R ' + link + b' %d >>\nstartxref\n%d\n%%%%EOF\n' % (old_offset, xref))
    suffix = 'valid' if valid else 'shadow'
    (root / f'{kind}-xref-stream-{suffix}.pdf').write_bytes(out)
