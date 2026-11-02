"""Generate a small PDF with an object number beyond the safe limit."""
from pathlib import Path

objects = {
    1: b"<< /Type /Catalog /Pages 2 0 R >>",
    2: b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
    3: b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] >>",
    25_165_825: b"null",
}
out = bytearray(b"%PDF-1.4\n")
offsets = {}
for number, body in objects.items():
    offsets[number] = len(out)
    out.extend(f"{number} 0 obj\n".encode() + body + b"\nendobj\n")
xref = len(out)
out.extend(b"xref\n0 4\n0000000000 65535 f \n")
for number in range(1, 4):
    out.extend(f"{offsets[number]:010} 00000 n \n".encode())
out.extend(f"25165825 1\n{offsets[25_165_825]:010} 00000 n \n".encode())
out.extend(f"trailer\n<< /Size 25165826 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode())
Path(__file__).resolve().parents[1].joinpath("pdf/canonical-large-object-number.pdf").write_bytes(out)
