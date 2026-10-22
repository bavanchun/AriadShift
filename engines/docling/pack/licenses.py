"""Licence gate and notices for everything the docling pack bundles.

Distributed artifacts must not contain AGPL, GPL or non-OSI code, and model weights must not
use OpenRAIL-M, custom or missing licences. Each Python distribution's licence is read from its
metadata (``License-Expression``, then ``License``, then trove classifiers) and checked against
an allow-list; anything unrecognised fails until it is reviewed and listed here.

Usage::

    python licenses.py --site SITE_PACKAGES [--notices DIR]
"""

from __future__ import annotations

import argparse
import re
import shutil
import sys
from collections.abc import Iterable
from dataclasses import dataclass
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


def _is_licence_name(name: str) -> bool:
    return re.match(r"(LICEN[CS]E|COPYING|NOTICE|AUTHORS)", name, re.I) is not None


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--site", type=Path, required=True)
    parser.add_argument("--notices", type=Path)
    args = parser.parse_args(argv)
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
