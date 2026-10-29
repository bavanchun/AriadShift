"""Licence gate and notices for everything the docling pack bundles.

AGPL and unreviewed copyleft or non-OSI code are refused. Named GNU runtime and permissive
attribution-only native exceptions carry complete notices; CC0 covers data or metadata only.
CI builds from wheels whose bytes are fixed by uv.lock hashes. This gate detects accidental
licensing mistakes after dependency/lock changes, including new libraries, hidden forbidden
licences and missing notices. It does not defend against malicious wheels with forged RECORD files.
Default notice discovery requires a nonempty file, including any file under dist-info/licenses;
it does not validate its content. Explicit inventory notice sources are separately hash-checked.
Archive rejection covers .zip/.egg/.whl suffixes and matching .pth references, plus ZIP local-header
magic in non-native files outside known NPZ/DOCX/PPTX/XLSX data formats
or exact distribution/version/path/SHA-256 model archive reviews. Reviewed model archives
also require one matching RECORD owner. Archive contents are not parsed at runtime, and
other archive magic is not inspected. RECORD refresh rewrites every DLL/PYD digest and
the third-party notice record; this can mask accidental post-install changes on Windows, whose
pack target remains deferred.
Exported toolchain wheels also refuse video-library names, including FFmpeg, even without
licence markers. Every native export additionally requires a positive component identity
from reviewed SHA-256 receipts and its allowed licence and bundled hash-verified notices.
Generated output checksums cannot create a review. Permissive component wheel receipts force
a new review on lock drift; they are not independently reviewed per-object hashes.
Export ZIP members require unique, case-insensitive relative POSIX paths. Directory entries
must have zero declared/compressed size and no readable bytes; payloads still reach scanning.
Installed RECORD checks still apply.
Native GPL/AGPL runtime self-declarations and GPL build flags are refused; absence of a
matching string does not establish a binary's complete licence provenance.
Each Python distribution's licence is read from its metadata (License-Expression, License and
trove classifiers) and checked against an allow-list. Unknown fields or classifiers fail until
their exact metadata is reviewed. The pywin32 exception additionally binds every wheel payload
file to a hash-checked original RECORD receipt and verifies installed hashes/sizes. Only known
uv installation additions and script relocation, plus derived bytecode caches for reviewed
sources, are permitted beyond those wheel paths.

Usage::

    python licenses.py --site SITE_PACKAGES [--notices DIR]
    python licenses.py --wheels WHEEL [WHEEL ...] [--openmp-dll PATH] [--allow-itt]
    python licenses.py --render-toolchain-pins DIR --release-tag TAG
"""

from __future__ import annotations

import argparse
import base64
import csv
import hashlib
import io
import json
import re
import shutil
import sys
import tomllib
import zipfile
from collections.abc import Iterable
from dataclasses import dataclass
from fnmatch import fnmatchcase
from functools import cache
from importlib import metadata
from pathlib import Path
from urllib.parse import unquote, urlsplit

ALLOWED = frozenset(
    {
        "0BSD",
        "Apache-2.0",
        "BSD-2-Clause",
        "BSD-3-Clause",
        "BSL-1.0",
        "CNRI-Python",
        "HPND",
        "ISC",
        "MIT",
        "MIT-CMU",
        "PSF-2.0",
        "Python-2.0",
        "Unlicense",
        "Zlib",
    }
)
# MPL-2.0 is accepted only for these unmodified distributions, one reviewed exception each.
MPL_EXCEPTIONS = frozenset({"certifi", "tqdm"})
# License exceptions that only relax a permissive licence.
ALLOWED_EXCEPTIONS = frozenset({"LLVM-exception"})
# Model weights and data files.
ALLOWED_DATA = frozenset({"Apache-2.0", "CDLA-Permissive-2.0", "CC0-1.0", "MIT", "PSF-2.0"})

_DENIED = re.compile(r"AGPL|(?<!L)GPL|SSPL|RAIL|BUSL|Commons-Clause|NC-|Proprietary", re.I)
_RAW_DENIED = re.compile(r"(?<!Lesser )General Public License|Affero", re.I)
_GPL_RUNTIME = re.compile(
    rb"lib[\w-]+ licen[sc]e:\s*(?:A?GPL)\b|(?:^|\s)--enable-gpl(?:\s|\x00|$)", re.I
)

# Free-text ``License`` fields and trove classifiers seen in the locked dependencies.
_KNOWN = {
    "apache": "Apache-2.0",
    "apache 2.0": "Apache-2.0",
    "apache 2.0 license": "Apache-2.0",
    "apache-2.0": "Apache-2.0",
    "apache software license": "Apache-2.0",
    "bsd": "BSD-3-Clause",
    "bsd license": "BSD-3-Clause",
    "bsd-3-clause": "BSD-3-Clause",
    "3-clause bsd license": "BSD-3-Clause",
    "isc license": "ISC",
    "isc license (iscl)": "ISC",
    "mit": "MIT",
    "mit license": "MIT",
    "mozilla public license 2.0 (mpl 2.0)": "MPL-2.0",
    "mpl-2.0": "MPL-2.0",
    "mpl-2.0 and mit": "MPL-2.0 AND MIT",
    "psfl": "PSF-2.0",
    "python software foundation license": "PSF-2.0",
}

# Reviewed distributions whose metadata does not state an SPDX expression.
OVERRIDES = {
    # "BSD-3-Clause, Apache-2.0, dependency licenses": pypdfium2 itself and the bundled PDFium.
    "pypdfium2": "Apache-2.0 AND BSD-3-Clause",
    # "Dual License": Apache-2.0 or BSD-3-Clause.
    "python-dateutil": "Apache-2.0 OR BSD-3-Clause",
    # The License field holds the full BSD-3-Clause text.
    "scipy": "BSD-3-Clause",
}


# pywin32 b312: metadata says PSF, but its bundled libraries have additional obligations.
# All three locked CPython 3.14 Windows wheels share these notices and adodbapi sources.
_PYWIN32_LICENSE = "BSD-3-Clause AND HPND AND LGPL-2.1-or-later AND MIT AND PSF-2.0 AND Python-2.0"
_PYWIN32_REVIEW = {
    "version": "312",
    "library_version": "2.6.2.0",
    "source": "https://github.com/mhammond/pywin32/archive/2a277cb5552756c2b4d42b524dc36d25e0bb6354.tar.gz",
    "metadata_sha256": "7b0ffc7aaed4258c6b2a0ef67d984d55a9169d9bbd1a0a8ace2a35542a75a519",
    "payload_sha256": "2563911fede3a7a0b77ce9bb712a2145a0853c4203a2de72fc80cca8a16bfcc4",
    "records": {
        "pywin32-312-cp314-win32.record": (
            "7af03355b008f5487ef350e846aa68c989505cf9ee8afd975e8f8b71ab66c07a"
        ),
        "pywin32-312-cp314-win-amd64.record": (
            "65762feb4074c3a4f4f1b7eeca7db60ad1c741f40dca1bf7efcdc7f13217019c"
        ),
        "pywin32-312-cp314-win-arm64.record": (
            "c2b5405aef6630c27ac46625bf227671d386538a7a9e545b6e8b74419411230d"
        ),
    },
    "notices": {
        "adodbapi/license.txt": "5ea2f23c7f00c3006bacf267183e816d8d4b6dc95ea57a4ddb0ff81de6d8719e",
        "com/License.txt": "fad39c18fc3825fa278db1c86fd52cf88d7c525b3c2fbf7817fa383cbae4d423",
        "com/win32comext/mapi/src/MAPIStubLibrary/LICENSE": (
            "1c4cf44d2fb8bf2d10214d6da80cc9e05502824cc4460f35be4c7bcb4cde53da"
        ),
        "isapi/README.txt": "41383bcc95edb8138d37077ae54bfdcbd2a3b77bf745a8529a9c161146e22303",
        "pythonwin/License.txt": "a74d826efb242db087b850348e20abbfd591b51e2081f2048735290913872924",
        "pythonwin/Scintilla/License.txt": (
            "57ec79bbeab4fd1cf7460a7ad450d2ab01471810459aeae9e75e4c66b472095f"
        ),
        "pythonwin/pywin/idle/LICENSE.txt": (
            "a0fa1e7b8a4230055685e796548ac084a6c0eea395294574093bba72efd9191b"
        ),
        "win32/License.txt": "a74d826efb242db087b850348e20abbfd591b51e2081f2048735290913872924",
    },
}


def _pywin32_generated_cache(relative: Path, files: set[str]) -> bool:
    match = re.fullmatch(r"(.+)\.cpython-314(?:\.opt-[012])?\.pyc", relative.name)
    return bool(
        match
        and relative.parent.name == "__pycache__"
        and (relative.parent.parent / (match[1] + ".py")).as_posix() in files
    )


