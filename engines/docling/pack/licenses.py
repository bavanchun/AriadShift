"""Licence gate and notices for everything the docling pack bundles.

Distributed artifacts must not contain AGPL, GPL or non-OSI code, and model weights must not
use OpenRAIL-M, custom or missing licences. Each Python distribution's licence is read from its
metadata (``License-Expression``, then ``License``, then trove classifiers) and checked against
an allow-list; anything unrecognised fails until it is reviewed and listed here.

Usage::

    python licenses.py --site SITE_PACKAGES [--notices DIR]
    python licenses.py --wheels WHEEL [WHEEL ...] [--openmp-dll PATH] [--allow-itt]
    python licenses.py --render-toolchain-pins DIR --release-tag TAG
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
import sys
import tomllib
import zipfile
from collections.abc import Iterable
from dataclasses import dataclass
from fnmatch import fnmatchcase
from importlib import metadata
from pathlib import Path

ALLOWED = frozenset(
    {
        "0BSD",
        "Apache-2.0",
        "BSD-2-Clause",
        "BSD-3-Clause",
        "BSL-1.0",
        "CC0-1.0",
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
ALLOWED_DATA = frozenset({"Apache-2.0", "CDLA-Permissive-2.0", "MIT", "PSF-2.0"})

_DENIED = re.compile(r"AGPL|(?<!L)GPL|SSPL|RAIL|BUSL|Commons-Clause|NC-|Proprietary", re.I)

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


@dataclass(frozen=True)
class Finding:
    distribution: str
    license: str
    reason: str


def canonical(name: str) -> str:
    return re.sub(r"[-_.]+", "-", name).lower()


def license_of(distribution: metadata.Distribution) -> str | None:
    """The SPDX expression for a distribution, or ``None`` when it cannot be determined."""
    name = canonical(distribution.metadata["Name"])
    if name in OVERRIDES:
        return OVERRIDES[name]
    expression = distribution.metadata.get("License-Expression")
    if expression:
        return expression.strip()
    field = (distribution.metadata.get("License") or "").strip()
    if field.lower() in _KNOWN:
        return _KNOWN[field.lower()]
    classifiers = [
        item.split("::")[-1].strip().lower()
        for item in distribution.metadata.get_all("Classifier") or []
        if item.startswith("License ::")
    ]
    known = [_KNOWN[item] for item in classifiers if item in _KNOWN]
    if known:
        return " OR ".join(dict.fromkeys(known))
    return None


def verdict(name: str, expression: str) -> str | None:
    """Returns why an expression is not acceptable, or ``None`` when it is."""
    if _DENIED.search(expression):
        return "excluded licence"
    try:
        allowed = _Expression(name, expression).allowed()
    except ValueError:
        return "not a valid SPDX expression"
    return None if allowed else "not in the allow-list"


class _Expression:
    """Evaluates an SPDX expression against the allow-list.

    ``OR`` lets the distributor pick any alternative, ``AND`` needs every operand, and
    ``WITH`` accepts only the reviewed exceptions. Anything the grammar does not cover is an
    error, which the gate treats as a refusal.
    """

    def __init__(self, name: str, expression: str) -> None:
        self._name = name
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
        return any(results)

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
        if licence == "MPL-2.0":
            return self._name in MPL_EXCEPTIONS
        return licence in ALLOWED


def check(distributions: Iterable[metadata.Distribution]) -> list[Finding]:
    findings = []
    for distribution in distributions:
        name = canonical(distribution.metadata["Name"])
        expression = license_of(distribution)
        if expression is None:
            findings.append(Finding(name, "unknown", "licence could not be determined"))
            continue
        reason = verdict(name, expression)
        if reason is not None:
            findings.append(Finding(name, expression, reason))
    return findings


def write_notices(distributions: Iterable[metadata.Distribution], notices: Path) -> None:
    """Copies every distribution's licence files and writes a summary table."""
    root = notices / "python"
    rows = []
    for distribution in sorted(distributions, key=lambda item: canonical(item.metadata["Name"])):
        name = canonical(distribution.metadata["Name"])
        version = distribution.version
        rows.append(f"{name} {version}: {license_of(distribution)}")
        target = root / f"{name}-{version}"
        for item in distribution.files or []:
            parts = item.parts
            if not parts or not parts[0].endswith(".dist-info") or len(parts) < 2:
                continue
            if not (parts[1] == "licenses" or _is_licence_name(parts[-1])):
                continue
            source = Path(str(item.locate()))
            if source.is_file():
                destination = target.joinpath(*parts[1:])
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(source, destination)
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
    platform = (
        "windows"
        if target.endswith("windows-msvc")
        else "macos"
        if target.endswith("apple-darwin")
        else "linux"
    )
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
    platform = (
        "windows"
        if target.endswith("windows-msvc")
        else ("macos" if target.endswith("apple-darwin") else "linux")
    )
    files = {
        path.relative_to(runtime).as_posix()
        for path in runtime.rglob("*")
        if path.is_file()
        and "site-packages" not in path.relative_to(runtime).parts
        and any(
            fnmatchcase(path.name.lower(), pattern)
            for pattern in ("*.so*", "*.dylib", "*.pyd", "*.dll")
        )
    }
    covered = set()
    errors = []
    for library in libraries:
        patterns = library.get("native_files", {}).get(platform, [])
        matches = {name for name in files if any(fnmatchcase(name, p) for p in patterns)}
        if not matches:
            errors.append(f"{library['name']}: inventory patterns match no native files")
        covered.update(matches)
    errors.extend(f"unlisted native runtime file: {name}" for name in sorted(files - covered))
    if errors:
        raise SystemExit("runtime inventory check failed:\n" + "\n".join(errors))


