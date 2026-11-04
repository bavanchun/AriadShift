"""Licence check and audit for the sign engine dependencies and pack."""

from __future__ import annotations

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
LicenceGate = _core.LicenceGate
canonical = _core.canonical
parse_expression = _core.parse_expression

MPL_EXCEPTIONS = frozenset({"certifi"})

REVIEW_TOML_PATH = Path(__file__).parent / "licence-review.toml"


def load_reviewed_inventory() -> dict[str, dict]:
    """Loads the 31 reviewed packages from licence-review.toml."""
    if not REVIEW_TOML_PATH.is_file():
        return {}
    data = tomllib.loads(REVIEW_TOML_PATH.read_text(encoding="utf-8"))
    packages = data.get("package", [])
    return {canonical(p["name"]): p for p in packages}


def verdict(name: str, expression: str, *, metadata_only: bool = False) -> str | None:
    """Evaluates an SPDX expression with sign-specific MPL exceptions (certifi)."""
    return _core.verdict(
        name,
        expression,
        allowed=ALLOWED,
        allowed_exceptions=ALLOWED_EXCEPTIONS,
        mpl_exceptions=MPL_EXCEPTIONS,
        metadata_only=metadata_only,
    )


def check_distribution(
    dist: metadata.Distribution,
    reviewed_inv: dict[str, dict],
) -> list[Finding]:
    """Audits one installed distribution for licence declaration, RECORD and notices."""
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

    # 2. Check declared licence expression
    spdx = rev["spdx"]
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
            Finding(name, spdx, "missing license or notice file in .dist-info")
        )

    # 4. Check RECORD file
    record_file = None
    for f in dist_info_files:
        if Path(str(f)).name == "RECORD":
            record_file = f
            break
    if record_file is None:
        findings.append(Finding(name, spdx, "missing RECORD file in .dist-info"))

    # 5. Check native components
    is_native_approved = rev.get("native", False)
    has_native_files = any(
        str(f).endswith((".so", ".pyd", ".dylib", ".dll")) for f in files
    )
    if has_native_files and not is_native_approved:
        findings.append(
            Finding(name, spdx, "unexpected native binary object in non-native package")
        )

    return findings


def check_environment(
    distributions: Iterable[metadata.Distribution] | None = None,
) -> list[Finding]:
    """Audits the complete installed environment against the sign licence gate."""
    if distributions is None:
        distributions = list(metadata.distributions())
    reviewed_inv = load_reviewed_inventory()
    findings: list[Finding] = []
    seen = set()

    for dist in distributions:
        name = canonical(dist.metadata.get("Name", ""))
        if not name or name == "ariad-sign":
            continue
        seen.add(name)
        findings.extend(check_distribution(dist, reviewed_inv))

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
