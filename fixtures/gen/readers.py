"""Generate deterministic TXT, CSV, EML, MSG, SVG, and CBZ fixtures and fuzz seeds."""
from pathlib import Path
import base64
import io
import math
import struct
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

# Deterministic zeroed-timestamp MSG compound files for fuzz seeds (zlib compressed)
SIMPLE_MSG = "eNrtms1Kw0AURr+k6qoL3Yi4kK5FSoILd4KI4kI3FtfSolShP9JE0MfxEXwSfQR9EBNvYgJtoXQUrWjPuczNTZjJmdBuZpLnp5WXh8f1V42xqwUlaVVLxbk31HKWJb84T9I0LS+n8KcQAAAAAAAAzAVvtgZMhtos1pi/vcbFjx8/fvz48ePHjx8/fvz4581/qr5FrJoO1LPjQPef2j9Y1aJX3st3HFPJUvEhwblFpFu1LMdqK1RdgV0LLLa1kx9DHU6816Z8feV99oam+fet3nPxe17xTK7+rP/aFH+Ym4Mfef7sdzr+pv0jf+z/NOv9K/z48ePHjx8/fvz48ePH70b53r9S1Oy/4MePHz9+/Pjx48ePHz9+/P/P39C1urpRR5eq6cRyZNFU2yoXGtavpwvLAxt/ZmOzyp0j69+x6GvLxse6svlEVn3kprVoZIax5bv8e4XuyFzrAgAAAAAAAIBJvAPFOjuR"
PROPERTIES_MSG = "eNrtmj9OwzAYR38JfyYGWBiYMjEwVIkYEAvqAhsS4gIIpKowQCoSQByHI3ADbgBHgIOQ8MWkUiilMgVaob5nJZ/tOn522sV2nx5Xnu/u1140wI7mVZRLWqzLQeNyLEthXS7KsuxXl/CvEAAAAAAAAMwEr7YGLBrXJNaY017j4sePHz9+/Pjx48ePHz9+/LPmP1RqKVekXV1YvNTtt/YPVrUQ9Pua83ymavdQ548sZbrSid1zdZWopdjqYkub2nIx0d6XfW0o1Djn2esNf89mndq9YzHXmcXMaq9dObNyam/mfVyficfwB+650fNP3Mxjn/kHVX9h/b36+Ku27V/aPwoHfk+T3r/Cjx8/fvz48ePHjx8/fvx+/PXZP/sv+PHjx48fP378+PHjx48f//T9B0POviPtu5jpWF3L/YS2tkd+/tEU6cbGkOvUcr2hI8vcfxQ61v5cLQEAAAAAAACAD29HQEAv"
RTF_MSG = "eNrt2b1Kw1AchvE3qR9QHHQRcQo6CA6SpIi4uJmpLuIkgqhINpU2Hdy8BPEOvASvRC9BN3dnc/wbUigFNQVtLX2ew2k+oPn1Y8rJ0+PC8/3D8ov62tGUcjenmfLY65lF85JfHufOue5pR2OViIiIiIiIaCJ6t3vAvGcO4x5z1Pe4+Pj4+Pj4+Pj4+Pj4+PiT5u/r0kamQLu6sG1L1wOtHyxq2utey6v4nprNm3L/2EZbHZ3aa6ZUkTYU2rnQRkNbxTZS8uW11uUXvl9+r6qfe+kHPyrk7UIPFX/na9Dn6Z+/09ovrd/4ff/nsNeP8PHx8fHx8fHx8fHx8cfD/8vn/qy/4OPj4+Pj4+Pj4+Pj4+P/l+f/B0oUaE/nats4UWp71Vu1uWKzeZh0GkevWU11tc6u0ijevIvrb0E2e5uJiIiIiIiIiEbaB+huM/Y="
ATTACHMENTS_MSG = "eNrtmsFOwjAch39D8ORBD3rgRGJijAfCQOPFmHDQmxd9ADNhColAIsPo4/gIPok+gj6IzD9jkIUEMgnBKN/XtFsL7beu26Ht3t+2Pl5e85+a4FRZ9cMNrcd5JxEjNqVMnO+HYTgqDuFPIQAAAAAAAFgJvmwO2E/EZcwxf3uOix8/fvz48ePHjx8/fvz48a+a/1IdC4EKOlPbjg96/tH6wY5yzqitTMo6a4Mk/pDg2kJXPd1YGuhOrooqWVnJQkXH0dHV+dS2DpTRPPvZe5rtdyNzKY3fceI+pfUP/r+f8HtmDiytqWG5R/k2Cl01bVza4+vZTVzPMAw5sdZGfc/Osf4zrf/De5/q/ju5xDOVxjl4TvKp/G4cywsd/8FYbS9o/Swz8T4te/0OP378+PHjx48fP378+NPSZ/8fP378+PHjx48fP378+PHj//f+amLvuSU/+gagoAs761rwdGdns7iy332rkdzD9lW3kls1dR+1M4u6OlanN3YXLT5ZTEs1CLxao+W3g0LdC7yCW64cHgkAAAAAAAAAknwDXp5Jqg=="


