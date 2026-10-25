"""Verify cargo-dist's formula against every built archive before publishing."""

import hashlib
from pathlib import Path
import re
import sys
import tomllib


ROOT = Path(__file__).resolve().parents[2]
HOMEBREW_TARGETS = {
    "aarch64-apple-darwin": ("mac", "arm"),
    "x86_64-apple-darwin": ("mac", "intel"),
    "aarch64-unknown-linux-musl": ("linux", "arm"),
    "x86_64-unknown-linux-musl": ("linux", "intel"),
}


def verify(distrib: Path, root: Path = ROOT) -> None:
    config = tomllib.loads((root / "dist-workspace.toml").read_text())["dist"]
    version = tomllib.loads((root / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    formula = (distrib / f"{config['formula']}.rb").read_text()
    if not re.search(rf'^\s*version "{re.escape(version)}"\s*$', formula, re.M):
        raise ValueError("Homebrew formula version does not match the workspace")

    # Check archive bytes as well as the .sha256 sidecars, including Windows.
    expected_urls = set()
    for target in config["targets"]:
        extension = "zip" if "windows" in target else "tar.xz"
        name = f"ariad-cli-{target}.{extension}"
        archive = distrib / name
        with archive.open("rb") as source:
            digest = hashlib.file_digest(source, "sha256").hexdigest()
        checksum = (distrib / f"{name}.sha256").read_text().split()
        if not checksum or checksum[0].lower() != digest:
            raise ValueError(f"Archive checksum does not match built bytes: {name}")

        if "windows" in target:
            continue  # Homebrew runs on macOS and Linux; Windows uses the ZIP.
        if target not in HOMEBREW_TARGETS:
            raise ValueError(f"Add Homebrew platform verification for target: {target}")
        os_name, cpu = HOMEBREW_TARGETS[target]
        url = f"https://github.com/bavanchun/AriadShift/releases/download/v{version}/{name}"
        expected_urls.add(url)
        # Match the pinned cargo-dist template's OS/CPU branch, without executing Ruby.
        os_block = re.search(rf"^  if OS\.{os_name}\?\n(.*?)^  end$", formula, re.M | re.S)
        if os_block is None:
            raise ValueError(f"Missing Homebrew OS branch for {target}")
        cpu_block = re.search(
            rf"^    if Hardware::CPU\.{cpu}\?\n(.*?)^    end$",
            os_block[1], re.M | re.S,
        )
        if cpu_block is None:
            raise ValueError(f"Missing Homebrew CPU branch for {target}")
        pair = rf'^\s*url "{re.escape(url)}"\n\s*sha256 "{digest}"\s*$'
        if not re.search(pair, cpu_block[1], re.M):
            raise ValueError(f"Homebrew URL or SHA-256 does not match built archive: {target}")

    urls = re.findall(r'^\s*url "([^"]+)"', formula, re.M)
    if len(urls) != len(expected_urls) or set(urls) != expected_urls:
        raise ValueError("Homebrew formula must cover exactly the published macOS/Linux targets")
    if not re.search(r'^\s*depends_on "pandoc"\s*$', formula, re.M):
        raise ValueError("Homebrew formula is missing its Pandoc runtime dependency")
    if not re.search(r'^\s*bin\.install "ashift"\s*$', formula, re.M):
        raise ValueError("Homebrew formula must install the ashift binary")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit("Usage: python3 scripts/release/verify-homebrew.py <distrib-dir>")
    try:
        verify(Path(sys.argv[1]))
    except (OSError, KeyError, ValueError) as error:
        sys.exit(f"ERROR: {error}")
    print("Verified Homebrew platform URLs and SHA-256 against all built archives.")
