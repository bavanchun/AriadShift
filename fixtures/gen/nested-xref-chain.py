"""Generate hybrid xref streams with an additional older table."""
from pathlib import Path
import re
import runpy
import struct

root = Path(__file__).resolve().parents[1]
runpy.run_path(str(root / 'gen/mismatched-xref-header.py'))
base = (root / 'pdf/mismatched-xref-header.pdf').read_bytes()
old_xref = int(re.search(rb'startxref\s+(\d+)', base).group(1))
for valid in (False, True):
    out = bytearray(base)
    offsets = {int(m.group(1)): m.start() for m in re.finditer(rb'(?m)^(\d+) 0 obj', out)}
    # Point at the xref-selected first version of the duplicated form.
    offsets[5] = re.search(rb'(?m)^5 0 obj', out).start()
    if valid:
        bad_entry = b'%010d 00000 n ' % offsets[3]
        pos = out.index(bad_entry, old_xref)
        out[pos:pos + len(bad_entry)] = b'0000000000 65535 f '
    offsets[1] = len(out)
    out.extend(b'1 0 obj\n<< >>\nendobj\n')
    offsets[7] = len(out)
    entry = struct.pack('>BIH', 1, offsets[1], 0)
    out.extend(b'7 0 obj\n<< /Type /XRef /Size 8 /W [1 4 2] /Index [1 1] /Prev %d /Length 7 >>\nstream\n' % old_xref + entry + b'\nendstream\nendobj\n')
    xref = len(out)
    out.extend(b'xref\n0 8\n0000000000 65535 f \n')
    for n in range(1, 8):
        out.extend(b'%010d 00000 n \n' % offsets[n])
    out.extend(b'trailer\n<< /Size 8 /Root 2 0 R /XRefStm %d >>\nstartxref\n%d\n%%%%EOF\n' % (offsets[7], xref))
    name = 'nested-xref-chain-valid.pdf' if valid else 'nested-xref-chain-shadow.pdf'
    (root / 'pdf' / name).write_bytes(out)