def _pywin32_record_matches(distribution: metadata.Distribution) -> bool:
    """Compare the installed payload to a hash-bound original wheel RECORD receipt."""
    files = list(distribution.files or [])
    installed = {item.as_posix(): item for item in files}
    record_name = f"pywin32-{distribution.version}.dist-info/RECORD"
    if len(installed) != len(files) or record_name not in installed:
        return False
    for relative, item in installed.items():
        if relative == record_name:
            if item.hash is not None or item.size is not None:
                return False
        elif not _record_file_matches(item, Path(item.locate())):
            return False
    site = Path(distribution.locate_file(""))
    for filename, digest in _PYWIN32_REVIEW["records"].items():
        receipt = Path(__file__).with_name("reviewed-records") / filename
        raw = receipt.read_bytes()
        if hashlib.sha256(raw).hexdigest() != digest:
            continue
        remaining = set(installed)
        payload = set()
        additions = {
            f"pywin32-{distribution.version}.dist-info/{name}"
            for name in ("INSTALLER", "REQUESTED", "direct_url.json", "uv_cache.json")
        }
        normalized = io.StringIO()
        writer = csv.writer(normalized, lineterminator="\n")
        for relative, _, _ in csv.reader(io.StringIO(raw.decode("utf-8"))):
            aliases = {relative}
            if relative.startswith(f"pywin32-{distribution.version}.data/scripts/"):
                name = Path(relative).name
                aliases.update(
                    f"{folder}/{name}"
                    for folder in ("bin", "Scripts", "../../../bin", "../../Scripts")
                )
            matches = aliases & remaining
            if len(matches) != 1:
                break
            actual = matches.pop()
            item = installed[actual]
            actual_checksum = f"{item.hash.mode}={item.hash.value}" if item.hash else ""
            writer.writerow(
                (relative, actual_checksum, str(item.size) if item.size is not None else "")
            )
            remaining.remove(actual)
            payload.add(actual)
            if actual != relative:
                # uv adds launchers beside the two relocated wheel scripts.
                launcher = str(Path(actual).with_suffix("")).replace("\\", "/")
                additions.update((launcher, launcher + ".exe"))
        else:
            if hashlib.sha256(normalized.getvalue().encode("utf-8")).hexdigest() != digest:
                continue
            if remaining - additions:
                continue
            # Shared bin/Scripts directories also contain other distributions' launchers.
            roots = {
                Path(name).parts[0]
                for name in payload
                if len(Path(name).parts) > 1 and Path(name).parts[0] not in {"..", "bin", "Scripts"}
            }
            for directory in roots:
                for path in (site / directory).rglob("*"):
                    relative = path.relative_to(site)
                    if path.is_symlink() or (
                        path.is_file()
                        and relative.as_posix() not in installed
                        and not _pywin32_generated_cache(relative, payload)
                    ):
                        return False
            return True
    return False


def _reviewed_pywin32_expression(distribution: metadata.Distribution) -> str | None:
    """Permit only the reviewed wheel payload, declaration, full notices and library source."""
    rule = _PYWIN32_REVIEW
    if distribution.version != rule["version"]:
        return None
    try:
        if not _pywin32_record_matches(distribution):
            return None
    except OSError, ValueError:
        return None
    text = distribution.read_text("METADATA") or ""
    if hashlib.sha256(text.encode("utf-8")).hexdigest() != rule["metadata_sha256"]:
        return None
    files = {item.as_posix(): item for item in distribution.files or []}
    for relative, digest in rule["notices"].items():
        matches = [
            item for name, item in files.items() if name.endswith(".dist-info/licenses/" + relative)
        ]
        if len(matches) != 1:
            return None
        notice = Path(matches[0].locate())
        if (
            notice.is_symlink()
            or not notice.is_file()
            or hashlib.sha256(notice.read_bytes()).hexdigest() != digest
        ):
            return None
    source = Path(distribution.locate_file("adodbapi"))
    if source.is_symlink() or not source.is_dir():
        return None
    lines = []
    for path in source.rglob("*"):
        if path.is_symlink():
            return None
        if not path.is_file() or (path.parent.name == "__pycache__" and path.suffix == ".pyc"):
            continue
        lines.append(
            f"{path.relative_to(source).as_posix()} {hashlib.sha256(path.read_bytes()).hexdigest()}"
        )
    digest = hashlib.sha256("\n".join(sorted(lines)).encode("utf-8")).hexdigest()
    return _PYWIN32_LICENSE if digest == rule["payload_sha256"] else None


@dataclass(frozen=True)
class Finding:
    distribution: str
    license: str
    reason: str


def canonical(name: str) -> str:
    return re.sub(r"[-_.]+", "-", name).lower()


def _licence_classifiers(distribution: metadata.Distribution) -> list[str]:
    return [
        item
        for item in distribution.metadata.get_all("Classifier") or []
        if item.split("::", 1)[0].strip().casefold() == "license"
    ]


def _duplicate_licence_fields(distribution: metadata.Distribution) -> bool:
    return any(
        len(distribution.metadata.get_all(field) or []) > 1
        for field in ("License", "License-Expression")
    )


def _empty_licence_fields(distribution: metadata.Distribution) -> bool:
    return any(
        not value.strip()
        for key, value in distribution.metadata.items()
        if key.casefold() in {"license", "license-expression"}
        or key.casefold().startswith("license-file")
    )


def _licence_signature(expression: str) -> frozenset[frozenset[str]]:
    """Compare known declarations independent of operand order and parentheses."""
    alternatives = _split_expression(expression, "OR")
    if len(alternatives) > 1:
        return frozenset(term for part in alternatives for term in _licence_signature(part))
    terms = _split_expression(expression, "AND")
    if len(terms) > 1:
        result = frozenset({frozenset()})
        for term in terms:
            result = frozenset(a | b for a in result for b in _licence_signature(term))
        return result
    return frozenset({frozenset(terms)})


def license_of(distribution: metadata.Distribution) -> str | None:
    """Resolve every declared field; unknown, duplicate or conflicting claims fail."""
    if _duplicate_licence_fields(distribution) or _empty_licence_fields(distribution):
        return None
    name = canonical(distribution.metadata["Name"])
    if name == "pywin32":
        return _reviewed_pywin32_expression(distribution)
    field = (distribution.metadata.get("License") or "").strip()
    reviewed = _reviewed_license_field(distribution)
    if field and field.lower() not in _KNOWN and not reviewed:
        return None
    expression = (distribution.metadata.get("License-Expression") or "").strip()
    claims = [expression] if expression else []
    if field:
        claims.append(OVERRIDES[name] if reviewed else _KNOWN[field.lower()])
    for classifier in _licence_classifiers(distribution):
        parts = [part.strip().casefold() for part in classifier.split("::")]
        if parts == ["license", "osi approved"]:
            continue
        if len(parts) < 2 or parts[-1] not in _KNOWN:
            return None
        claims.append(_KNOWN[parts[-1]])
    if not claims or any(
        verdict(name, claim, metadata_only=_cc0_metadata_only(distribution)) is not None
        for claim in claims
    ):
        return None
    signatures = {_licence_signature(claim) for claim in claims}
    if reviewed and name == "python-dateutil":
        # Only this reviewed dual-licence declaration resolves its two classifiers.
        expected = _licence_signature(OVERRIDES[name])
        if expression and _licence_signature(expression) != expected:
            return None
        if any(not signature <= expected for signature in signatures):
            return None
        return OVERRIDES[name]
    return claims[0] if len(signatures) == 1 else None


def _reviewed_license_field(distribution: metadata.Distribution) -> bool:
    """Bind full-text overrides to the reviewed metadata, never just a package name."""
    name = canonical(distribution.metadata["Name"])
    field = distribution.metadata.get("License") or ""
    if name == "scipy":
        # SciPy 1.18.1's full text includes the separately inventoried GNU runtime notices.
        return hashlib.sha256(field.encode()).hexdigest() in {
            "2a463073ea6816d22b763c8e2f03886f4e8bf4f2a234f3c9267401d688588d17",
            "7f2f9e131a09f25323e392aac37fb2e263c600bb9d1ffc7c3b6ca3e1d017573e",
            "c85f6c4154a28ccb2a49fe9a1cbcacb17939c14ffad7a2f63d32655610652bb7",
        }
    return bool(field) and field.strip() == {
        "pypdfium2": "BSD-3-Clause, Apache-2.0, dependency licenses",
        "python-dateutil": "Dual License",
    }.get(name)


def verdict(name: str, expression: str, *, metadata_only: bool = False) -> str | None:
    """Returns why an expression is not acceptable, or ``None`` when it is."""
    if _DENIED.search(expression):
        return "excluded licence"
    try:
        allowed = _Expression(name, expression, metadata_only=metadata_only).allowed()
    except ValueError:
        return "not a valid SPDX expression"
    return None if allowed else "not in the allow-list"


class _Expression:
    """Evaluates an SPDX expression against the allow-list.

    Every ``OR`` alternative must be known and allowed; ``AND`` needs every operand, and
    ``WITH`` accepts only the reviewed exceptions. Anything the grammar does not cover is an
    error, which the gate treats as a refusal.
    """

    def __init__(self, name: str, expression: str, *, metadata_only: bool = False) -> None:
        self._name = name
        self._metadata_only = metadata_only
        self._tokens = re.findall(r"\(|\)|[^\s()]+", expression)
        self._position = 0

    def allowed(self) -> bool:
        result = self._or()
        if self._position != len(self._tokens):
            raise ValueError("unexpected token")
        return result

    def _peek(self) -> str | None:
        return self._tokens[self._position] if self._position < len(self._tokens) else None

    def _take(self) -> str:
        token = self._peek()
        if token is None:
            raise ValueError("unexpected end")
        self._position += 1
        return token

    def _or(self) -> bool:
        results = [self._and()]
        while self._peek() == "OR":
            self._take()
            results.append(self._and())
        return all(results)

    def _and(self) -> bool:
        results = [self._operand()]
        while self._peek() == "AND":
            self._take()
            results.append(self._operand())
        return all(results)

    def _operand(self) -> bool:
        word = self._take()
        if word == "(":
            result = self._or()
            if self._take() != ")":
                raise ValueError("missing closing parenthesis")
            return result
        if word in (")", "AND", "OR", "WITH"):
            raise ValueError("unexpected word")
        if self._peek() == "WITH":
            self._take()
            exception = self._take()
            return exception in ALLOWED_EXCEPTIONS and self._licence_allowed(word)
        return self._licence_allowed(word)

    def _licence_allowed(self, licence: str) -> bool:
        if licence == "CC0-1.0":
            return self._metadata_only
        if licence == "MPL-2.0":
            return self._name in MPL_EXCEPTIONS
        return licence in ALLOWED


def check(distributions: Iterable[metadata.Distribution]) -> list[Finding]:
    findings = []
    for distribution in distributions:
        name = canonical(distribution.metadata["Name"])
        if _duplicate_licence_fields(distribution):
            findings.append(Finding(name, "unknown", "duplicate single-value licence field"))
            continue
        if name == "pywin32":
            if license_of(distribution) is None:
                findings.append(
                    Finding(
                        name, "unknown", "changed or unreviewed pywin32 metadata, notices or source"
                    )
                )
            continue
        raw = list(distribution.metadata.get_all("License") or [])
        # Only the exact reviewed field may contain separately gated runtime notices.
        if _reviewed_license_field(distribution):
            raw = [OVERRIDES[name]]
        raw += _licence_classifiers(distribution)
        raw += list(distribution.metadata.get_all("License-Expression") or [])
        excluded = next(
            (item for item in raw if _DENIED.search(item) or _RAW_DENIED.search(item)), None
        )
        if excluded is not None:
            findings.append(Finding(name, excluded, "excluded licence"))
            continue
        expression = license_of(distribution)
        if expression is None:
            findings.append(
                Finding(name, "unknown", "unknown or inconsistent licence declarations")
            )
            continue
        reason = verdict(name, expression, metadata_only=_cc0_metadata_only(distribution))
        if reason is not None:
            findings.append(Finding(name, expression, reason))
    return findings


