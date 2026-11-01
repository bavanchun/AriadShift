"""Generate a shared form invoked with two inherited image scopes."""
from pathlib import Path
objects = [b'<< /Type /Catalog /Pages 2 0 R >>', b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>', b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] >>', b'<< /Type /XObject /Subtype /Form /BBox [0 0 10 10] /Length 6 >>\nstream\n/Im Do\nendstream', b'<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /Length 1 >>\nstream\nA\nendstream', b'<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /Length 1 >>\nstream\nB\nendstream']
out = bytearray(b'%PDF-1.4\n%\xe2\xe3\xcf\xd3\n'); offsets = []
for number, body in enumerate(objects, 1): offsets.append(len(out)); out.extend(b'%d 0 obj\n' % number + body + b'\nendobj\n')
xref = len(out); out.extend(b'xref\n0 7\n0000000000 65535 f \n')
for offset in offsets: out.extend(b'%010d 00000 n \n' % offset)
out.extend(b'trailer\n<< /Size 7 /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n' % xref)
Path(__file__).resolve().parents[1].joinpath('pdf/form-resource-scopes.pdf').write_bytes(out)
