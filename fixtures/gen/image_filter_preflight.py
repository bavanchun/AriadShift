"""Generate PDFs with indirect and chained image filters."""
from pathlib import Path
jpx = bytearray(45)
jpx[:6] = bytes.fromhex('ff4fff510029')
jpx[8:12] = (20000).to_bytes(4, 'big')
jpx[12:16] = (20000).to_bytes(4, 'big')
jpx[40:42] = (1).to_bytes(2, 'big')
jpx[42:45] = bytes([7, 1, 1])
for name, filter_value, payload in [
    ('indirect-jpx-filter.pdf', b'[ /ASCIIHexDecode 7 0 R ]', b'00>'),
    ('indirect-jbig2-filter.pdf', b'[ /ASCIIHexDecode 7 0 R ]', b'00>'),
]:
    special = b'/JPXDecode' if b'jpx' in name.encode() else b'/JBIG2Decode'
    if special == b'/JPXDecode':
        payload = bytes(jpx).hex().encode() + b'>'
    objects = [b'<< /Type /Catalog /Pages 2 0 R >>', b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>', b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << /XObject << /Im 4 0 R >> >> /Contents 5 0 R >>', b'<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceGray /Filter 6 0 R /Length %d >>\nstream\n' % len(payload) + payload + b'\nendstream', b'<< /Length 6 >>\nstream\n/Im Do\nendstream', filter_value, special]
    out = bytearray(b'%PDF-1.4\n%\xe2\xe3\xcf\xd3\n'); offsets = []
    for number, body in enumerate(objects, 1):
        offsets.append(len(out)); out.extend(b'%d 0 obj\n' % number + body + b'\nendobj\n')
    xref = len(out); out.extend(b'xref\n0 8\n0000000000 65535 f \n')
    for offset in offsets: out.extend(b'%010d 00000 n \n' % offset)
    out.extend(b'trailer\n<< /Size 8 /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n' % xref)
    Path(__file__).resolve().parents[1].joinpath('pdf', name).write_bytes(out)
