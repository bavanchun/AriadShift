"""Generate an incremental PDF hiding an invalid older xref entry."""
from pathlib import Path
import re
import runpy

root = Path(__file__).resolve().parents[1]
runpy.run_path(str(root / 'gen/mismatched-xref-header.py'))
out = bytearray((root / 'pdf/mismatched-xref-header.pdf').read_bytes())
previous = int(re.search(rb'startxref\s+(\d+)', out).group(1))
object_offset = len(out)
out.extend(b'1 0 obj\n<< >>\nendobj\n')
xref_offset = len(out)
out.extend(b'xref\n1 1\n%010d 00000 n \n' % object_offset)
out.extend(b'trailer\n<< /Size 7 /Root 2 0 R /Prev %d >>\nstartxref\n%d\n%%%%EOF\n' % (previous, xref_offset))
(root / 'pdf/incremental-xref-shadow.pdf').write_bytes(out)
