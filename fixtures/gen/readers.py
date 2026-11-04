"""Generate deterministic TXT, CSV, EML, MSG, SVG, and CBZ fixtures and fuzz seeds."""
from pathlib import Path
import base64
import io
import zipfile
import zlib

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

# Deterministic zeroed-timestamp MSG compound files (zlib compressed)
SAMPLE_MSG = "eNrtm99qE0EYxU/SNtbihYJ4UVAC6o1o6JqCICK2auhlUcRLyZ9tsrpJlt2N2DtfxUfw3nfQR9AHMfHMdJMsQs1siKDt+Q2zmdnMN+ebbzaBzEy+fb3y/dPn7R/4jcdYx3hyCZWsXsply2WgnNXHk8lkensi/isghBBCCCGEEOJc8JO/Ace5LIQQQgghhBBCiLPHCwyZUlTxHAO+xjgutH5wDRulaV9lR5s15o9Z4zdMCUZo8ZqiCw817PDeDlMdD+yrh8apfd1BuZQfj6vfN7BI/ynLe0765gzEhezsg4u2ab/tpN/4K+M3ob+V04/ho40AEcvvWY7pS8CnYjDz5qb1I5+mPETJ6m+yvLHE+tNp469nI3cdf6VA/M3zd9VJv+6ij6LxX2euLtD3crFepL+W9emqb+bpS06/SeWU1zZ6hZ+AR3yap2O/uNL5N599t8//Fubnjlw0N+33lou+l+X7K51/E6frK1q/LQNY5vsP0pe+9KUvfelLX/rSP5f6+b3/aa5kv2d1LkAIIYQQQgghhDgbHCLGEG/tDrA5BfAKETp2R9B3Wj/YQ4iAtj5tX9MuxDu7f+hKc2b/hPkD6316ELJc490ha39mn21a1H7JlgG97hVc/2hZ++W0DQdsHTIN6cOJL3exxXRgo2CiEiDh1XhmamEW28RGO7LR79qWiW1nShHvxfZ9swM7t+2w3saIfvn2rIaxN7v2vo3hwPpttPdn/ZveulSMaZtYz+bzdUJer2bn/qhQ/G4fPmvc82q71c6o3z+uRp2jajNNm+1e3x+k+nuNEEIIIYQQQvwz/AJlentm"
SIMPLE_MSG = "eNrtms1Kw0AURr+k6qoL3Yi4kK5FSoILd4KI4kI3FtfSolShP9JE0MfxEXwSfQR9EBNvYgJtoXQUrWjPuczNTZjJmdBuZpLnp5WXh8f1V42xqwUlaVVLxbk31HKWJb84T9I0LS+n8KcQAAAAAAAAzAVvtgZMhtos1pi/vcbFjx8/fvz48ePHjx8/fvz4581/qr5FrJoO1LPjQPef2j9Y1aJX3st3HFPJUvEhwblFpFu1LMdqK1RdgV0LLLa1kx9DHU6816Z8feV99oam+fet3nPxe17xTK7+rP/aFH+Ym4Mfef7sdzr+pv0jf+z/NOv9K/z48ePHjx8/fvz48ePH70b53r9S1Oy/4MePHz9+/Pjx48ePHz9+/P/P39C1urpRR5eq6cRyZNFU2yoXGtavpwvLAxt/ZmOzyp0j69+x6GvLxse6svlEVn3kprVoZIax5bv8e4XuyFzrAgAAAAAAAIBJvAPFOjuR"
PROPERTIES_MSG = "eNrtmj9OwzAYR38JfyYGWBiYMjEwVIkYEAvqAhsS4gIIpKowQCoSQByHI3ADbgBHgIOQ8MWkUiilMgVaob5nJZ/tOn522sV2nx5Xnu/u1140wI7mVZRLWqzLQeNyLEthXS7KsuxXl/CvEAAAAAAAAMwEr7YGLBrXJNaY017j4sePHz9+/Pjx48ePHz9+/LPmP1RqKVekXV1YvNTtt/YPVrUQ9Pua83ymavdQ548sZbrSid1zdZWopdjqYkub2nIx0d6XfW0o1Djn2esNf89mndq9YzHXmcXMaq9dObNyam/mfVyficfwB+650fNP3Mxjn/kHVX9h/b36+Ku27V/aPwoHfk+T3r/Cjx8/fvz48ePHjx8/fvx+/PXZP/sv+PHjx48fP378+PHjx48f//T9B0POviPtu5jpWF3L/YS2tkd+/tEU6cbGkOvUcr2hI8vcfxQ61v5cLQEAAAAAAACAD29HQEAv"
RTF_MSG = "eNrt2b1Kw1AchvE3qR9QHHQRcQo6CA6SpIi4uJmpLuIkgqhINpU2Hdy8BPEOvASvRC9BN3dnc/wbUigFNQVtLX2ew2k+oPn1Y8rJ0+PC8/3D8ov62tGUcjenmfLY65lF85JfHufOue5pR2OViIiIiIiIaCJ6t3vAvGcO4x5z1Pe4+Pj4+Pj4+Pj4+Pj4+PiT5u/r0kamQLu6sG1L1wOtHyxq2utey6v4nprNm3L/2EZbHZ3aa6ZUkTYU2rnQRkNbxTZS8uW11uUXvl9+r6qfe+kHPyrk7UIPFX/na9Dn6Z+/09ovrd/4ff/nsNeP8PHx8fHx8fHx8fHx8cfD/8vn/qy/4OPj4+Pj4+Pj4+Pj4+P/l+f/B0oUaE/nats4UWp71Vu1uWKzeZh0GkevWU11tc6u0ijevIvrb0E2e5uJiIiIiIiIiEbaB+huM/Y="
ATTACHMENTS_MSG = "eNrtmsFOwjAch39D8ORBD3rgRGJijAfCQOPFmHDQmxd9ADNhColAIsPo4/gIPok+gj6IzD9jkIUEMgnBKN/XtFsL7beu26Ht3t+2Pl5e85+a4FRZ9cMNrcd5JxEjNqVMnO+HYTgqDuFPIQAAAAAAAFgJvmwO2E/EZcwxf3uOix8/fvz48ePHjx8/fvz48a+a/1IdC4EKOlPbjg96/tH6wY5yzqitTMo6a4Mk/pDg2kJXPd1YGuhOrooqWVnJQkXH0dHV+dS2DpTRPPvZe5rtdyNzKY3fceI+pfUP/r+f8HtmDiytqWG5R/k2Cl01bVza4+vZTVzPMAw5sdZGfc/Osf4zrf/De5/q/ju5xDOVxjl4TvKp/G4cywsd/8FYbS9o/Swz8T4te/0OP378+PHjx48fP378+NPSZ/8fP378+PHjx48fP378+PHj//f+amLvuSU/+gagoAs761rwdGdns7iy332rkdzD9lW3kls1dR+1M4u6OlanN3YXLT5ZTEs1CLxao+W3g0LdC7yCW64cHgkAAAAAAAAAknwDXp5Jqg=="

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

