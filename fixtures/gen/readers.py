"""Generate deterministic TXT, CSV, EML, SVG, and CBZ fixtures and fuzz seeds."""
from pathlib import Path
import zipfile
import io

ROOT = Path(__file__).resolve().parents[2]

# Minimal valid 1x1 8-bit grayscale PNG with verified CRCs
PNG_1X1 = bytes([
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, # signature
    0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, # IHDR length & type
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, # 1x1
    0x08, 0x00, 0x00, 0x00, 0x00, 0x3a, 0x7e, 0x9b, 0x55, # 8-bit gray + crc
    0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, # IDAT length & type
    0x78, 0x9c, 0x63, 0x60, 0x00, 0x00, 0x00, 0x02, 0x00, 0x01, 0x73, 0x75, 0x01, 0x18, # crc
    0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82, # IEND
])

def generate_txt():
    content = "Neighborhood Repair Cafe\n\nNotes from the community workshop held on 12 April 2026.\n"
    data = content.encode("utf-8")
    
    txt_fixture = ROOT / "fixtures/txt/sample.txt"
    txt_fixture.parent.mkdir(parents=True, exist_ok=True)
    txt_fixture.write_bytes(data)
    
    txt_seed = ROOT / "fuzz/seeds/reader_text/simple.txt"
    txt_seed.parent.mkdir(parents=True, exist_ok=True)
    txt_seed.write_bytes(data)

def generate_csv():
    content = "Item,Category,Status\nDesk lamp,Lighting,Repaired\nToaster,Kitchen,Inspected\nRadio,Audio,Pending\n"
    data = content.encode("utf-8")
    
    csv_fixture = ROOT / "fixtures/csv/sample.csv"
    csv_fixture.parent.mkdir(parents=True, exist_ok=True)
    csv_fixture.write_bytes(data)
    
    csv_seed = ROOT / "fuzz/seeds/reader_csv/simple.csv"
    csv_seed.parent.mkdir(parents=True, exist_ok=True)
    csv_seed.write_bytes(data)

def generate_eml():
    content = (
        "From: Alice Walker <alice@example.com>\r\n"
        "To: Bob Smith <bob@example.com>\r\n"
        "Subject: Project Update\r\n"
        "Date: Wed, 15 Apr 2026 10:30:00 +0000\r\n"
        "MIME-Version: 1.0\r\n"
        "Content-Type: multipart/mixed; boundary=\"boundary-12345\"\r\n"
        "\r\n"
        "--boundary-12345\r\n"
        "Content-Type: text/plain; charset=\"utf-8\"\r\n"
        "\r\n"
        "Hello Bob,\r\n"
        "\r\n"
        "Here is the latest progress report on the document pipeline.\r\n"
        "\r\n"
        "Best regards,\r\n"
        "Alice\r\n"
        "\r\n"
        "--boundary-12345\r\n"
        "Content-Type: application/octet-stream; name=\"report.pdf\"\r\n"
        "Content-Disposition: attachment; filename=\"report.pdf\"\r\n"
        "Content-Transfer-Encoding: base64\r\n"
        "\r\n"
        "JVBERi0xLjQKJcTl8uXrp420ced90=\r\n"
        "\r\n"
        "--boundary-12345--\r\n"
    )
    data = content.encode("utf-8")
    
    eml_fixture = ROOT / "fixtures/email/sample.eml"
    eml_fixture.parent.mkdir(parents=True, exist_ok=True)
    eml_fixture.write_bytes(data)
    
    eml_seed = ROOT / "fuzz/seeds/reader_eml/simple.eml"
    eml_seed.parent.mkdir(parents=True, exist_ok=True)
    eml_seed.write_bytes(data)

def generate_svg():
    content = (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100" width="100" height="100">\n'
        '  <circle cx="50" cy="50" r="40" fill="#4a90e2" stroke="#202c35" stroke-width="2"/>\n'
        '  <text x="50" y="55" font-size="12" text-anchor="middle" fill="#ffffff">AriadShift</text>\n'
        '</svg>\n'
    )
    data = content.encode("utf-8")
    
    svg_fixture = ROOT / "fixtures/svg/sample.svg"
    svg_fixture.parent.mkdir(parents=True, exist_ok=True)
    svg_fixture.write_bytes(data)
    
    svg_seed = ROOT / "fuzz/seeds/reader_svg/simple.svg"
    svg_seed.parent.mkdir(parents=True, exist_ok=True)
    svg_seed.write_bytes(data)

def generate_cbz():
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED) as zf:
        zf.writestr("page1.png", PNG_1X1)
        zf.writestr("page2.png", PNG_1X1)
        zf.writestr("page10.png", PNG_1X1)
    data = buf.getvalue()
    
    cbz_fixture = ROOT / "fixtures/cbz/sample.cbz"
    cbz_fixture.parent.mkdir(parents=True, exist_ok=True)
    cbz_fixture.write_bytes(data)

def main():
    generate_txt()
    generate_csv()
    generate_eml()
    generate_svg()
    generate_cbz()
    print("Generated TXT, CSV, EML, SVG, CBZ fixtures and seeds.")

if __name__ == "__main__":
    main()
