#!/usr/bin/env python3
"""Verify tessdata blobs at the pinned revision and emit the compiled language catalogue."""
from __future__ import annotations

import hashlib
import json
from pathlib import Path
import sys
from urllib.request import HTTPRedirectHandler, build_opener

REVISION = "e12c65a915945e4c28e237a9b52bc4a8f39a0cec"
LICENSE_SHA256 = "a6cba85bc92e0cff7a450b1d873c0eaa2e9fc96bf472df0247a26bec77bf3ff9"
MODELS = (
    "ara aze bul bod cat ces dan deu ell eng spa eus fas fra gle glg hin hrv hun ind ita "
    "jpn kor mal nor nld pol por ron rus slk slv srp_latn swe tha tur ukr vie chi_sim chi_tra"
).split()


class HttpsRedirects(HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        if not newurl.startswith("https://"):
            raise ValueError("redirect must use HTTPS")
        return super().redirect_request(req, fp, code, msg, headers, newurl)


def get(url: str, cap: int) -> bytes:
    if not url.startswith("https://"):
        raise ValueError("URL must use HTTPS")
    with build_opener(HttpsRedirects).open(url, timeout=120) as response:
        data = response.read(cap + 1)
    if len(data) > cap:
        raise ValueError(f"input exceeds size limit: {url}")
    return data


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: generate-catalogue.py OUTPUT_RS")
    root = f"https://raw.githubusercontent.com/tesseract-ocr/tessdata_best/{REVISION}"
    license_bytes = get(f"{root}/LICENSE", 100_000)
    if hashlib.sha256(license_bytes).hexdigest() != LICENSE_SHA256:
        raise ValueError("tessdata licence hash mismatch")
    tree_url = f"https://api.github.com/repos/tesseract-ocr/tessdata_best/git/trees/{REVISION}"
    tree = json.loads(get(tree_url, 4 * 1024 * 1024))
    if tree.get("truncated"):
        raise ValueError("upstream tree is incomplete")
    entries = {item["path"]: item for item in tree["tree"] if item.get("type") == "blob"}
    records = []
    for model in sorted(MODELS):
        name = f"{model}.traineddata"
        entry = entries[name]
        size = entry["size"]
        if size > 128 * 1024 * 1024:
            raise ValueError(f"model too large: {name}")
        content = get(f"{root}/{name}", size)
        git_hash = hashlib.sha1(f"blob {len(content)}\0".encode() + content, usedforsecurity=False).hexdigest()
        if len(content) != size or git_hash != entry["sha"]:
            raise ValueError(f"model blob mismatch: {name}")
        records.append((model, hashlib.sha256(content).hexdigest(), size))
        print(f"verified {name}", file=sys.stderr)
    lines = [
        "//! Generated from the pinned Apache-2.0 tessdata_best revision; do not edit.",
        f'pub const REVISION: &str = "{REVISION}";',
        "pub const LANGUAGES: &[(&str, &str, u64)] = &[",
    ]
    lines.extend(f'    ("{name}", "{digest}", {size}),' for name, digest, size in records)
    lines.append("];\n")
    destination = Path(sys.argv[1])
    temporary = destination.with_suffix(".tmp")
    temporary.write_text("\n".join(lines), encoding="utf-8")
    temporary.replace(destination)


if __name__ == "__main__":
    main()