def _utf16le_z(text: str) -> bytes:
    """Encode a string as null-terminated UTF-16LE."""
    return (text + "\0").encode("utf-16-le")


def _cfb_name_key(name: str) -> tuple[int, str]:
    """MS-CFB §2.6.4 sibling ordering key: UTF-16 code unit length, then uppercase."""
    return (len(name.encode("utf-16-le")) // 2, name.upper())


def _pack_cfb_dir_entry(
    name: str,
    obj_type: int,
    color: int,
    left_sibling: int,
    right_sibling: int,
    child: int,
    start_sector: int,
    stream_len: int,
) -> bytes:
    """Pack a 128-byte MS-CFB directory entry with zeroed CLSID and timestamps."""
    if obj_type == 0:
        return struct.pack(
            "<64sHBBIII16sIQQIQ",
            b"",
            0,
            0,
            0,
            0xFFFFFFFF,
            0xFFFFFFFF,
            0xFFFFFFFF,
            b"\x00" * 16,
            0,
            0,
            0,
            0,
            0,
        )
    name_utf16 = name.encode("utf-16-le")
    name_len_bytes = len(name_utf16) + 2
    assert name_len_bytes <= 64
    return struct.pack(
        "<64sHBBIII16sIQQIQ",
        name_utf16.ljust(64, b"\x00"),
        name_len_bytes,
        obj_type,
        color,
        left_sibling,
        right_sibling,
        child,
        b"\x00" * 16,
        0,
        0,
        0,
        start_sector,
        stream_len,
    )


def build_sample_msg() -> bytes:
    """Construct fixtures/email/sample.msg as a deterministic MS-CFB v4 container."""
    sector_size = 4096
    mini_sector_size = 64
    no_stream = 0xFFFFFFFF
    end_of_chain = 0xFFFFFFFE
    fat_sect = 0xFFFFFFFD

    # 2026-04-15T10:30:00Z in Windows FILETIME (100ns intervals since 1601-01-01).
    submit_filetime = 134_207_226_000_000_000
    root_props = bytes(32) + struct.pack(
        "<HHIq",
        0x0040,  # PT_SYSTIME
        0x0039,  # PR_CLIENT_SUBMIT_TIME
        0,
        submit_filetime,
    )
    recip_props = bytes(8) + struct.pack(
        "<HHIII",
        0x0003,  # PT_LONG
        0x0C15,  # PR_RECIPIENT_TYPE
        0,
        1,       # MAPI_TO
        0,
    )

    tree = {
        "__substg1.0_0037001F": _utf16le_z("Project Update"),
        "__substg1.0_0C1A001F": _utf16le_z("Alice Walker"),
        "__substg1.0_0C1F001F": _utf16le_z("alice@example.com"),
        "__substg1.0_1000001F": _utf16le_z(
            "Hello Bob,\n\n"
            "Here is the latest progress report on the document pipeline.\n\n"
            "Best regards,\n"
            "Alice"
        ),
        "__properties_version1.0": root_props,
        "__recip_version1.0_#00000000": {
            "__properties_version1.0": recip_props,
            "__substg1.0_3001001F": _utf16le_z("Bob Smith"),
            "__substg1.0_3003001F": _utf16le_z("bob@example.com"),
        },
        "__attach_version1.0_#00000000": {
            "__substg1.0_3707001F": _utf16le_z("report.pdf"),
            "__substg1.0_37010102": b"%PDF-1.4 dummy pdf attachment",
        },
    }

    mini_stream = bytearray()
    minifat: list[int] = []

    def alloc_mini_stream(payload: bytes) -> tuple[int, int]:
        if not payload:
            return (end_of_chain, 0)
        n_sectors = math.ceil(len(payload) / mini_sector_size)
        first_sec = len(minifat)
        for i in range(n_sectors):
            nxt = first_sec + i + 1 if i + 1 < n_sectors else end_of_chain
            minifat.append(nxt)
        mini_stream.extend(payload.ljust(n_sectors * mini_sector_size, b"\x00"))
        return (first_sec, len(payload))

    # Entry 0 is always Root Entry; populate children recursively.
    entries: list[dict] = [
        {
            "name": "Root Entry",
            "obj_type": 5,
            "color": 1,
            "left": no_stream,
            "right": no_stream,
            "child": no_stream,
            "start": 0,
            "size": 0,
        }
    ]

    def build_storage_children(children_map: dict) -> int:
        sorted_items = sorted(children_map.items(), key=lambda kv: _cfb_name_key(kv[0]))
        n = len(sorted_items)
        if n == 0:
            return no_stream
        max_depth = int(math.log2(n))
        node_indices: list[int] = []
        for name, val in sorted_items:
            idx = len(entries)
            node_indices.append(idx)
            if isinstance(val, dict):
                entries.append(
                    {
                        "name": name,
                        "obj_type": 1,
                        "color": 1,
                        "left": no_stream,
                        "right": no_stream,
                        "child": no_stream,
                        "start": 0,
                        "size": 0,
                    }
                )
            else:
                start_sec, size = alloc_mini_stream(val)
                entries.append(
                    {
                        "name": name,
                        "obj_type": 2,
                        "color": 1,
                        "left": no_stream,
                        "right": no_stream,
                        "child": no_stream,
                        "start": start_sec,
                        "size": size,
                    }
                )

        for (name, val), idx in zip(sorted_items, node_indices):
            if isinstance(val, dict):
                entries[idx]["child"] = build_storage_children(val)

        def build_bst(lo: int, hi: int, depth: int) -> int:
            if lo >= hi:
                return no_stream
            mid = (lo + hi) // 2
            ent_idx = node_indices[mid]
            # Nodes above max_depth are Black (1); deepest level leaves (depth == max_depth > 0)
            # are Red (0), guaranteeing equal black-height on all paths and no adjacent Red nodes.
            entries[ent_idx]["color"] = 0 if (max_depth > 0 and depth == max_depth) else 1
            entries[ent_idx]["left"] = build_bst(lo, mid, depth + 1)
            entries[ent_idx]["right"] = build_bst(mid + 1, hi, depth + 1)
            return ent_idx

        return build_bst(0, n, 0)

    entries[0]["child"] = build_storage_children(tree)
    entries[0]["start"] = 3
    entries[0]["size"] = len(mini_stream)

    assert len(entries) <= 32, "single V4 directory sector holds up to 32 entries"
    assert len(minifat) <= 1024, "single V4 MiniFAT sector holds up to 1024 entries"

    num_mini_container_sectors = max(1, math.ceil(len(mini_stream) / sector_size))

    # Header (4096 bytes): 512-byte CFB header + 3584 zero bytes.
    difat = [0] + [no_stream] * 108
    header_512 = struct.pack(
        "<8s16s5H6s9I109I",
        b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1",
        b"\x00" * 16,
        0x003E,         # minor version
        0x0004,         # major version (V4: 4096-byte sectors)
        0xFFFE,         # byte order
        12,             # sector shift (1 << 12 = 4096)
        6,              # mini sector shift (1 << 6 = 64)
        b"\x00" * 6,    # reserved
        1,              # number of directory sectors
        1,              # number of FAT sectors
        1,              # first directory sector location
        0,              # transaction signature
        4096,           # mini stream cutoff size
        2,              # first MiniFAT sector location
        1,              # number of MiniFAT sectors
        end_of_chain,   # first DIFAT sector location
        0,              # number of DIFAT sectors
        *difat,
    )
    header_sector = header_512.ljust(sector_size, b"\x00")

    # Sector 0: FAT
    fat = [no_stream] * 1024
    fat[0] = fat_sect       # Sector 0: FAT
    fat[1] = end_of_chain   # Sector 1: Directory
    fat[2] = end_of_chain   # Sector 2: MiniFAT
    for i in range(num_mini_container_sectors):
        sec_id = 3 + i
        fat[sec_id] = sec_id + 1 if i + 1 < num_mini_container_sectors else end_of_chain
    fat_sector = struct.pack("<1024I", *fat)

    # Sector 1: Directory (32 entries of 128 bytes)
    dir_chunks = [
        _pack_cfb_dir_entry(
            e["name"],
            e["obj_type"],
            e["color"],
            e["left"],
            e["right"],
            e["child"],
            e["start"],
            e["size"],
        )
        for e in entries
    ]
    unallocated_entry = _pack_cfb_dir_entry("", 0, 0, no_stream, no_stream, no_stream, 0, 0)
    dir_chunks.extend([unallocated_entry] * (32 - len(entries)))
    dir_sector = b"".join(dir_chunks)

    # Sector 2: MiniFAT
    minifat_padded = minifat + [no_stream] * (1024 - len(minifat))
    minifat_sector = struct.pack("<1024I", *minifat_padded)

    # Sector 3..: Mini-stream container
    mini_container = bytes(mini_stream).ljust(num_mini_container_sectors * sector_size, b"\x00")

    return header_sector + fat_sector + dir_sector + minifat_sector + mini_container

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

# LZFu stream from the MSG reader's unit test; CRC 0x74e85c33 matches its CONTENTS.
RTF_LZFU = (
    b'#\x00\x00\x00"\x00\x00\x00LZFu3\\\xe8t\x03\x00\n\x00rcpg125\x922\n\xf3 t\x07\x90t}\x0f\x10'
)
RTF_STREAM_NAME = "__substg1.0_10090102".encode("utf-16-le")


def with_rtf_stream(container, stream):
    """Replace the RTF body stream of an MSG compound file in place.

    The stream lives in a single 64-byte mini sector, so the new bytes are
    written at its offset and the directory entry's 32-bit size is updated.
    """
    data = bytearray(container)
    start = data.index(RTF_LZFU[:16])
    assert len(stream) <= 64 and start % 64 == 0
    data[start:start + 64] = stream.ljust(64, b"\x00")
    entry = data.index(RTF_STREAM_NAME)
    assert entry % 128 == 0
    data[entry + 0x78:entry + 0x7C] = len(stream).to_bytes(4, "little")
    return bytes(data)


def generate_msg():
    sample_data = build_sample_msg()
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

    # The stored RTF seed truncates its LZFu stream; write the full stream and
    # a twin that differs only by a wrong CRC.
    rtf_base = zlib.decompress(base64.b64decode(RTF_MSG))
    bad_crc = RTF_LZFU[:12] + bytes(b ^ 0xFF for b in RTF_LZFU[12:16]) + RTF_LZFU[16:]
    (seed_dir / "rtf.msg").write_bytes(with_rtf_stream(rtf_base, RTF_LZFU))
    (seed_dir / "rtf-bad-crc.msg").write_bytes(with_rtf_stream(rtf_base, bad_crc))

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
