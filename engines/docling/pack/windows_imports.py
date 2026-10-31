"""Retarget wheel-mangled PE imports to the installed Microsoft redistributable."""

from __future__ import annotations

import struct
from pathlib import Path


def retarget(path: Path, aliases: dict[bytes, bytes]) -> bool:
    """Shorten DLL names in the PE import tables without relocating any image data.

    The pinned Windows wheels are PE32+ images. Handle normal and delay-load imports;
    refuse unsupported layouts rather than leaving a dependency on a deleted DLL.
    """
    data = bytearray(path.read_bytes())
    if data[:2] != b"MZ":
        return False
    try:
        pe = struct.unpack_from("<I", data, 0x3C)[0]
        optional = pe + 24
        if data[pe : pe + 4] != b"PE\0\0" or struct.unpack_from("<H", data, optional)[0] != 0x20B:
            raise ValueError("expected a PE32+ image")
        count = struct.unpack_from("<H", data, pe + 6)[0]
        sections = optional + struct.unpack_from("<H", data, pe + 20)[0]

        def offset(rva: int) -> int:
            for section in range(sections, sections + 40 * count, 40):
                size, address, raw_size, raw = struct.unpack_from("<IIII", data, section + 8)
                if address <= rva < address + min(size, raw_size):
                    return raw + rva - address
            raise ValueError(f"unmapped import RVA {rva:#x}")

        changed = False
        for directory, stride, name_field in [(1, 20, 12), (13, 32, 4)]:
            rva, size = struct.unpack_from("<II", data, optional + 112 + 8 * directory)
            if not rva:
                continue
            table = offset(rva)
            for entry in range(table, table + size, stride):
                if not any(data[entry : entry + stride]):
                    break
                if directory == 13 and struct.unpack_from("<I", data, entry)[0] != 1:
                    raise ValueError("expected RVA-based delay imports")
                name = offset(struct.unpack_from("<I", data, entry + name_field)[0])
                end = data.index(0, name)
                old = bytes(data[name:end])
                if replacement := aliases.get(old.lower()):
                    if len(replacement) > len(old):
                        raise ValueError("replacement DLL name is longer")
                    data[name:end] = replacement.ljust(len(old), b"\0")
                    changed = True
        if changed:
            # Discard the previous optional checksum after changing the import names.
            struct.pack_into("<I", data, optional + 64, 0)
            path.write_bytes(data)
        return changed
    except (ValueError, struct.error) as error:
        raise SystemExit(f"cannot retarget Microsoft runtime imports in {path}: {error}") from error
