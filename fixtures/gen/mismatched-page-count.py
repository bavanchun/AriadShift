"""Generate a PDF whose intermediate page count disagrees with its leaves."""
from pathlib import Path

objects = {
    1: b"<< /Type /Catalog /Pages 2 0 R >>",
    2: b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
    3: b"<< /Type /Pages /Parent 2 0 R /Kids [4 0 R] /Count 2 >>",
    4: b"<< /Type /Page /Parent 3 0 R /MediaBox [0 0 10 10] >>",
}
out = bytearray(b"%PDF-1.4\n")
offsets = {}
for number, body in objects.items():
    offsets[number] = len(out)
    out.extend(f"{number} 0 obj\n".encode() + body + b"\nendobj\n")
xref = len(out)
out.extend(b"xref\n0 5\n0000000000 65535 f \n")
for number in range(1, 5):
    out.extend(f"{offsets[number]:010} 00000 n \n".encode())
out.extend(f"trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode())
Path(__file__).resolve().parents[1].joinpath("pdf/mismatched-page-count.pdf").write_bytes(out)