def generate_msg():
    sample_data = zlib.decompress(base64.b64decode(SAMPLE_MSG))
    msg_fixture = ROOT / "fixtures/email/sample.msg"
    msg_fixture.parent.mkdir(parents=True, exist_ok=True)
    msg_fixture.write_bytes(sample_data)

    seeds = {
        "simple.msg": SIMPLE_MSG,
        "properties.msg": PROPERTIES_MSG,
        "rtf.msg": RTF_MSG,
        "attachments.msg": ATTACHMENTS_MSG,
    }
    seed_dir = ROOT / "fuzz/seeds/reader_msg"
    seed_dir.mkdir(parents=True, exist_ok=True)
    for filename, b64_payload in seeds.items():
        (seed_dir / filename).write_bytes(zlib.decompress(base64.b64decode(b64_payload)))

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
        for name in ["page1.png", "page2.png", "page10.png"]:
            zinfo = zipfile.ZipInfo(name, (2026, 11, 4, 12, 53, 56))
            zinfo.compress_type = zipfile.ZIP_DEFLATED
            zinfo.external_attr = 25165824
            zf.writestr(zinfo, PNG_1X1)
    data = buf.getvalue()
    
    cbz_fixture = ROOT / "fixtures/cbz/sample.cbz"
    cbz_fixture.parent.mkdir(parents=True, exist_ok=True)
    cbz_fixture.write_bytes(data)

    cbz_seed = ROOT / "fuzz/seeds/reader_cbz/simple.cbz"
    cbz_seed.parent.mkdir(parents=True, exist_ok=True)
    cbz_seed.write_bytes(data)

def main():
    generate_txt()
    generate_csv()
    generate_eml()
    generate_msg()
    generate_svg()
    generate_cbz()
    print("Generated TXT, CSV, EML, MSG, SVG, CBZ fixtures and seeds.")

if __name__ == "__main__":
    main()