_NUMPY_METADATA_SHA256 = "e2b909d6a37d76c2bc130f528d7fed0c517322bce16dc7846b6a6df30e200403"
_NUMPY_FILE_INVENTORIES = frozenset(
    {
        "1b8ffe0592f49f1daf0f29a365cb47354ffca7fe63030c831ec39894517c5e97",
        "80e85ff2da44d20cb2848389cec02e7e0852325fdf188e1264d04bd39488794a",
        "d8dae2d8a536498e0de91d3769f2d61bfa740a8a74ae34bd2fec84a072a0e94e",
        "ed179c6718e7dbb6c023c54eee6cd5583375e629cde9030050994a6443751e40",
    }
)


def _cc0_metadata_only(distribution: metadata.Distribution) -> bool:
    # The four locked NumPy wheels contain the SplitMix64 notice, not its benchmark/
    # test-generator code. Bind the exception to their metadata and complete source-file
    # inventories so a renamed or newly added source file also requires review.
    if (canonical(distribution.metadata["Name"]), distribution.version) != ("numpy", "2.5.4"):
        return False
    text = distribution.read_text("METADATA") or ""
    if hashlib.sha256(text.encode()).hexdigest() != _NUMPY_METADATA_SHA256:
        return False
    root = Path(distribution.locate_file("numpy"))
    paths = sorted(
        "numpy/" + item.relative_to(root).as_posix()
        for item in root.rglob("*")
        if item.is_file() and "__pycache__" not in item.parts and item.suffix != ".pyc"
    )
    return hashlib.sha256("\n".join(paths).encode()).hexdigest() in _NUMPY_FILE_INVENTORIES


def _distribution_notices(distribution: metadata.Distribution) -> list[Path]:
    return [
        Path(item.locate())
        for item in distribution.files or []
        if len(item.parts) > 1
        and item.parts[0].endswith(".dist-info")
        and (
            item.parts[1].casefold() == "licenses"
            or re.match(r"(LICEN[CS]E|COPYING|NOTICE)", item.name, re.I)
        )
        and Path(item.locate()).is_file()
        and Path(item.locate()).read_bytes().strip()
    ]


def _inventory_notices(site: Path, item: dict) -> list[Path]:
    root, source = _native_notice(site, item)
    sources = [source]
    for extra in item.get("additional_notices", []):
        path = root / extra["path"]
        if (
            not path.resolve().is_relative_to(root.resolve())
            or not path.is_file()
            or not path.read_bytes().strip()
            or hashlib.sha256(path.read_bytes()).hexdigest() != extra["sha256"]
        ):
            raise SystemExit(f"missing or changed reviewed native notice: {extra['path']}")
        sources.append(path)
    return sources


def write_notices(
    distributions: Iterable[metadata.Distribution], notices: Path, inventory: Iterable[dict] = ()
) -> None:
    """Copies every distribution's licence files and writes a summary table."""
    distributions = list(distributions)
    by_name = {canonical(item.metadata["Name"]): item for item in distributions}
    root = notices / "python"
    rows = []
    reviewed_owners = set()
    for item in inventory:
        owner = by_name[item["distribution"]]
        for source in _inventory_notices(Path(owner.locate_file("")), item):
            if not source.is_file() or not source.read_bytes().strip():
                raise SystemExit(
                    f"native owner has no nonempty licence notice: {item['distribution']}"
                )
        _, source = _native_notice(Path(owner.locate_file("")), item)
        if hashlib.sha256(source.read_bytes()).hexdigest() != item.get("notice_sha256"):
            raise SystemExit(f"changed reviewed native notice: {item['notice']}")
        reviewed_owners.add(item["distribution"])
    for distribution in sorted(distributions, key=lambda item: canonical(item.metadata["Name"])):
        name = canonical(distribution.metadata["Name"])
        if name == "pywin32" and license_of(distribution) is None:
            raise SystemExit("changed or unreviewed pywin32 metadata, notices or source")
        has_native = any(
            Path(item.locate()).is_file() and _native_file(Path(item.locate()))
            for item in distribution.files or []
        )
        if has_native and not _distribution_notices(distribution) and name not in reviewed_owners:
            raise SystemExit(f"native owner has no nonempty licence notice: {name}")
        version = distribution.version
        rows.append(f"{name} {version}: {license_of(distribution)}")
        target = root / f"{name}-{version}"
        for item in distribution.files or []:
            parts = item.parts
            if not parts or not parts[0].endswith(".dist-info") or len(parts) < 2:
                continue
            if not (parts[1].casefold() == "licenses" or _is_licence_name(parts[-1])):
                continue
            source = Path(str(item.locate()))
            if source.is_file():
                destination = target.joinpath(*parts[1:])
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(source, destination)
        if name == "pywin32":
            rule = _PYWIN32_REVIEW
            target.mkdir(parents=True, exist_ok=True)
            (target / "adodbapi-source.txt").write_text(
                f"adodbapi {rule['library_version']}: LGPL-2.1-or-later\n"
                "Unmodified dynamically imported Python source is included in\n"
                "runtime site-packages/adodbapi.\n"
                "Full LGPL-2.1 text: licenses/adodbapi/license.txt.\n"
                f"Source payload SHA-256: {rule['payload_sha256']}\n"
                f"Upstream pywin32 {rule['version']} (tag b312), including adodbapi source:\n"
                f"{rule['source']}\n",
                encoding="utf-8",
            )
    notices.mkdir(parents=True, exist_ok=True)
    (notices / "python-distributions.txt").write_text("\n".join(rows) + "\n", encoding="utf-8")


def combined_expression(expressions: Iterable[str]) -> str:
    """Merge conjunctions across targets, retaining nonredundant OR/WITH clauses."""
    terms = set()
    for expression in expressions:
        terms.update(_conjuncts(expression))
    # A separately required licence already satisfies an OR alternative. Likewise, its
    # unqualified obligations subsume the same licence with an optional exception.
    redundant = {
        term
        for term in terms
        if (
            len(alternatives := _split_expression(term, "OR")) > 1
            and any(alternative in terms for alternative in alternatives)
        )
        or (" WITH " in term and term.split(" WITH ", 1)[0] in terms)
    }
    return " AND ".join(f"({item})" for item in sorted(terms - redundant))


def _split_expression(expression: str, operator: str) -> list[str]:
    tokens = re.findall(r"\(|\)|[^\s()]+", expression)
    while tokens and tokens[0] == "(":
        depth = 0
        for _end, word in enumerate(tokens):
            depth += (word == "(") - (word == ")")
            if depth == 0:
                break
        if _end != len(tokens) - 1:
            break
        tokens = tokens[1:-1]
    parts = []
    start = depth = 0
    for index, word in enumerate(tokens):
        depth += (word == "(") - (word == ")")
        if depth == 0 and word == operator:
            parts.append(" ".join(tokens[start:index]))
            start = index + 1
    parts.append(" ".join(tokens[start:]))
    return parts


def _conjuncts(expression: str) -> list[str]:
    alternatives = _split_expression(expression, "OR")
    if len(alternatives) > 1:
        return [" OR ".join(alternatives)]
    terms = _split_expression(expression, "AND")
    if len(terms) == 1:
        return terms
    return [term for part in terms for term in _conjuncts(part)]


def pack_expression(distributions: Iterable[metadata.Distribution], extra: Iterable[str]) -> str:
    """Refuse failing distributions before including their actual licences in the manifest."""
    distributions = list(distributions)
    findings = check(distributions)
    if findings:
        details = "\n".join(f"  {f.distribution}: {f.license}: {f.reason}" for f in findings)
        raise SystemExit(f"licence gate failed:\n{details}")
    # Metadata expressions have already passed the gate; retain their alternatives and exceptions.
    return combined_expression([*(license_of(item) for item in distributions), *extra])


def runtime_libraries(release: str, target: str) -> list[dict]:
    """The reviewed runtime-library inventory, tied to the pinned standalone Python release."""
    with Path(__file__).with_name("runtime-libraries.toml").open("rb") as handle:
        inventory = tomllib.load(handle)
    if inventory["python_release"] != release:
        raise SystemExit("review runtime-library licences for the new Python release")
    platform = _target_platform(target)
    return [
        item
        for item in inventory["libraries"]
        if item.get("platform", platform) == platform
        or (item.get("platform") == "unix" and platform != "windows")
    ]


def check_runtime_files(runtime: Path, libraries: list[dict], target: str) -> None:
    """Require attribution for every native runtime file, excluding separately gated wheels.

    Patterns name shared libraries and extensions, including libraries statically linked into
    libpython or a .pyd. Every selected inventory entry must describe at least one native file.
    """
    platform = _target_platform(target)
    files = {
        path.relative_to(runtime).as_posix()
        for path in runtime.rglob("*")
        if path.is_file()
        and "site-packages" not in path.relative_to(runtime).parts
        and _native_file(path)
    }
    covered = set()
    errors = []
    for library in libraries:
        patterns = list(library.get("native_files", {}).get(platform, []))
        if library["name"] == "cpython":
            # The interpreter has the same upstream notice as libpython. It is now
            # detected by magic even when it has no shared-library suffix.
            patterns += ["bin/python", "bin/python3", "bin/python3.14", "python.exe", "pythonw.exe"]
        matches = {name for name in files if any(fnmatchcase(name, p) for p in patterns)}
        if not matches:
            errors.append(f"{library['name']}: inventory patterns match no native files")
        covered.update(matches)
    errors.extend(f"unlisted native runtime file: {name}" for name in sorted(files - covered))
    if errors:
        raise SystemExit("runtime inventory check failed:\n" + "\n".join(errors))


