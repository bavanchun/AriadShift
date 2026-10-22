"""Download and verify the pinned inputs of the docling pack.

Every download is checked against the size and SHA-256 in ``pins.toml`` and stored once in a
content-addressed cache, by default ``~/.cache/ariadshift/docling``. Installing assets links or
copies verified blobs into a destination laid out like an installed pack::

    <dest>/models/...      Docling's artifacts_path
    <dest>/tessdata/...    TESSDATA_PREFIX

Usage::

    python fetch.py assets --dest DIR [--cache DIR]
    python fetch.py python --target TRIPLE [--cache DIR]
"""

from __future__ import annotations

import argparse
import hashlib
import os
import shutil
import sys
import tempfile
import time
import tomllib
import urllib.request
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from typing import Any

PINS = Path(__file__).with_name("pins.toml")
CHUNK = 1 << 20
ATTEMPTS = 3
TIMEOUT_S = 60
USER_AGENT = "ariadshift-pack-builder"


@dataclass(frozen=True)
class Pinned:
    """One pinned file: where to get it, what it must hash to, where it goes."""

    url: str
    sha256: str
    size: int
    dest: PurePosixPath


def load_pins(path: Path = PINS) -> dict[str, Any]:
    with path.open("rb") as handle:
        return tomllib.load(handle)


def default_cache() -> Path:
    return Path.home() / ".cache" / "ariadshift" / "docling"


def _relative(value: str) -> PurePosixPath:
    path = PurePosixPath(value)
    if path.is_absolute() or ".." in path.parts or not path.parts:
        raise ValueError(f"pinned path must be relative and stay inside its directory: {value}")
    return path


def model_files(pins: dict[str, Any]) -> list[Pinned]:
    """Model files relative to ``models/``."""
    files = []
    for model in pins["models"]:
        for item in model["files"]:
            url = item.get("url") or (
                f"{model['source']}/resolve/{model['revision']}/{item['path']}"
            )
            files.append(
                Pinned(
                    url=url,
                    sha256=item["sha256"],
                    size=item["size"],
                    dest=_relative(model["dest"]) / _relative(item["path"]),
                )
            )
    return files


def tessdata_files(pins: dict[str, Any]) -> list[Pinned]:
    """OCR data files relative to ``tessdata/``."""
    return [
        Pinned(
            url=item["url"],
            sha256=item["sha256"],
            size=item["size"],
            dest=_relative(item["path"]),
        )
        for item in pins["tessdata"]["files"]
    ]


def python_archive(pins: dict[str, Any], target: str) -> Pinned:
    targets = pins["python"]["targets"]
    if target not in targets:
        raise SystemExit(f"no pinned Python runtime for {target}; known: {', '.join(targets)}")
    entry = targets[target]
    name = entry["url"].rsplit("/", 1)[-1]
    return Pinned(
        url=entry["url"], sha256=entry["sha256"], size=entry["size"], dest=_relative(name)
    )


def fetch(item: Pinned, cache: Path) -> Path:
    """Returns the verified blob for ``item``, downloading it when the cache lacks it."""
    blobs = cache / "blobs"
    blobs.mkdir(parents=True, exist_ok=True)
    blob = blobs / item.sha256
    if blob.is_file() and blob.stat().st_size == item.size and _sha256(blob) == item.sha256:
        return blob
    last_error: Exception | None = None
    for attempt in range(1, ATTEMPTS + 1):
        try:
            _download(item, blob)
            return blob
        except (OSError, ValueError) as error:
            last_error = error
            print(f"download failed ({attempt}/{ATTEMPTS}): {item.url}: {error}", file=sys.stderr)
            time.sleep(2 * attempt)
    raise SystemExit(f"could not download {item.url}: {last_error}")


def _download(item: Pinned, blob: Path) -> None:
    if not item.url.startswith("https://"):
        raise ValueError("pinned downloads must use https")
    request = urllib.request.Request(item.url, headers={"User-Agent": USER_AGENT})  # noqa: S310
    digest = hashlib.sha256()
    received = 0
    handle, temporary = tempfile.mkstemp(dir=blob.parent, prefix=".partial-")
    try:
        with (
            os.fdopen(handle, "wb") as output,
            urllib.request.urlopen(  # noqa: S310
                request, timeout=TIMEOUT_S
            ) as response,
        ):
            while chunk := response.read(CHUNK):
                received += len(chunk)
                if received > item.size:
                    raise ValueError(f"more than the pinned {item.size} bytes")
                digest.update(chunk)
                output.write(chunk)
        if received != item.size:
            raise ValueError(f"received {received} bytes, pinned {item.size}")
        if digest.hexdigest() != item.sha256:
            raise ValueError("SHA-256 does not match the pin")
        os.replace(temporary, blob)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(CHUNK):
            digest.update(chunk)
    return digest.hexdigest()


def place(blob: Path, destination: Path, *, copy: bool) -> None:
    """Puts a verified blob at ``destination``, hard-linking when allowed and possible."""
    destination.parent.mkdir(parents=True, exist_ok=True)
    if destination.exists() or destination.is_symlink():
        destination.unlink()
    if not copy:
        try:
            os.link(blob, destination)
            return
        except OSError:
            pass
    shutil.copyfile(blob, destination)


def install_assets(pins: dict[str, Any], dest: Path, cache: Path, *, copy: bool = False) -> None:
    """Installs the pinned models and OCR data under ``dest/models`` and ``dest/tessdata``."""
    for directory, files in (("models", model_files(pins)), ("tessdata", tessdata_files(pins))):
        root = dest / directory
        expected = {root / Path(*item.dest.parts) for item in files}
        if root.is_symlink():
            root.unlink()
        for item in files:
            place(fetch(item, cache), root / Path(*item.dest.parts), copy=copy)
        # Files that are no longer pinned must not linger in a development install.
        for path in sorted(root.rglob("*"), reverse=True):
            if path.is_file() and path not in expected:
                path.unlink()
            elif path.is_dir() and not any(path.iterdir()):
                path.rmdir()


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--cache", type=Path, default=default_cache())
    commands = parser.add_subparsers(dest="command", required=True)
    assets = commands.add_parser("assets", help="install models and tessdata")
    assets.add_argument("--dest", type=Path, required=True)
    assets.add_argument("--copy", action="store_true", help="copy instead of hard-linking")
    runtime = commands.add_parser("python", help="fetch the standalone Python archive")
    runtime.add_argument("--target", required=True)
    args = parser.parse_args(argv)

    pins = load_pins()
    if args.command == "assets":
        install_assets(pins, args.dest, args.cache, copy=args.copy)
        print(f"installed models and tessdata into {args.dest}")
    else:
        print(fetch(python_archive(pins, args.target), args.cache))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
