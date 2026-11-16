"""Licence check and audit for the sign engine dependencies and pack."""

from __future__ import annotations

import argparse
import base64
import hashlib
import importlib.metadata as metadata
import importlib.util
import re
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
UV_LOCK_PATH = Path(__file__).resolve().parents[1] / "uv.lock"

NOTICES_DIR = Path(__file__).parent / "notices"

# Native objects are recognised by suffix and by file magic (ELF, PE `MZ`, Mach-O).
_NATIVE_SUFFIX = re.compile(r"\.(so(\.\d+)*|dylib|dll|pyd|exe)$", re.IGNORECASE)
_NATIVE_MAGIC = (
    b"\x7fELF",
    b"MZ",
    b"\xfe\xed\xfa\xce",
    b"\xce\xfa\xed\xfe",
    b"\xfe\xed\xfa\xcf",
    b"\xcf\xfa\xed\xfe",
    b"\xca\xfe\xba\xbe",
    b"\xbe\xba\xfe\xca",
)
# The only extension-module suffixes a reviewed native entry may carry: ABI-tagged
# CPython (`.cpython-314-x86_64-linux-gnu.so`, `.cp314-win_amd64.pyd`) or stable ABI
# (`.abi3.so`; Windows stable-ABI modules are a plain `.pyd`).
_EXTENSION_SUFFIX = re.compile(
    r"\.(?:cpython-3\d{1,2}t?-[A-Za-z0-9_]+(?:-[A-Za-z0-9_]+)*\.so"
    r"|cp3\d{1,2}t?-[A-Za-z0-9_]+\.pyd"
    r"|abi3\.so|abi3\.pyd|pyd)",
)
# Formats that are ZIP containers by design and never carry executable code paths.
_ZIP_DATA_SUFFIXES = frozenset({".npz", ".docx", ".pptx", ".xlsx"})
_ARCHIVE_SUFFIXES = frozenset({".zip", ".egg", ".whl"})
_ARCHIVE_REFERENCE = re.compile(r"\.(zip|egg|whl)(?:[/\\]|$)", re.IGNORECASE)
_UNREVIEWED_STDLIB_DISTS = ("pip", "setuptools", "wheel")


def _native_name(name: str) -> bool:
    return _NATIVE_SUFFIX.search(name) is not None


def _native_file(path: Path) -> bool:
    """True for a native object by suffix or by leading magic bytes."""
    if _native_name(path.name):
        return True
    with path.open("rb") as handle:
        return handle.read(8).startswith(_NATIVE_MAGIC)


def native_entry_matches(relative: str, entry: str) -> bool:
    """Exact reviewed stem plus a valid extension suffix; no wildcard approval."""
    relative = relative.replace("\\", "/")
    if "*" in entry or "?" in entry or "[" in entry:
        return False
    if not relative.startswith(entry):
        return False
    return _EXTENSION_SUFFIX.fullmatch(relative[len(entry):]) is not None


def load_reviewed_inventory() -> dict[str, dict]:
    """Loads the reviewed packages from licence-review.toml."""
    if not REVIEW_TOML_PATH.is_file():
        return {}
    data = tomllib.loads(REVIEW_TOML_PATH.read_text(encoding="utf-8"))
    packages = data.get("package", [])
    return {canonical(p["name"]): p for p in packages}


def load_locked_wheels(lock_path: Path | None = None) -> dict[tuple[str, str], dict[str, str]]:
    """Maps each locked (package, version) to its {wheel filename: sha256} from uv.lock."""
    path = UV_LOCK_PATH if lock_path is None else lock_path
    data = tomllib.loads(path.read_text(encoding="utf-8"))
    locked: dict[tuple[str, str], dict[str, str]] = {}
    for package in data.get("package", []):
        wheels = {
            wheel["url"].rsplit("/", 1)[-1]: wheel["hash"].removeprefix("sha256:")
            for wheel in package.get("wheels", [])
        }
        locked[(canonical(package["name"]), package.get("version", ""))] = wheels
    return locked