_NATIVE_MAGIC = (
    b"\x7fELF",
    b"MZ",
    b"\xfe\xed\xfa\xce",
    b"\xce\xfa\xed\xfe",
    b"\xfe\xed\xfa\xcf",
    b"\xcf\xfa\xed\xfe",
    b"\xca\xfe\xba\xbe",
    b"\xbe\xba\xfe\xca",
    b"\xca\xfe\xba\xbf",
    b"\xbf\xba\xfe\xca",
    b"!<arch>\n",
    b"!<thin>\n",
)
_GCC_RUNTIME = "GPL-3.0-or-later WITH GCC-exception-3.1"
# Native exceptions are bound to library and distribution, never Python classifiers.
NATIVE_LIBRARIES = {
    "libgeos_c": ("LGPL-2.1-only", {"shapely"}),
    "libgeos": ("LGPL-2.1-only", {"shapely"}),
    "libspatialindex": ("MIT", {"rtree"}),
    "libgomp": (_GCC_RUNTIME, {"torch"}),
    "libgfortran": (_GCC_RUNTIME, {"numpy", "scipy", "opencv-python-headless", "torch"}),
    "libquadmath": ("LGPL-2.1-or-later", {"numpy", "scipy", "opencv-python-headless"}),
    "libscipy_openblas": ("BSD-3-Clause", {"numpy", "scipy"}),
    "libopenblas": ("BSD-3-Clause", {"opencv-python-headless"}),
    "libXau": ("MIT", {"pillow"}),
    "libavif": ("BSD-2-Clause", {"pillow"}),
    "libbrotlicommon": ("MIT", {"pillow"}),
    "libbrotlidec": ("MIT", {"pillow"}),
    "libfreetype": ("FTL", {"pillow"}),
    "libjpeg": ("BSD-3-Clause AND IJG AND Zlib", {"pillow", "torchvision"}),
    "libpng16": ("libpng-2.0", {"pillow", "torchvision"}),
    "libtiff": ("libtiff", {"pillow"}),
    "libharfbuzz": ("MIT", {"pillow"}),
    "liblcms2": ("MIT", {"pillow"}),
    "liblzma": ("0BSD", {"pillow"}),
    "libopenjp2": ("BSD-2-Clause", {"pillow"}),
    "libsharpyuv": ("BSD-3-Clause", {"pillow", "torchvision"}),
    "libwebp": ("BSD-3-Clause", {"pillow", "torchvision"}),
    "libwebpdemux": ("BSD-3-Clause", {"pillow"}),
    "libwebpmux": ("BSD-3-Clause", {"pillow"}),
    "libxcb": ("MIT", {"pillow"}),
    "libzstd": ("BSD-3-Clause", {"pillow"}),
    "libz": ("Zlib", {"pillow", "torchvision"}),
}
with Path(__file__).with_name("native-components.toml").open("rb") as _handle:
    _native_reviews = tomllib.load(_handle)
    _NATIVE_COMPONENTS = {item["name"]: item for item in _native_reviews["components"]}
    _GNU_TARGET_SHARED_FILES = {
        (item["target"], item["distribution"], item["path"]): item["sha256"]
        for item in _native_reviews.get("gnu_runtimes", [])
    }
NATIVE_LIBRARIES.update(
    {name: (item["license"], {item["distribution"]}) for name, item in _NATIVE_COMPONENTS.items()}
)
_NATIVE_VERSIONS = {
    "shapely": {"2.2.0"},
    "rtree": {"1.4.1"},
    "numpy": {"2.5.4"},
    "scipy": {"1.18.1"},
    "pillow": {"12.3.0", "13.0.0"},
    "torch": {"2.14.1"},
    "torchvision": {"0.29.1"},
    "opencv-python-headless": {"5.0.0.93"},
}
for _component in _NATIVE_COMPONENTS.values():
    _NATIVE_VERSIONS.setdefault(_component["distribution"], set()).add(
        _component["version"].split("+")[0]
    )

# Hash-bound complete notices from locked wheels or matching upstream releases.
_NATIVE_NOTICES = {
    ("rtree", "1.4.1"): (
        "libspatialindex.txt",
        frozenset({"6c4a6221066c0f5ff3244f36dfdfd627e274ee871d87f82c7da40168c53745e2"}),
    ),
    ("shapely", "2.2.0"): (
        "licenses/LICENSE_GEOS",
        frozenset(
            {
                "7233baa9a7d4a17257b357706b7dbdc4b675e5ca45425b3418673b16990cf239",
                "81d9aaf9ab7e3e6184568a70f27ee3e138d8d1e543b46dad5af88cff61cf1b36",
            }
        ),
    ),
    ("numpy", "2.5.4"): (
        "licenses/LICENSE.txt",
        frozenset(
            {
                "4860083caa0de2ac3292ca98bd074bd8f45d8b32624e37b1e70a240bff61e488",
                "649a589ab31076e79e9ce62ba24eae9959598124176640dbc95f87bd10f34fcf",
                "a804dff0ead9fadc5293456410bcbfc32bf024be9c4513459663fb7b442d2341",
            }
        ),
    ),
    ("scipy", "1.18.1"): (
        "LICENSE.txt",
        frozenset(
            {
                "09a1b9e19573045b0784a181cc4bef677039d4f2111631c747bf72b7f19017d4",
                "17e7db9ffa4913121fa2df484633a362456a482da736c99db4711a98473770ea",
                "f0f5c56b298ec9795df1199edc328a987242716d14a1bde6813112e22dc15f99",
            }
        ),
    ),
    ("pillow", "12.3.0"): (
        "licenses/LICENSE",
        frozenset(
            {
                "4f7866a74802c6326f81faff59a56546b6aec2b10b91973e0e9308de95e79857",
                "dda12a98c1979cf3d94df1cff45d27a4cb3f04a60c76f76902ac54cac03ec0ce",
            }
        ),
    ),
    ("pillow", "13.0.0"): (
        "licenses/LICENSE",
        frozenset(
            {
                "988ffe8ab3480b0f4e9a0ab2702bffd73d9fb6b47198043fb3ba101a62fac236",
                "dda12a98c1979cf3d94df1cff45d27a4cb3f04a60c76f76902ac54cac03ec0ce",
            }
        ),
    ),
}


# Exact shared objects from the hash-locked wheels. A changed or static payload
# cannot inherit an exception meant for unmodified dynamically loaded libraries.
_REVIEWED_SHARED_SOURCES = {
    "shapely": ("3.14.1", "https://download.osgeo.org/geos/geos-3.14.1.tar.bz2"),
    "rtree": ("2.1.0", "https://github.com/libspatialindex/libspatialindex/archive/2.1.0.zip"),
}
_REVIEWED_SHARED_FILES = {
    "shapely.libs/libgeos-d1e818bb.so.3.14.1": (
        "1e9192b9f1b9f43cd7adbb9999d23d9efda34695cc057bb1911bfc1f9c1d1c9d"
    ),
    "shapely.libs/libgeos_c-ac38a227.so.1.20.5": (
        "47a714ef803c586c8ec85473efe42fab96dadc1fe51663fc7123dd4743acb9a8"
    ),
    "shapely.libs/libgeos-bf1df61c.so.3.14.1": (
        "7e7306267b0cd693c3b40770dcf42df53a4f0ee5041011e47f0d9cf9a0be9344"
    ),
    "shapely.libs/libgeos_c-f518db6b.so.1.20.5": (
        "9ef62ec5e9134b0d1abb1d97e5f6bd4ce0a8f9c9e422cfdb89b3063f8df931f2"
    ),
    "shapely/.dylibs/libgeos.3.14.1.dylib": (
        "032ed33655a02fb281ffc56db46626664c4827cecf7b99df1c1375d0988133d1"
    ),
    "shapely/.dylibs/libgeos_c.1.20.5.dylib": (
        "6a1886cf6bd5b6decb596429479a8846b117578528f59e29681547e728e28b18"
    ),
    "shapely.libs/geos-bf067cd6ff74ee0ad5f3ff52c7ef08c9.dll": (
        "0948bdb5c70b82e5055b55e9419b78d68372b1d0ec877277fa6e79276f6299b9"
    ),
    "shapely.libs/geos_c-6dd9fd915eef8a7928285416bef1e666.dll": (
        "6e65985b30831761887de3217853a720147626b18cb41668cd2bdc43d1bcded1"
    ),
    "rtree.libs/libspatialindex-e5350069.so": (
        "e4eec0695dee7d3864e5d6e6ec3b3ba80a5adf8e65e7586bf0d8ed4584b91449"
    ),
    "rtree.libs/libspatialindex-8598ee94.so": (
        "9cfa808c9cfafeea6725c277085d3144bb29972e911066c191aa893aa147a300"
    ),
    "rtree/lib/libspatialindex.dylib": (
        "70108e4f07ccd87c724f28b0e14fa26b9603ef61a45653087df89b1fc26665b3"
    ),
    "rtree/lib/libspatialindex_c.dylib": (
        "a4c2282d8ba7f63562b0631370f4e6fbd2b078e3841b5081d77b535b11af7676"
    ),
    "rtree/lib/spatialindex-64.dll": (
        "4a0116d384b1a6cf0ca74a0366d82359e6211e89933ec37dd18eedb537f619c2"
    ),
    "rtree/lib/spatialindex_c-64.dll": (
        "b08c8672a7cbec64abaabdfcabc40b8e090e2a06eab85569b1a87aef82dd33de"
    ),
}


def _native_notice(site: Path, item: dict) -> tuple[Path, Path]:
    root = Path(__file__).with_name("licenses") if item.get("notice_scope") == "upstream" else site
    return root, root / item["notice"]


@cache
def _component_basenames(paths: tuple[str, ...]) -> frozenset[str]:
    return frozenset(Path(path).name.casefold() for path in paths)


_LOCK_FILE = Path(__file__).resolve().parents[3] / "uv.lock"