_INTEL_LIBRARIES = ("iomp", "ippicv", "libipp", "libmkl", "mkl_rt", "mkl_core")
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


def _scan_stream(
    handle,
    relative: str,
    name: str,
    approved_openmp: dict[str, str],
    errors: list[str],
    *,
    allow_itt: bool = True,
    strip_opencv_ipp_notice: bool = False,
) -> None:
    native = any(
        fnmatchcase(name, p)
        for p in ("*.so*", "*.dll", "*.pyd", "*.dylib", "*.exe", "*.a", "*.lib")
    )
    libraries = _INTEL_LIBRARIES if allow_itt else (*_INTEL_LIBRARIES, *_ITT_LIBRARIES)
    if any(word in name for word in libraries):
        data = handle.read()
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
        iterator = iter(chunks) if chunks is not None else iter(lambda: handle.read(1 << 20), b"")
        for chunk in iterator:
            data = (tail + chunk).lower()
            if any(marker in data for marker in markers):
                errors.append(f"proprietary Intel binary content: {relative}")
                break
            tail = data[-128:]
    elif any(word in name for word in ("license", "licence", "notice", "copying")):
        raw = b"".join(chunks) if chunks is not None else handle.read()
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


def check_wheel_files(
    wheels: Iterable[Path],
    approved_openmp: dict[str, str] | None = None,
    *,
    allow_itt: bool = False,
    openmp_dll: Path | None = None,
) -> None:
    """Audit built wheel archives and optional OpenMP sidecars for forbidden Intel components.

    Source-built toolchain wheels disable ITT instrumentation (``USE_ITT=0``,
    ``-DWITH_ITT=OFF``, ``-DLIBOMP_USE_ITT_NOTIFY=OFF``), so ``allow_itt``
    defaults to ``False`` here.
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
            _scan_stream(
                handle,
                label,
                openmp_dll.name.lower(),
                approved,
                errors,
                allow_itt=allow_itt,
            )
    for path in wheel_list:
        if not path.is_file():
            raise SystemExit(f"missing wheel or artifact file: {path}")
        name = path.name.lower()
        if name.endswith(".whl"):
            is_opencv = name.startswith("opencv_python_headless-")
            with zipfile.ZipFile(path) as archive:
                for info in sorted(archive.infolist(), key=lambda item: item.filename):
                    if info.is_dir():
                        continue
                    member_rel = f"{path.name}!/{info.filename}"
                    present.add(member_rel)
                    member_name = Path(info.filename).name.lower()
                    if openmp_digest is not None and "iomp" in member_name:
                        approved.setdefault(member_rel, openmp_digest)
                    with archive.open(info) as handle:
                        _scan_stream(
                            handle,
                            member_rel,
                            member_name,
                            approved,
                            errors,
                            allow_itt=allow_itt,
                            strip_opencv_ipp_notice=is_opencv,
                        )
        else:
            label = path.name
            present.add(label)
            if openmp_digest is not None and "iomp" in name:
                approved.setdefault(label, openmp_digest)
            with path.open("rb") as handle:
                _scan_stream(
                    handle,
                    label,
                    name,
                    approved,
                    errors,
                    allow_itt=allow_itt,
                )
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
