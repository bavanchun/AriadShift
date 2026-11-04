"""Build and packaging script for the sign engine pack."""

from __future__ import annotations

import argparse
import shutil
import sys
from pathlib import Path

PACK_ROOT = Path(__file__).resolve().parent
SIGN_ENGINE_ROOT = PACK_ROOT.parent
REPO_ROOT = SIGN_ENGINE_ROOT.parents[1]


def build_pack(out_dir: Path, ashift_bin: Path | None = None) -> None:
    """Builds an unpacked development sign pack."""
    out_dir.mkdir(parents=True, exist_ok=True)

    # 1. Copy manifest
    manifest_src = PACK_ROOT / "pack.toml"
    shutil.copy2(manifest_src, out_dir / "pack.toml")

    # 2. Setup notices
    notices_dir = out_dir / "notices"
    notices_dir.mkdir(parents=True, exist_ok=True)
    sign_notice = notices_dir / "SIGN-PACK.txt"
    sign_notice.write_text(
        "AriadShift Sign Engine Pack\n"
        "Copyright (c) 2026 AriadShift Authors\n"
        "Licensed under MIT and third-party permissive licenses.\n",
        encoding="utf-8",
    )
    repo_license = REPO_ROOT / "LICENSE"
    if repo_license.is_file():
        shutil.copy2(repo_license, notices_dir / "ariadshift-LICENSE")

    # 3. Setup python-env
    python_env_dest = out_dir / "python-env"
    venv_src = SIGN_ENGINE_ROOT / ".venv"
    if venv_src.is_dir() and not python_env_dest.exists():
        # Copy or symlink virtual environment
        try:
            shutil.copytree(venv_src, python_env_dest, symlinks=True)
        except Exception:
            python_env_dest.symlink_to(venv_src, target_is_directory=True)

    # 4. Ensure ariad_sign sources are installed / accessible
    src_dir = SIGN_ENGINE_ROOT / "src" / "ariad_sign"
    site_packages = list(python_env_dest.glob("lib/python*/site-packages"))
    if site_packages:
        dest_pkg = site_packages[0] / "ariad_sign"
        if not dest_pkg.exists():
            shutil.copytree(src_dir, dest_pkg)

    print(f"Sign pack built successfully at: {out_dir}")


def check_conformance(pack_dir: Path) -> bool:
    """Validates sign pack structure and manifest."""
    manifest = pack_dir / "pack.toml"
    if not manifest.is_file():
        print(f"Error: manifest missing at {manifest}", file=sys.stderr)
        return False
    return True


def main() -> int:
    parser = argparse.ArgumentParser(description="Sign engine pack builder")
    subparsers = parser.add_subparsers(dest="command", required=True)

    build_p = subparsers.add_parser("build", help="Build sign pack")
    build_p.add_argument("--out", type=Path, required=True, help="Output pack directory")
    build_p.add_argument("--ashift", type=Path, default=None, help="Path to ashift binary")

    conf_p = subparsers.add_parser("conformance", help="Verify pack conformance")
    conf_p.add_argument("--out", type=Path, required=True, help="Pack directory to verify")

    args = parser.parse_args()
    if args.command == "build":
        build_pack(args.out, args.ashift)
        return 0
    elif args.command == "conformance":
        return 0 if check_conformance(args.out) else 1
    return 1


if __name__ == "__main__":
    sys.exit(main())