@cache
def _locked_wheels(contents: bytes) -> dict[tuple[str, str, str], str]:
    """Cache parsing by the actual lock bytes, so same-process edits cannot hide drift."""
    return {
        (
            canonical(package["name"]),
            package["version"],
            Path(unquote(urlsplit(wheel["url"]).path)).name,
        ): wheel["hash"].removeprefix("sha256:")
        for package in tomllib.loads(contents.decode("utf-8"))["package"]
        for wheel in package.get("wheels", [])
    }


def _native_family(owner: str, filename: str) -> str | None:
    filename = Path(filename.replace("\\", "/")).name.casefold()
    if owner == "shapely" and filename.startswith("geos"):
        filename = "lib" + filename
    if owner == "rtree" and re.match(r"(?:lib)?spatialindex(?:_c)?(?:[-_.0-9]|$)", filename):
        return "libspatialindex"
    return next(
        (
            name
            for name, (_, owners) in NATIVE_LIBRARIES.items()
            if owner in owners
            and re.match(re.escape(name.casefold()) + r"(?:[-_.0-9]|$)", filename)
        ),
        None,
    ) or next(
        (
            name
            for name, item in _NATIVE_COMPONENTS.items()
            if item["distribution"] == owner
            and filename in _component_basenames(tuple(item["files"]))
        ),
        None,
    )


def _native_file(path: Path) -> bool:
    if any(fnmatchcase(path.name.lower(), p) for p in ("*.so*", "*.dylib", "*.dll", "*.pyd")):
        return True
    with path.open("rb") as handle:
        return handle.read(8).startswith(_NATIVE_MAGIC)


with Path(__file__).with_name("reviewed-data-archives.toml").open("rb") as _archive_handle:
    _REVIEWED_DATA_ARCHIVES = tomllib.load(_archive_handle)["archives"]


def _reviewed_data_archive(site: Path, path: Path, relative: str) -> bool:
    rules = [rule for rule in _REVIEWED_DATA_ARCHIVES if rule["path"] == relative]
    if not rules:
        return False
    owners = [
        (distribution, item)
        for distribution in metadata.distributions(path=[str(site)])
        for item in distribution.files or []
        if item.as_posix() == relative
    ]
    if len(owners) != 1:
        return False
    owner, item = owners[0]
    if not _record_file_matches(item, path):
        return False
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    return any(
        canonical(owner.metadata["Name"] or "") == rule["distribution"]
        and owner.version == rule["version"]
        and digest == rule["sha256"]
        for rule in rules
    )


def _native_files(site: Path) -> set[str]:
    """Suffix and magic detection covers every file in site-packages."""
    files = set()
    for path in site.rglob("*"):
        relative = path.relative_to(site).as_posix()
        if path.is_symlink() and (
            path.is_dir() or not path.resolve().is_relative_to(site.resolve())
        ):
            raise SystemExit(f"unsafe site-packages symlink: {relative}")
        if not path.is_file():
            continue
        if path.suffix.casefold() in {".zip", ".egg", ".whl"}:
            raise SystemExit(f"uninspected site-packages archive: {relative}")
        if path.suffix.casefold() == ".pth" and any(
            re.search(r"\.(zip|egg|whl)(?:[/\\]|$)", line.strip(), re.I)
            for line in path.read_text(encoding="utf-8").splitlines()
            if not line.lstrip().startswith("#")
        ):
            raise SystemExit(f"archive reference in site-packages path file: {relative}")
        if _native_file(path):
            if path.is_symlink():
                raise SystemExit(f"native file is a symlink: {relative}")
            if path.stat().st_nlink != 1:
                raise SystemExit(f"native file is a hard link: {relative}")
            if _GPL_RUNTIME.search(path.read_bytes()):
                raise SystemExit(f"forbidden GPL runtime declaration: {relative}")
            files.add(relative)
        elif path.suffix.casefold() not in {".npz", ".docx", ".pptx", ".xlsx"}:
            with path.open("rb") as handle:
                is_zip = handle.read(4) == b"PK\x03\x04"
            if is_zip and not _reviewed_data_archive(site, path, relative):
                raise SystemExit(f"uninspected site-packages ZIP archive: {relative}")
    return files


def _vendor_path(relative: str) -> bool:
    path = Path(relative)
    return any(
        part.casefold().endswith(".libs") or part.casefold() == ".dylibs" for part in path.parts
    ) or any(
        _native_family(owner, path.name) is not None
        for _, owners in NATIVE_LIBRARIES.values()
        for owner in owners
    )


def _vendored_files(site: Path) -> set[str]:
    return {name for name in _native_files(site) if _vendor_path(name)}


def _record_file_matches(item: metadata.PackagePath, path: Path) -> bool:
    if item.hash is None or item.hash.mode != "sha256" or path.is_symlink() or not path.is_file():
        return False
    data = path.read_bytes()
    digest = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).decode().rstrip("=")
    return item.hash.value == digest and item.size == len(data)


def _native_record_owners(site: Path, files: set[str]) -> dict[str, list[metadata.Distribution]]:
    owners: dict[str, list[metadata.Distribution]] = {}
    for distribution in metadata.distributions(path=[str(site)]):
        for item in distribution.files or []:
            relative = Path(str(item)).as_posix()
            if relative in files:
                path = site / relative
                if not _record_file_matches(item, path):
                    raise SystemExit(f"native RECORD SHA-256/size mismatch: {relative}")
                owners.setdefault(relative, []).append(distribution)
    return owners


_GNU_SHARED_FILES = {
    "numpy.libs/libgfortran-83c28eba-468e71e5.so.5.0.0": (
        "a6a368328d02a5d8a149746f13f6268f8cc9192df5c18777b25c3c9b5ef52160"
    ),
    "numpy.libs/libquadmath-2284e583-a9307bba.so.0.0.0": (
        "6bc3069003caec1f075be3c51ae8355332f081fa9465910c4b5724e7fe7d8ad4"
    ),
    "scipy.libs/libgfortran-040039e1-0352e75f.so.5.0.0": (
        "c6090048eccc763522c12ef016f81da6b627cb3a044f55cf0479a839c41c0980"
    ),
    "scipy.libs/libgfortran-83c28eba.so.5.0.0": (
        "468e71e59e1d784821a76702e5ecd1e0dbcfc024499f74a552d1bea4b26f27ad"
    ),
    "scipy.libs/libquadmath-2284e583.so.0.0.0": (
        "a9307bbaa353623fa014e8b682d50c4629f5e5c7d61daa9a8652f7d165cd421a"
    ),
    "scipy.libs/libquadmath-96973f99-934c22de.so.0.0.0": (
        "6ed5137f412781ad7863439fb543613f620b43c32b63292a0029246162f5bbc6"
    ),
    "torch.libs/libgomp-6a29c32d.so.1.0.0": (
        "2aa037784caec5079de4f23a7d526a62bcc03a3fb98ac94031345daabe21c465"
    ),
}
_GNU_FAMILIES = frozenset({"libgomp", "libgfortran", "libquadmath"})


def _elf_shared_object(path: Path) -> bool:
    with path.open("rb") as handle:
        header = handle.read(20)
    return (
        len(header) == 20
        and header.startswith(b"\x7fELF")
        and header[4] in {1, 2}
        and header[5] in {1, 2}
        and int.from_bytes(header[16:18], "little" if header[5] == 1 else "big") == 3
    )


def vendored_libraries(site: Path, target: str) -> list[dict]:
    """Select named vendor libraries, using actual RECORD ownership in every directory."""
    files = _native_files(site)
    owners = _native_record_owners(site, files)
    platform = _target_platform(target)
    inventory = []
    distributions = {
        canonical(item.metadata["Name"]): item for item in metadata.distributions(path=[str(site)])
    }
    for relative in sorted(name for name in files if _vendor_path(name)):
        matches = owners.get(relative, [])
        if len(matches) != 1:
            raise SystemExit(f"native file requires exactly one RECORD owner: {relative}")
        distribution = matches[0]
        owner = canonical(distribution.metadata["Name"])
        if distribution.version.split("+")[0] not in _NATIVE_VERSIONS.get(owner, set()):
            raise SystemExit(f"review vendored library licences for {owner}: {relative}")
        family = _native_family(owner, Path(relative).name)
        if family is None:
            raise SystemExit(f"review vendored library licences for {owner}: {relative}")
        component = _NATIVE_COMPONENTS.get(family)
        if component is not None:
            if distribution.version != component["version"] or relative not in component["files"]:
                raise SystemExit(f"unlisted native component for {owner}: {relative}")
            sources = component["notices"]
            item = {
                "name": family,
                "distribution": owner,
                "license": component["license"],
                "notice": sources[0]["path"],
                "notice_sha256": sources[0]["sha256"],
                "notice_scope": component.get("notice_scope", "wheel"),
                "additional_notices": sources[1:],
                "native_files": {platform: [relative]},
            }
            if component.get("source"):
                item.update(
                    version=component.get("upstream_version", component["version"]),
                    source=component["source"],
                )
            inventory.append(item)
            continue
        # The source-built torch/OpenCV wheels may only carry their own notices.
        # SciPy supplies OpenBLAS/GNU runtime terms; Pillow supplies the complete
        # matching codec notices missing from TorchVision's BSD-only wrapper notice.
        notice_owner = (
            distributions.get("scipy")
            if owner in {"torch", "opencv-python-headless"}
            else distributions.get("pillow")
            if owner == "torchvision"
            else distribution
        )
        if notice_owner is None:
            raise SystemExit(f"missing vendored licence notice for {owner}: {relative}")
        notice_name = canonical(notice_owner.metadata["Name"])
        notice_version = notice_owner.version
        reviewed = _NATIVE_NOTICES.get((notice_name, notice_version))
        if reviewed is None:
            raise SystemExit(f"review native licence notice for {notice_name} {notice_version}")
        scope = "upstream" if owner == "rtree" else "wheel"
        notice = (
            reviewed[0]
            if scope == "upstream"
            else f"{notice_name.replace('-', '_')}-{notice_version}.dist-info/{reviewed[0]}"
        )
        _, source = _native_notice(site, {"notice": notice, "notice_scope": scope})
        if (
            not source.is_file()
            or hashlib.sha256(source.read_bytes()).hexdigest() not in reviewed[1]
        ):
            raise SystemExit(f"native licence notice digest mismatch: {notice}")
        inventory.append(
            {
                "name": family,
                "distribution": owner,
                "license": NATIVE_LIBRARIES[family][0],
                "notice": notice,
                "notice_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
                "notice_scope": scope,
                "native_files": {platform: [relative]},
            }
        )
        if owner in _REVIEWED_SHARED_SOURCES:
            version, source_url = _REVIEWED_SHARED_SOURCES[owner]
            inventory[-1].update(version=version, source=source_url)
    return inventory