def check_locked_wheels(
    reviewed_inv: dict[str, dict], lock_path: Path | None = None
) -> list[Finding]:
    """Binds every reviewed bundled library to the exact wheels locked in uv.lock."""
    findings: list[Finding] = []
    path = UV_LOCK_PATH if lock_path is None else lock_path
    try:
        locked = load_locked_wheels(path)
    except (OSError, tomllib.TOMLDecodeError, KeyError) as error:
        return [Finding("uv.lock", "unknown", f"cannot read locked wheels from {path}: {error}")]
    for name, rev in reviewed_inv.items():
        for library, entry in rev.items():
            if not isinstance(entry, dict) or "locked_wheels" not in entry:
                continue
            reviewed = entry["locked_wheels"]
            actual = locked.get((name, rev["version"]))
            licence = entry.get("license", "unknown")
            subject = f"{name} ({library} {entry.get('version', '?')})"
            if actual is None:
                reasons = [f"{name} {rev['version']} is not locked in uv.lock"]
            else:
                reasons = (
                    [f"unreviewed locked wheel {w}" for w in sorted(set(actual) - set(reviewed))]
                    + [
                        f"reviewed wheel not locked in uv.lock: {w}"
                        for w in sorted(set(reviewed) - set(actual))
                    ]
                    + [
                        f"locked wheel SHA-256 differs from review: {w}"
                        for w in sorted(set(reviewed) & set(actual))
                        if reviewed[w] != actual[w]
                    ]
                )
            findings.extend(Finding(name, licence, f"{subject}: {r}") for r in reasons)
    return findings


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
    if item.hash is None or item.hash.mode != "sha256":
        return False
    if item.size is None:
        return False
    data = path.read_bytes()
    if len(data) != item.size:
        return False
    digest = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).decode().rstrip("=")
    return item.hash.value == digest


def check_distribution(
    dist: metadata.Distribution,
    reviewed_inv: dict[str, dict],
    site_root: Path | None = None,
) -> list[Finding]:
    """Audits one installed distribution for metadata licence, RECORD integrity, native code, and notices."""
    findings: list[Finding] = []
    name = canonical(dist.metadata.get("Name", ""))
    if not name or name == "ariad-sign" or name in _UNREVIEWED_STDLIB_DISTS:
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

    # 3. Check for non-empty licence notice file in dist-info
    files = dist.files or []
    dist_info_files = [f for f in files if ".dist-info" in str(f)]
    notice_files = [
        f
        for f in dist_info_files
        if any(
            part.upper().startswith(("LICEN", "COPYING", "NOTICE"))
            for part in Path(str(f)).parts
        )
    ]
    has_nonempty_notice = False
    for nf in notice_files:
        try:
            p = Path(str(nf.locate())) if site_root is None else site_root / str(nf)
            if p.is_file() and p.stat().st_size > 0:
                has_nonempty_notice = True
                break
        except Exception:
            pass
    if not has_nonempty_notice:
        findings.append(
            Finding(name, spdx or "unknown", "missing or empty license or notice file in .dist-info")
        )

    # 3b. Check packaged third-party notices required by the reviewed inventory
    for notice in rev.get("notices", []):
        notice_path = NOTICES_DIR / notice
        if not notice_path.is_file() or notice_path.stat().st_size == 0:
            findings.append(
                Finding(name, spdx or "unknown", f"missing or empty bundled notice: {notice}")
            )

    # 4. Check RECORD file existence
    record_file = next((f for f in dist_info_files if Path(str(f)).name == "RECORD"), None)
    if record_file is None:
        findings.append(Finding(name, spdx or "unknown", "missing RECORD file in .dist-info"))

    # 5. Check native components, pattern approval, and verify RECORD SHA-256 and size
    is_native_approved = rev.get("native", False)
    native_files = [f for f in files if _native_name(Path(str(f)).name)]
    if native_files and not is_native_approved:
        findings.append(
            Finding(name, spdx or "unknown", "unexpected native binary object in non-native package")
        )

    approved_entries = rev.get("native_files", [])
    for nf in native_files:
        nf_str = str(nf).replace("\\", "/")
        if is_native_approved and not any(
            native_entry_matches(nf_str, entry) for entry in approved_entries
        ):
            findings.append(
                Finding(
                    name,
                    spdx or "unknown",
                    f"unapproved native object '{nf_str}' in package '{name}'",
                )
            )

        if not _record_file_matches(nf, site_root):
            findings.append(
                Finding(
                    name,
                    spdx or "unknown",
                    f"native file RECORD SHA-256 or size mismatch: {nf}",
                )
            )

    return findings


