"""Licence check and audit for the sign engine dependencies and pack."""

from __future__ import annotations

import argparse
import base64
import hashlib
import importlib.metadata as metadata
import importlib.util
import sys
import tomllib
from collections.abc import Iterable
from pathlib import Path

_core_path = Path(__file__).resolve().parents[2] / "common" / "licenses" / "core.py"
_spec = importlib.util.spec_from_file_location("engines_common_licenses_core", _core_path)
if _spec is None or _spec.loader is None:
    raise ImportError(f"could not load shared licence core from {_core_path}")
_core = importlib.util.module_from_spec(_spec)
sys.modules[_spec.name] = _core
_spec.loader.exec_module(_core)

ALLOWED = frozenset(set(_core.ALLOWED) | {"MIT-0"})
ALLOWED_DATA = _core.ALLOWED_DATA
ALLOWED_EXCEPTIONS = _core.ALLOWED_EXCEPTIONS
Finding = _core.Finding
canonical = _core.canonical
parse_expression = _core.parse_expression

MPL_EXCEPTIONS = frozenset({"certifi"})

# Extra known free-text licenses and classifiers specific to locked sign packages.
_KNOWN = dict(_core._KNOWN)
_KNOWN.update({
    "psf-2.0": "PSF-2.0",
    "python software foundation license": "PSF-2.0",
})

# Reviewed packages whose metadata contains idiosyncratic or conflicting tags.
OVERRIDES = {
    "qrcode": "BSD-3-Clause",
}

REVIEW_TOML_PATH = Path(__file__).parent / "licence-review.toml"


def load_reviewed_inventory() -> dict[str, dict]:
    """Loads the reviewed packages from licence-review.toml."""
    if not REVIEW_TOML_PATH.is_file():
        return {}
    data = tomllib.loads(REVIEW_TOML_PATH.read_text(encoding="utf-8"))
    packages = data.get("package", [])
    return {canonical(p["name"]): p for p in packages}


def verdict(name: str, expression: str, *, metadata_only: bool = False) -> str | None:
    """Evaluates an SPDX expression with sign-specific MPL exceptions (certifi) and MIT-0."""
    return _core.verdict(
        name,
        expression,
        allowed=ALLOWED,
        allowed_exceptions=ALLOWED_EXCEPTIONS,
        mpl_exceptions=MPL_EXCEPTIONS,
        metadata_only=metadata_only,
    )


def resolve_distribution_license(dist: metadata.Distribution) -> str | None:
    """Reads and resolves the declared SPDX licence expression directly from installed metadata."""
    name = canonical(dist.metadata.get("Name", ""))
    if name in OVERRIDES:
        return OVERRIDES[name]
    return _core.license_of_distribution(
        dist,
        allowed=ALLOWED,
        allowed_exceptions=ALLOWED_EXCEPTIONS,
        mpl_exceptions=MPL_EXCEPTIONS,
        known=_KNOWN,
        overrides=OVERRIDES,
    )


def _record_file_matches(item: metadata.PackagePath, site_root: Path | None = None) -> bool:
    """Verifies that an installed file matches its wheel RECORD SHA-256 and byte size."""
    try:
        path = Path(str(item.locate())) if site_root is None else site_root / str(item)
    except Exception:
        return False
    if not path.is_file() or path.is_symlink():
        return False
    if item.hash is None:
        return True
    data = path.read_bytes()
    if item.size is not None and len(data) != item.size:
        return False
    if item.hash.mode == "sha256":
        digest = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).decode().rstrip("=")
        if item.hash.value != digest:
            return False
    return True