def _target_platform(target: str) -> str:
    return (
        "windows"
        if target.endswith("windows-msvc")
        else ("macos" if target.endswith("apple-darwin") else "linux")
    )


def check_vendored_native_files(site: Path, inventory: list[dict], target: str) -> list[dict]:
    """Reject unlisted native files, stale/overlapping entries and missing notices."""
    platform = _target_platform(target)
    files = _native_files(site)
    owners = _native_record_owners(site, files)
    distributions = {id(owner): owner for records in owners.values() for owner in records}
    licensed = {key: not check([owner]) for key, owner in distributions.items()}
    notice_owners = {key for key, owner in distributions.items() if _distribution_notices(owner)}
    covered = set()
    errors = []
    locked = (
        _locked_wheels(_LOCK_FILE.read_bytes())
        if any("reviewed_wheels" in _NATIVE_COMPONENTS.get(item["name"], {}) for item in inventory)
        else {}
    )
    for item in inventory:
        reviewed = NATIVE_LIBRARIES.get(item["name"])
        if reviewed is not None:
            allowed = item["license"] == reviewed[0] and item["distribution"] in reviewed[1]
        else:
            allowed = False
        if not allowed:
            errors.append(f"{item['name']}: excluded or unreviewed native licence")
        patterns = item.get("native_files", {}).get(platform, [])
        matches = {name for name in files if any(fnmatchcase(name, p) for p in patterns)}
        if not matches:
            errors.append(f"{item['name']}: inventory patterns match no native files")
        if covered & matches:
            errors.append(f"{item['name']}: native file matches multiple inventory entries")
        covered.update(matches)
        component = _NATIVE_COMPONENTS.get(item["name"])
        if component is not None and (
            item["notice"] != component["notices"][0]["path"]
            or item.get("notice_scope", "wheel") != component.get("notice_scope", "wheel")
            or item.get("notice_sha256") != component["notices"][0]["sha256"]
            or item.get("additional_notices", []) != component["notices"][1:]
            or any(name not in component["files"] for name in matches)
        ):
            errors.append(f"{item['name']}: unreviewed component file or notices")
        # Permissive entries bind reviewed locked-wheel inputs plus installed RECORD
        # hashes/sizes, not independently reviewed per-object bytes. Approved source
        # toolchain replacements retain their own output hashes and runtime probes.
        if component is not None and "reviewed_wheels" in component:
            binding = component["reviewed_wheels"].get(target)
            if (
                not binding
                or not re.fullmatch(r"[a-f0-9]{64}", binding.get("sha256", ""))
                or (
                    locked.get(
                        (component["distribution"], component["version"], binding.get("name"))
                    )
                    != binding["sha256"]
                )
            ):
                errors.append(f"{item['name']}: reviewed locked wheel changed; renew native review")
        if component is not None and (
            component["license"].startswith(("LGPL-", "MPL-"))
            or component.get("shared_objects") is not None
        ):
            if (item.get("version"), item.get("source")) != (
                component.get("upstream_version"),
                component.get("source"),
            ) or not all((item.get("version"), item.get("source"))):
                errors.append(f"{item['name']}: missing exact upstream version/source")
            if any(
                not _elf_shared_object(site / relative)
                or hashlib.sha256((site / relative).read_bytes()).hexdigest()
                != component.get("shared_objects", {}).get(relative)
                for relative in matches
            ):
                errors.append(f"{item['name']}: requires an unmodified reviewed shared object")
        for relative in matches:
            records = owners.get(relative, [])
            if (
                len(records) != 1
                or canonical(records[0].metadata["Name"]) != item["distribution"]
                or not licensed[id(records[0])]
                or (component is not None and records[0].version != component["version"])
                or not (site / relative).resolve().is_relative_to(site.resolve())
            ):
                errors.append(f"native file requires one licensed RECORD owner: {relative}")
            if _native_family(item["distribution"], Path(relative).name) != item["name"]:
                errors.append(f"native file does not match its named library: {relative}")
        if item["name"] in _GNU_FAMILIES and any(
            not _elf_shared_object(site / relative)
            or hashlib.sha256((site / relative).read_bytes()).hexdigest()
            != (
                _GNU_SHARED_FILES.get(relative)
                or _GNU_TARGET_SHARED_FILES.get((target, item["distribution"], relative))
            )
            for relative in matches
        ):
            errors.append(f"{item['name']}: requires an unmodified reviewed ELF shared object")
        shared = _REVIEWED_SHARED_SOURCES.get(item["distribution"])
        if shared is not None:
            if (item.get("version"), item.get("source")) != shared:
                errors.append(f"{item['name']}: missing exact upstream version/source")
            if any(
                hashlib.sha256((site / name).read_bytes()).hexdigest()
                != _REVIEWED_SHARED_FILES.get(name)
                for name in matches
            ):
                errors.append(f"{item['name']}: requires an unmodified reviewed shared object")
        notice_root, notice = _native_notice(site, item)
        if not notice.resolve().is_relative_to(notice_root.resolve()) or not notice.is_file():
            errors.append(f"{item['name']}: missing or unsafe native licence notice")
        elif not notice.read_bytes():
            errors.append(f"{item['name']}: empty native licence notice")
        elif (
            item.get("notice_sha256")
            and hashlib.sha256(notice.read_bytes()).hexdigest() != item["notice_sha256"]
        ):
            errors.append(f"{item['name']}: native licence notice digest mismatch")
        elif not notice.read_bytes().strip():
            errors.append(f"{item['name']}: empty native licence notice")
        elif item["license"] in {_GCC_RUNTIME, "LGPL-2.1-or-later", "LGPL-2.1-only"}:
            text = notice.read_text(encoding="utf-8").lower()
            if item["name"] in _GNU_FAMILIES:
                text += "\n".join(
                    p.read_text(encoding="utf-8").lower() for p in _gnu_notice_texts()
                )
            required = (
                ("gnu lesser general public license",)
                if item["license"].startswith("LGPL-")
                else ("gnu general public license", "gcc runtime library exception")
            )
            if any(term not in text for term in required):
                errors.append(f"{item['name']}: incomplete GNU runtime licence notice")
        for source in _inventory_notices(site, item):
            if not source.is_file() or not source.read_bytes().strip():
                errors.append(f"{item['name']}: missing native licence notice")
        notice_owners.update(
            key
            for key, owner in distributions.items()
            if canonical(owner.metadata["Name"]) == item["distribution"]
        )
    for relative in sorted(files - covered):
        records = owners.get(relative, [])
        if (
            _vendor_path(relative)
            or not _extension_module(site / relative)
            or len(records) != 1
            or not (site / relative).resolve().is_relative_to(site.resolve())
        ):
            errors.append(f"unlisted native wheel file: {relative}")
        elif not licensed[id(records[0])]:
            errors.append(f"native module has no licensed RECORD owner: {relative}")
        elif id(records[0]) not in notice_owners:
            errors.append(f"native owner has no nonempty licence notice: {relative}")

    if errors:
        raise SystemExit("vendored inventory check failed:\n" + "\n".join(errors))
    return inventory


def _extension_module(path: Path) -> bool:
    if re.search(r"(\.cpython-[^.]+\.so|\.abi3\.so|\.pyd)$", path.name, re.I):
        return True
    # Mach-O extension bundles may use a plain .so basename.
    with path.open("rb") as handle:
        header = handle.read(16)
    endian = {
        b"\xfe\xed\xfa\xce": "big",
        b"\xce\xfa\xed\xfe": "little",
        b"\xfe\xed\xfa\xcf": "big",
        b"\xcf\xfa\xed\xfe": "little",
    }.get(header[:4])
    return (
        path.suffix.casefold() == ".so"
        and endian is not None
        and int.from_bytes(header[12:16], endian) == 8
    )


# The wheel aggregates carry libquadmath attribution and a licence reference,
# but omit the full LGPL document. Ship it too, normalizing trailing whitespace.
_LGPL_21_SHA256 = "43285937a0e9b75b13eb12fdd35ac3101f7fa92cb0a950d18e4dbf90c7a15c4f"
_LGPL_21_SOURCE = (
    "https://raw.githubusercontent.com/gcc-mirror/gcc/refs/tags/"
    "releases/gcc-14.2.0/libquadmath/COPYING.LIB"
)


def _libquadmath_licence_text() -> Path:
    source = Path(__file__).with_name("licenses") / "lgpl-2.1.txt"
    if not source.is_file() or hashlib.sha256(source.read_bytes()).hexdigest() != _LGPL_21_SHA256:
        raise SystemExit("libquadmath: missing or changed full LGPL-2.1 text")
    return source


_GNU_NOTICE_TEXTS = {
    "gpl-3.0.txt": ("8ceb4b9ee5adedde47b31e975c1d90c73ad27b6b165a1dcd80c7c545eb65b903"),
    "gcc-runtime-exception-3.1.txt": (
        "28e85c5aa4af9b4f1dfe6b4817aa3eefb3eaaee7fd735045016f29ccf50276a1"
    ),
    "lgpl-2.1.txt": ("43285937a0e9b75b13eb12fdd35ac3101f7fa92cb0a950d18e4dbf90c7a15c4f"),
    "gnu-runtime-source-pointers.txt": (
        "7fc1eb329b7c9793720aa04faa2498c086eede2779bf5e8b4e7dccb494116181"
    ),
    "gnu-runtime-source-offer.txt": (
        "20b795b27debe132e63c1c7153504444831bfd9c35fbf0d73dd470f431953fcf"
    ),
}