def _relative_owners(site_root: Path) -> dict[str, str]:
    owners: dict[str, str] = {}
    for dist in metadata.distributions(path=[str(site_root)]):
        owner = canonical(dist.metadata.get("Name", ""))
        for item in dist.files or []:
            owners[item.as_posix()] = owner
    return owners


def check_site_tree(site_root: Path, reviewed_inv: dict[str, dict]) -> list[Finding]:
    """Scans every file under site-packages: symlinks, archives, `.pth`, native objects, ZIP magic.

    Reviewed packages' suffix-native files are audited by `check_distribution`; this scan
    adds everything that audit cannot see: unowned or unreviewed-owner native objects
    (including pip's vendored launcher executables), magic-only native files, hard links,
    archive references and ZIP containers hidden behind innocuous names.
    """
    findings: list[Finding] = []
    owners = _relative_owners(site_root)
    root = site_root.resolve()

    def add(relative: str, reason: str) -> None:
        owner = owners.get(relative, "site-packages")
        findings.append(Finding(owner, "unknown", f"{reason}: {relative}"))

    for path in sorted(site_root.rglob("*")):
        relative = path.relative_to(site_root).as_posix()
        if path.is_symlink() and (path.is_dir() or not path.resolve().is_relative_to(root)):
            add(relative, "unsafe site-packages symlink")
            continue
        if not path.is_file():
            continue
        suffix = path.suffix.casefold()
        if suffix in _ARCHIVE_SUFFIXES:
            add(relative, "uninspected site-packages archive")
            continue
        if suffix == ".pth":
            for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
                stripped = line.strip()
                if not stripped or stripped.startswith("#"):
                    continue
                if _ARCHIVE_REFERENCE.search(stripped):
                    add(relative, "archive reference in site-packages path file")
                    break
                if stripped.startswith(("import ", "import\t")):
                    add(relative, "executable import line in site-packages path file")
                    break
        if _native_file(path):
            if path.is_symlink():
                add(relative, "native file is a symlink")
                continue
            if path.stat().st_nlink != 1:
                add(relative, "native file is a hard link")
            owner = owners.get(relative)
            rev = reviewed_inv.get(owner) if owner else None
            if rev is None:
                add(relative, "native file not owned by a reviewed package")
            elif not _native_name(path.name):
                # Magic-only native file: suffix scanning in check_distribution misses it.
                entries = rev.get("native_files", [])
                if not rev.get("native", False) or not any(
                    native_entry_matches(relative, entry) for entry in entries
                ):
                    add(relative, "unapproved native object (detected by file magic)")
        elif suffix not in _ZIP_DATA_SUFFIXES:
            with path.open("rb") as handle:
                if handle.read(4) == b"PK\x03\x04":
                    add(relative, "uninspected site-packages ZIP archive")
    return findings


def check_environment(
    distributions: Iterable[metadata.Distribution] | None = None,
    site_root: Path | None = None,
    lock_path: Path | None = None,
) -> list[Finding]:
    """Audits the complete installed environment against the sign licence gate."""
    if distributions is None:
        if site_root is not None:
            distributions = list(metadata.distributions(path=[str(site_root)]))
        else:
            distributions = list(metadata.distributions())
    reviewed_inv = load_reviewed_inventory()
    findings: list[Finding] = check_locked_wheels(reviewed_inv, lock_path)
    seen = set()

    for dist in distributions:
        name = canonical(dist.metadata.get("Name", ""))
        if not name or name == "ariad-sign":
            continue
        seen.add(name)
        findings.extend(check_distribution(dist, reviewed_inv, site_root))

    if site_root is not None:
        findings.extend(check_site_tree(site_root, reviewed_inv))

    DEV_PACKAGES = {"pytest", "ruff", "iniconfig", "pluggy", "pygments", "packaging"}

    # Also verify that all non-Windows reviewed dependencies are present
    for req_name, req_info in reviewed_inv.items():
        if req_name not in seen and req_name not in ("colorama", "tzdata"):
            if site_root is not None and req_name in DEV_PACKAGES:
                continue
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