def check_distribution(
    dist: metadata.Distribution,
    reviewed_inv: dict[str, dict],
    site_root: Path | None = None,
) -> list[Finding]:
    """Audits one installed distribution for metadata licence, RECORD integrity, native code, and notices."""
    findings: list[Finding] = []
    name = canonical(dist.metadata.get("Name", ""))
    if not name or name == "ariad-sign":
        return findings

    # 1. Check against reviewed inventory
    rev = reviewed_inv.get(name)
    if rev is None:
        findings.append(
            Finding(
                name,
                "unknown",
                f"unreviewed package '{name}' not in licence-review.toml",
            )
        )
        return findings

    # 2. Check declared licence expression read from installed metadata
    spdx = resolve_distribution_license(dist)
    if spdx is None:
        raw_expr = (dist.metadata.get("License-Expression") or "").strip()
        raw_lic = (dist.metadata.get("License") or "").strip()
        declared = raw_expr or raw_lic or "unresolved"
        findings.append(
            Finding(
                name,
                declared,
                f"unresolved or unreviewed metadata licence declaration in distribution '{name}'",
            )
        )
    else:
        v = verdict(name, spdx)
        if v is not None:
            findings.append(Finding(name, spdx, f"licence not allowed: {v}"))

    # 3. Check for licence notice file in dist-info
    files = dist.files or []
    dist_info_files = [f for f in files if ".dist-info" in str(f)]
    notice_found = any(
        any(
            part.upper().startswith(("LICEN", "COPYING", "NOTICE"))
            for part in Path(str(f)).parts
        )
        for f in dist_info_files
    )
    if not notice_found:
        findings.append(
            Finding(name, spdx or "unknown", "missing license or notice file in .dist-info")
        )

    # 4. Check RECORD file existence
    record_file = next((f for f in dist_info_files if Path(str(f)).name == "RECORD"), None)
    if record_file is None:
        findings.append(Finding(name, spdx or "unknown", "missing RECORD file in .dist-info"))

    # 5. Check native components and verify RECORD SHA-256 and size
    is_native_approved = rev.get("native", False)
    native_files = [
        f for f in files if str(f).endswith((".so", ".pyd", ".dylib", ".dll"))
    ]
    if native_files and not is_native_approved:
        findings.append(
            Finding(name, spdx or "unknown", "unexpected native binary object in non-native package")
        )

    # Check RECORD SHA-256 and size matching for all native files
    for nf in native_files:
        if not _record_file_matches(nf, site_root):
            findings.append(
                Finding(
                    name,
                    spdx or "unknown",
                    f"native file RECORD SHA-256 or size mismatch: {nf}",
                )
            )

    return findings


def check_environment(
    distributions: Iterable[metadata.Distribution] | None = None,
    site_root: Path | None = None,
) -> list[Finding]:
    """Audits the complete installed environment against the sign licence gate."""
    if distributions is None:
        if site_root is not None:
            distributions = list(metadata.distributions(path=[str(site_root)]))
        else:
            distributions = list(metadata.distributions())
    reviewed_inv = load_reviewed_inventory()
    findings: list[Finding] = []
    seen = set()

    for dist in distributions:
        name = canonical(dist.metadata.get("Name", ""))
        if not name or name == "ariad-sign":
            continue
        seen.add(name)
        findings.extend(check_distribution(dist, reviewed_inv, site_root))

    # Also verify that all non-Windows reviewed dependencies are present
    for req_name, req_info in reviewed_inv.items():
        if req_name not in seen and req_name not in ("colorama", "tzdata"):
            findings.append(
                Finding(
                    req_name,
                    req_info["spdx"],
                    "expected locked package not found in environment",
                )
            )

    return findings


def main() -> int:
    parser = argparse.ArgumentParser(description="Sign pack license gate")
    parser.add_argument(
        "--site", type=Path, default=None, help="Path to site-packages to audit"
    )
    args = parser.parse_args()
    if args.site:
        dists = list(metadata.distributions(path=[str(args.site)]))
        findings = check_environment(dists, site_root=args.site)
    else:
        dists = list(metadata.distributions())
        findings = check_environment(dists)

    if findings:
        for f in findings:
            print(f"FINDING: {f.distribution} ({f.license}): {f.reason}", file=sys.stderr)
        return 1
    print(f"License gate passed: {len(dists)} distributions audited cleanly.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