def _gnu_notice_texts() -> list[Path]:
    root = Path(__file__).with_name("licenses")
    paths = []
    for name, digest in _GNU_NOTICE_TEXTS.items():
        source = root / name
        if not source.is_file() or hashlib.sha256(source.read_bytes()).hexdigest() != digest:
            raise SystemExit(f"GNU runtime notice is missing or changed: {name}")
        paths.append(source)
    return paths


def write_vendored_notices(site: Path, notices: Path, inventory: list[dict]) -> None:
    """Copy complete upstream texts once and attribute every native library/file."""
    root = notices / "vendored"
    root.mkdir(parents=True, exist_ok=True)
    rows = []
    for item in inventory:
        sources = _inventory_notices(site, item)
        if item["name"] in _GNU_FAMILIES:
            sources.extend(_gnu_notice_texts())
        text_rows = []
        for text_source in sources:
            destination = root / (
                f"{hashlib.sha256(text_source.read_bytes()).hexdigest()}-{text_source.name}"
            )
            if not destination.exists():
                shutil.copyfile(text_source, destination)
            text_rows.append(f"  Text: notices/{destination.relative_to(notices).as_posix()}")
        if item["name"] == "libquadmath":
            text_rows.append(f"  Licence text source: {_LGPL_21_SOURCE}")
        if item["name"] in _GNU_FAMILIES:
            text_rows.append(
                "  Exact GCC source release: not established; see bundled ABI/source pointers "
                "and corresponding-source offer."
            )
        rows.extend(
            [
                f"{item['distribution']}: {item['name']}: {item['license']}"
                + (f"; upstream version {item['version']}" if item.get("version") else ""),
                *text_rows,
                f"  Source: {item['source']}"
                if item.get("source")
                else "  GNU runtime source archive: https://ftp.gnu.org/gnu/gcc/"
                if item["license"] in {_GCC_RUNTIME, "LGPL-2.1-or-later"}
                else f"  Source: https://pypi.org/project/{item['distribution']}/",
                f"  Native files: {item['native_files']}",
            ]
        )
    (notices / "vendored-libraries.txt").write_text("\n".join(rows) + "\n", encoding="utf-8")


_INTEL_LIBRARIES = ("iomp", "ippicv", "libipp", "libmkl", "mkl_rt", "mkl_core")
# These families cannot belong to a video-disabled toolchain export. Names alone
# suffice, even when a binary omits GPL declarations or configure strings.
_VIDEO_LIBRARIES = {
    "libav": "FFmpeg is disabled in exported toolchains",
    "libsw": "FFmpeg resampling/scaling is disabled in exported toolchains",
    "libpostproc": "FFmpeg postprocessing is disabled in exported toolchains",
    "libx264": "x264 is excluded from video-disabled exports",
    "libx265": "x265 is excluded from video-disabled exports",
    "librubberband": "Rubber Band is excluded from video-disabled exports",
    "libvidstab": "video stabilization is excluded from video-disabled exports",
    "libfdk": "FDK codecs are excluded from video-disabled exports",
}
_ITT_LIBRARIES = ("ittnotify", "libitt")
_INTEL_MARKERS = (
    b"ippicvgetlibversion",
    b"ippicv_",
    b"ippgetlibversion",
    b"intel(r) openmp",
    b"@(#) intel(r) omp",
    b"intel(r) oneapi math kernel library",
    b"mkl_get_version_string",
    b"mkl_blas_",
    b"mkl_lapack_",
    b"mkl_serv_",
    "Intel(R) OpenMP".encode("utf-16le").lower(),
)
_ITT_MARKERS = (b"__itt_", b"ittnotify", b"ijit_notifyevent")
_IPP_NOTICE_HEADING = (
    "Intel(R) IPP ICV library statically linked within x86 and x86_64 opencv-python packages."
)
_NOTICE_SEPARATOR = "------------------------------------------------------------------------------"


def _strip_reviewed_opencv_ipp_notice(text: str, label: str) -> str:
    sections = text.split(_NOTICE_SEPARATOR)
    matches = [s for s in sections if s.strip().startswith(_IPP_NOTICE_HEADING)]
    if len(matches) != 1 or "Intel Simplified Software License" not in matches[0]:
        raise SystemExit(f"unreviewed OpenCV IPP notice layout: {label}")
    return _NOTICE_SEPARATOR.join(s for s in sections if s != matches[0])


def _stream_chunks(header: bytes, handle):
    yield header
    yield from iter(lambda: handle.read(1 << 20), b"")


def _scan_stream(
    handle,
    relative: str,
    name: str,
    approved_openmp: dict[str, str],
    errors: list[str],
    *,
    allow_itt: bool = True,
    strip_opencv_ipp_notice: bool = False,
    reject_video: bool = False,
) -> str | None:
    header = handle.read(20)
    native = header.startswith(_NATIVE_MAGIC) or any(
        fnmatchcase(name, p)
        for p in ("*.so*", "*.dll", "*.pyd", "*.dylib", "*.exe", "*.a", "*.lib")
    )
    if reject_video:
        native |= header.startswith((b"BC\xc0\xde", b"\xde\xc0\x17\x0b", b"\0\0\xff\xff")) or (
            len(header) == 20
            and header[:2] in (b"\x64\x86", b"\x4c\x01", b"\x64\xaa", b"\xc0\x01", b"\xc4\x01")
            and int.from_bytes(header[2:4], "little") > 0
        )
    if native and reject_video and not re.match(r"libavif(?:[._-]|$)", name):
        for family, reason in _VIDEO_LIBRARIES.items():
            if name.startswith(family):
                errors.append(f"forbidden video library: {relative}: {reason}")
                return
    libraries = _INTEL_LIBRARIES if allow_itt else (*_INTEL_LIBRARIES, *_ITT_LIBRARIES)
    if any(word in name for word in libraries):
        data = header + handle.read()
        if relative not in approved_openmp or (
            hashlib.sha256(data).hexdigest() != approved_openmp[relative]
        ):
            errors.append(f"unapproved Intel library: {relative}")
            return
        chunks = [data]
    else:
        chunks = None
    if native:
        # Inspect even approved files: a poisoned native-build cache must
        # not bless the original vendor DLL by supplying its digest.
        markers = _INTEL_MARKERS if allow_itt else (*_INTEL_MARKERS, *_ITT_MARKERS)
        tail = b""
        digest = hashlib.sha256() if reject_video else None
        iterator = iter(chunks) if chunks is not None else _stream_chunks(header, handle)
        for chunk in iterator:
            if digest is not None:
                digest.update(chunk)
            data = (tail + chunk).lower()
            if _GPL_RUNTIME.search(data) or (reject_video and b"--enable-nonfree" in data):
                errors.append(f"forbidden GPL runtime declaration: {relative}")
                return None
            if any(marker in data for marker in markers):
                errors.append(f"proprietary Intel binary content: {relative}")
                return None
            tail = data[-128:]
        return digest.hexdigest() if digest is not None else None
    elif any(word in name for word in ("license", "licence", "notice", "copying")):
        raw = b"".join(chunks) if chunks is not None else header + handle.read()
        if strip_opencv_ipp_notice and name == "license-3rd-party.txt":
            raw = _strip_reviewed_opencv_ipp_notice(raw.decode("utf-8"), relative).encode("utf-8")
        data = raw.lower()
        # ONNX Runtime's common notices list MKL even in CPU ARM builds
        # without MKL. A license title alone does not identify shipped code.
        # The explicit IPP/ICV section and native markers remain forbidden.
        if any(
            marker in data
            for marker in (
                b"ipp icv library",
                b"intel(r) integrated performance primitives",
            )
        ):
            errors.append(f"proprietary IPP ICV notice: {relative}")
    return None


def _export_native_review(digest: str, label: str, archive, artifact: Path, errors: list[str]):
    """Identify actual bytes using trusted reviews, never candidate output checksums.

    Paths in receipts document the reviewed originals; identity follows the hash
    through renames. Notices must accompany these bytes in the artifact itself.
    Installed-pack wheel-input reviews do not constitute export object approvals.
    """
    matches = [
        component
        for component in _NATIVE_COMPONENTS.values()
        if digest in component.get("export_objects", {}).values()
    ]
    if len(matches) != 1:
        errors.append(f"unreviewed native export: {label}: sha256={digest}; matches={len(matches)}")
        return
    component = matches[0]
    if (
        not all(component.get(key) for key in ("name", "distribution", "version", "license"))
        or verdict(component["distribution"], component["license"]) is not None
        or not component.get("notices")
        or any(path not in component["files"] for path in component["export_objects"])
    ):
        errors.append(f"native export lacks an allowed component licence/review: {label}")
        return
    for notice in component["notices"]:
        relative = notice["path"]
        # Receipt paths use portable archive syntax, including on Windows,
        # where a leading slash is rooted but Path.is_absolute() is false.
        if (
            relative.startswith("/")
            or ":" in relative
            or "\\" in relative
            or ".." in relative.split("/")
        ):
            errors.append(f"native export notice path is unsafe: {label}")
            continue
        path = Path(relative)
        if component.get("notice_scope") == "upstream":
            path = Path("notices") / path
        try:
            raw = (
                archive.read(path.as_posix())
                if archive is not None
                else (artifact.parent / path).read_bytes()
            )
        except KeyError, FileNotFoundError:
            raw = b""
        if not raw.strip() or hashlib.sha256(raw).hexdigest() != notice["sha256"]:
            errors.append(f"native export lacks its reviewed notice: {label}: {path}")


