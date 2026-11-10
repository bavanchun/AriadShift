#!/usr/bin/env python3
"""Download and hash-check the pinned sources and bundled model files."""
from __future__ import annotations

import hashlib
from pathlib import Path
import sys
import tempfile
import tomllib
from urllib.request import HTTPRedirectHandler, build_opener

ROOT = Path(__file__).resolve().parents[3]
MAX_BYTES = 128 * 1024 * 1024


class HttpsRedirects(HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        if not newurl.startswith("https://"):
            raise ValueError("redirect must use HTTPS")
        return super().redirect_request(req, fp, code, msg, headers, newurl)


def fetch(url: str, expected: str, destination: Path, size: int | None = None) -> None:
    if not url.startswith("https://"):
        raise ValueError("input URL must use HTTPS")
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=destination.parent, delete=False) as out:
        temporary = Path(out.name)
        try:
            digest = hashlib.sha256()
            length = 0
            with build_opener(HttpsRedirects).open(url, timeout=120) as response:
                if not response.geturl().startswith("https://"):
                    raise ValueError("redirect did not use HTTPS")
                while chunk := response.read(1024 * 1024):
                    length += len(chunk)
                    if length > MAX_BYTES:
                        raise ValueError("input exceeds size limit")
                    digest.update(chunk)
                    out.write(chunk)
            if digest.hexdigest() != expected or (size is not None and length != size):
                raise ValueError(f"hash or size mismatch for {url}")
        except BaseException:
            temporary.unlink(missing_ok=True)
            raise
    temporary.replace(destination)


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: fetch-inputs.py OUTPUT_DIR")
    output = Path(sys.argv[1])
    pins = tomllib.loads((ROOT / "engines/tesseract/pack/pins.toml").read_text())
    for name in ("tesseract", "leptonica"):
        entry = pins[name]
        fetch(entry["url"], entry["sha256"], output / f"{name}.tar.gz", entry.get("size"))
    files = tomllib.loads((ROOT / "engines/docling/pack/pins.toml").read_text())["tessdata"]["files"]
    for entry in files:
        if entry["path"] in {"vie.traineddata", "eng.traineddata", "osd.traineddata", "LICENSE"}:
            fetch(entry["url"], entry["sha256"], output / "tessdata" / entry["path"], entry["size"])


if __name__ == "__main__":
    main()