def check_vendor_files(
    root: Path,
    approved_openmp: dict[str, str] | None = None,
    *,
    allow_itt: bool = True,
) -> None:
    """Reject proprietary wheel contents, independently of distribution metadata.

    The Windows LLVM replacement retains PyTorch's expected DLL name. Only the
    exact file produced by the verified source build is accepted at that path.
    The caller keeps its digest outside the runtime being checked.
    """
    approved_openmp = approved_openmp or {}
    errors: list[str] = []
    present: set[str] = set()
    for path in sorted(root.rglob("*")):
        if not path.is_file():
            continue
        relative = path.relative_to(root).as_posix()
        present.add(relative)
        name = path.name.lower()
        if name.endswith(".whl"):
            is_opencv = name.startswith("opencv_python_headless-")
            with zipfile.ZipFile(path) as archive:
                for info in sorted(archive.infolist(), key=lambda item: item.filename):
                    if info.is_dir():
                        continue
                    member_rel = f"{relative}!/{info.filename}"
                    present.add(member_rel)
                    member_name = Path(info.filename).name.lower()
                    with archive.open(info) as handle:
                        _scan_stream(
                            handle,
                            member_rel,
                            member_name,
                            approved_openmp,
                            errors,
                            allow_itt=allow_itt,
                            strip_opencv_ipp_notice=is_opencv,
                        )
            continue
        with path.open("rb") as handle:
            _scan_stream(
                handle,
                relative,
                name,
                approved_openmp,
                errors,
                allow_itt=allow_itt,
            )
    missing = set(approved_openmp) - present
    errors.extend(f"missing LLVM OpenMP replacement: {name}" for name in sorted(missing))
    if errors:
        raise SystemExit("vendor library check failed:\n" + "\n".join(errors))


def _export_directory(info: zipfile.ZipInfo) -> bool:
    return (
        info.is_dir()
        or bool(info.external_attr & 0x10)
        or (info.external_attr >> 16) & 0o170000 == 0o040000
    )


def check_wheel_files(
    wheels: Iterable[Path],
    approved_openmp: dict[str, str] | None = None,
    *,
    allow_itt: bool = False,
    openmp_dll: Path | None = None,
) -> None:
    """Require reviewed native bytes/licences/notices and refuse Intel/video exports.

    Source-built toolchain wheels disable ITT instrumentation (``USE_ITT=0``,
    ``-DWITH_ITT=OFF``, ``-DLIBOMP_USE_ITT_NOTIFY=OFF``), so ``allow_itt``
    defaults to ``False`` here. Video-library names are refused independently of
    GPL self-declarations/flags. The installed pack keeps its named LGPL FFmpeg review.
    Every native payload also requires one explicit export-object SHA-256 review
    and complete reviewed notices inside its wheel (or beside a loose artifact).
    No candidate hash, OpenMP sidecar or Python extension bypasses this requirement.
    """
    approved = dict(approved_openmp or {})
    wheel_list = [Path(wheel) for wheel in wheels]
    if not wheel_list:
        raise SystemExit("no wheels provided to check_wheel_files")
    openmp_digest = None
    if openmp_dll is not None:
        if not openmp_dll.is_file():
            raise SystemExit(f"missing LLVM OpenMP sidecar: {openmp_dll}")
        openmp_digest = hashlib.sha256(openmp_dll.read_bytes()).hexdigest()
    errors: list[str] = []
    present: set[str] = set()
    if openmp_dll is not None and openmp_digest is not None:
        label = openmp_dll.name
        present.add(label)
        approved[label] = openmp_digest
        with openmp_dll.open("rb") as handle:
            digest = _scan_stream(
                handle,
                label,
                openmp_dll.name.lower(),
                approved,
                errors,
                allow_itt=allow_itt,
                reject_video=True,
            )
        if digest is not None:
            _export_native_review(digest, label, None, openmp_dll, errors)
    for path in wheel_list:
        if not path.is_file():
            raise SystemExit(f"missing wheel or artifact file: {path}")
        name = path.name.lower()
        if name.endswith(".whl"):
            is_opencv = name.startswith("opencv_python_headless-")
            with zipfile.ZipFile(path) as archive:
                members = archive.infolist()
                names = [info.filename.removesuffix("/").casefold() for info in members]
                files = {
                    name
                    for info, name in zip(members, names, strict=True)
                    if not _export_directory(info)
                }
                parents = {
                    "/".join(name.split("/")[:index])
                    for name in names
                    for index in range(1, len(name.split("/")))
                }
                if len(names) != len(set(names)) or files & parents:
                    errors.append(f"duplicate native export archive paths: {path.name}")
                for info in sorted(members, key=lambda item: item.filename):
                    member_rel = f"{path.name}!/{info.filename}"
                    member_path = info.filename.removesuffix("/")
                    if (
                        info.orig_filename != info.filename
                        or "\\" in member_path
                        or ":" in member_path
                        or any(ord(char) < 32 or ord(char) == 127 for char in info.orig_filename)
                        or any(part in ("", ".", "..") for part in member_path.split("/"))
                    ):
                        errors.append(f"unsafe native export archive path: {member_rel!r}")
                    member_name = Path(member_path).name.lower()
                    with archive.open(info) as handle:
                        if _export_directory(info):
                            if not (info.file_size or info.compress_size or handle.read(1)):
                                continue
                            errors.append(f"native export directory payload: {member_rel}")
                            handle.seek(0)
                        present.add(member_rel)
                        if openmp_digest is not None and "iomp" in member_name:
                            approved.setdefault(member_rel, openmp_digest)
                        digest = _scan_stream(
                            handle,
                            member_rel,
                            member_name,
                            approved,
                            errors,
                            allow_itt=allow_itt,
                            reject_video=True,
                            strip_opencv_ipp_notice=is_opencv,
                        )
                    if digest is not None:
                        _export_native_review(digest, member_rel, archive, path, errors)
        else:
            label = path.name
            present.add(label)
            if openmp_digest is not None and "iomp" in name:
                approved.setdefault(label, openmp_digest)
            with path.open("rb") as handle:
                digest = _scan_stream(
                    handle,
                    label,
                    name,
                    approved,
                    errors,
                    allow_itt=allow_itt,
                    reject_video=True,
                )
            if digest is not None:
                _export_native_review(digest, label, None, path, errors)
    missing = set(approved) - present
    errors.extend(f"missing LLVM OpenMP replacement: {name}" for name in sorted(missing))
    if errors:
        raise SystemExit("vendor library check failed:\n" + "\n".join(errors))


def render_toolchain_pins(
    artifacts: Path,
    release_tag: str,
    *,
    repo: str = "bavanchun/AriadShift",
) -> str:
    """Render the ``[toolchain]`` TOML table from build records.

    The records were downloaded from a ``toolchain-wheels-*`` release.
    """
    if not re.fullmatch(r"toolchain-wheels-[A-Za-z0-9][A-Za-z0-9._-]*", release_tag):
        raise SystemExit("toolchain release tag must be named toolchain-wheels-<version>")
    prefix = f"https://github.com/{repo}/releases/download/{release_tag}/"
    records = sorted(artifacts.rglob("*-build-*.json"))
    if not records:
        raise SystemExit(f"no *-build-*.json provenance records found in {artifacts}")
    lines = [
        "[toolchain]",
        f'release = "{release_tag}"',
        f'source = "https://github.com/{repo}/releases/tag/{release_tag}"',
    ]
    by_target: dict[str, dict[str, dict]] = {}
    for record_path in records:
        record = json.loads(record_path.read_text(encoding="utf-8"))
        target = record["target"]
        kind = record.get("kind", "opencv" if record_path.name.startswith("opencv-") else "torch")
        by_target.setdefault(target, {})[kind] = record
    for target in sorted(by_target):
        for kind in sorted(by_target[target]):
            record = by_target[target][kind]
            lines.extend(
                [
                    "",
                    f"[toolchain.targets.{target}.{kind}]",
                    f'build_key = "{record["build_key"]}"',
                    "files = [",
                ]
            )
            for name, meta in sorted(record["files"].items()):
                asset = meta.get("asset", name)
                lines.append(
                    f'    {{ name = "{name}", url = "{prefix}{asset}", '
                    f'sha256 = "{meta["sha256"]}", size = {meta["size"]} }},'
                )
            lines.append("]")
    return "\n".join(lines) + "\n"


def write_runtime_notices(notices: Path, libraries: list[dict]) -> None:
    """Copy the upstream release texts, verifying their recorded digests, and list their sources."""
    destination = notices / "runtime"
    destination.mkdir(parents=True, exist_ok=True)
    lines = ["C libraries in the standalone Python runtime (Tk/Tcl removed):", ""]
    for item in libraries:
        notice = item.get("notice")
        if notice is None:
            source = Path(__file__).with_name("licenses") / item["text"]
            if hashlib.sha256(source.read_bytes()).hexdigest() != item["sha256"]:
                raise SystemExit(f"runtime licence text digest mismatch: {source}")
            shutil.copyfile(source, destination / item["text"])
            notice = f"notices/runtime/{item['text']}"
        lines.extend(
            [
                f"{item['name']} {item['version']}: {item['license']}",
                f"  Source: {item['source']}",
                f"  Text: {notice}",
            ]
        )
        if "archive_member" in item:
            lines.append(f"  Release archive member: {item['archive_member']}")
    (notices / "runtime-libraries.txt").write_text("\n".join(lines) + "\n", encoding="utf-8")


def _is_licence_name(name: str) -> bool:
    return re.match(r"(LICEN[CS]E|COPYING|NOTICE|AUTHORS)", name, re.I) is not None


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--site", type=Path)
    parser.add_argument("--notices", type=Path)
    parser.add_argument("--wheels", type=Path, nargs="+")
    parser.add_argument("--openmp-dll", type=Path)
    parser.add_argument("--allow-itt", action="store_true")
    parser.add_argument("--render-toolchain-pins", type=Path)
    parser.add_argument("--release-tag")
    args = parser.parse_args(argv)
    if args.render_toolchain_pins is not None:
        if not args.release_tag:
            parser.error("--render-toolchain-pins requires --release-tag")
        sys.stdout.write(render_toolchain_pins(args.render_toolchain_pins, args.release_tag))
        return 0
    if args.wheels is not None:
        check_wheel_files(
            args.wheels,
            allow_itt=args.allow_itt,
            openmp_dll=args.openmp_dll,
        )
        print(f"{len(args.wheels)} wheel artifact(s) passed the vendor library gate")
        return 0
    if args.site is None:
        parser.error("one of --site, --wheels, or --render-toolchain-pins is required")
    distributions = list(metadata.distributions(path=[str(args.site)]))
    findings = check(distributions)
    for finding in findings:
        print(
            f"{finding.distribution}: {finding.license}: {finding.reason}",
            file=sys.stderr,
        )
    if findings:
        return 1
    if args.notices is not None:
        write_notices(distributions, args.notices)
    print(f"{len(distributions)} distributions passed the licence gate")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
