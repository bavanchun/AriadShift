"""Core SPDX evaluation and distribution licence verification."""

from __future__ import annotations

import re
from collections.abc import Iterable
from dataclasses import dataclass
from importlib import metadata

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

ALLOWED_EXCEPTIONS = frozenset({"LLVM-exception"})

ALLOWED_DATA = frozenset(
    {"Apache-2.0", "CDLA-Permissive-2.0", "CC0-1.0", "MIT", "PSF-2.0"}
)

_DENIED = re.compile(
    r"AGPL|(?<!L)GPL|SSPL|RAIL|BUSL|Commons-Clause|NC-|Proprietary", re.IGNORECASE
)
_RAW_DENIED = re.compile(r"(?<!Lesser )General Public License|Affero", re.IGNORECASE)
_GPL_RUNTIME = re.compile(
    rb"lib[\w-]+ licen[sc]e:\s*(?:A?GPL)\b|(?:^|\s)--enable-gpl(?:\s|\x00|$)",
    re.IGNORECASE,
)

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


@dataclass(frozen=True)
class Finding:
    distribution: str
    license: str
    reason: str


def canonical(name: str) -> str:
    return re.sub(r"[-_.]+", "-", name).lower()


class _Expression:
    """Evaluates an SPDX expression against the allow-list.

    Every ``OR`` alternative must be known and allowed; ``AND`` needs every operand, and
    ``WITH`` accepts only the reviewed exceptions.
    """

    def __init__(
        self,
        name: str,
        expression: str,
        *,
        allowed: frozenset[str] = ALLOWED,
        allowed_exceptions: frozenset[str] = ALLOWED_EXCEPTIONS,
        mpl_exceptions: frozenset[str] = frozenset(),
        metadata_only: bool = False,
    ) -> None:
        self._name = name
        self._allowed = allowed
        self._allowed_exceptions = allowed_exceptions
        self._mpl_exceptions = mpl_exceptions
        self._metadata_only = metadata_only
        self._tokens = re.findall(r"\(|\)|[^\s()]+", expression)
        self._position = 0

    def allowed(self) -> bool:
        result = self._or()
        if self._position != len(self._tokens):
            raise ValueError("unexpected token")
        return result

    def _peek(self) -> str | None:
        return (
            self._tokens[self._position] if self._position < len(self._tokens) else None
        )

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
            return exception in self._allowed_exceptions and self._licence_allowed(word)
        return self._licence_allowed(word)

    def _licence_allowed(self, licence: str) -> bool:
        if licence == "CC0-1.0":
            return self._metadata_only
        if licence == "MPL-2.0":
            return self._name in self._mpl_exceptions
        return licence in self._allowed


def parse_expression(
    expression: str,
    name: str = "",
    *,
    allowed: frozenset[str] = ALLOWED,
    allowed_exceptions: frozenset[str] = ALLOWED_EXCEPTIONS,
    mpl_exceptions: frozenset[str] = frozenset(),
    metadata_only: bool = False,
) -> bool:
    """Evaluates an SPDX expression, returning True if all requirements are met."""
    if _DENIED.search(expression):
        return False
    try:
        return _Expression(
            name,
            expression,
            allowed=allowed,
            allowed_exceptions=allowed_exceptions,
            mpl_exceptions=mpl_exceptions,
            metadata_only=metadata_only,
        ).allowed()
    except ValueError:
        return False


def verdict(
    name: str,
    expression: str,
    *,
    allowed: frozenset[str] = ALLOWED,
    allowed_exceptions: frozenset[str] = ALLOWED_EXCEPTIONS,
    mpl_exceptions: frozenset[str] = frozenset(),
    metadata_only: bool = False,
) -> str | None:
    """Returns why an expression is not acceptable, or ``None`` when it is."""
    if _DENIED.search(expression):
        return "excluded licence"
    try:
        expr_allowed = _Expression(
            name,
            expression,
            allowed=allowed,
            allowed_exceptions=allowed_exceptions,
            mpl_exceptions=mpl_exceptions,
            metadata_only=metadata_only,
        ).allowed()
    except ValueError:
        return "not a valid SPDX expression"
    return None if expr_allowed else "not in the allow-list"


def _split_expression(expression: str, operator: str) -> list[str]:
    """Split an SPDX expression by a binary operator outside parentheses."""
    tokens = re.findall(r"\(|\)|[^\s()]+", expression)
    while tokens and tokens[0] == "(":
        depth = 0
        end_idx = 0
        for end_idx, word in enumerate(tokens):
            depth += (word == "(") - (word == ")")
            if depth == 0:
                break
        if end_idx != len(tokens) - 1:
            break
        tokens = tokens[1:-1]
    parts: list[str] = []
    start = depth = 0
    for index, word in enumerate(tokens):
        depth += (word == "(") - (word == ")")
        if depth == 0 and word == operator:
            parts.append(" ".join(tokens[start:index]))
            start = index + 1
    parts.append(" ".join(tokens[start:]))
    return [p.strip() for p in parts if p.strip()]


def _licence_signature(expression: str) -> frozenset[frozenset[str]]:
    """Compare known declarations independent of operand order and parentheses."""
    alternatives = _split_expression(expression, "OR")
    if len(alternatives) > 1:
        return frozenset(
            term for part in alternatives for term in _licence_signature(part)
        )
    terms = _split_expression(expression, "AND")
    if len(terms) > 1:
        result = frozenset({frozenset()})
        for term in terms:
            result = frozenset(a | b for a in result for b in _licence_signature(term))
        return result
    return frozenset({frozenset(terms)})


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


@dataclass(frozen=True)
class LicenceGate:
    """Configurable licence gate for inspecting distributions and wheels."""

    allowed: frozenset[str] = ALLOWED
    allowed_exceptions: frozenset[str] = ALLOWED_EXCEPTIONS
    allowed_data: frozenset[str] = ALLOWED_DATA
    mpl_exceptions: frozenset[str] = frozenset()
    known: dict[str, str] = None  # type: ignore[assignment]
    overrides: dict[str, str] = None  # type: ignore[assignment]

    def __post_init__(self) -> None:
        if self.known is None:
            object.__setattr__(self, "known", dict(_KNOWN))
        if self.overrides is None:
            object.__setattr__(self, "overrides", {})

    def check(self, distributions: Iterable[metadata.Distribution]) -> list[Finding]:
        return check_distributions(
            distributions,
            allowed=self.allowed,
            allowed_exceptions=self.allowed_exceptions,
            mpl_exceptions=self.mpl_exceptions,
            known=self.known,
            overrides=self.overrides,
        )

    def license_of(self, distribution: metadata.Distribution) -> str | None:
        return license_of_distribution(
            distribution,
            allowed=self.allowed,
            allowed_exceptions=self.allowed_exceptions,
            mpl_exceptions=self.mpl_exceptions,
            known=self.known,
            overrides=self.overrides,
        )


def license_of_distribution(
    distribution: metadata.Distribution,
    *,
    allowed: frozenset[str] = ALLOWED,
    allowed_exceptions: frozenset[str] = ALLOWED_EXCEPTIONS,
    mpl_exceptions: frozenset[str] = frozenset(),
    known: dict[str, str] | None = None,
    overrides: dict[str, str] | None = None,
) -> str | None:
    """Resolve declared fields of a distribution against allow-list and known classifiers."""
    if _duplicate_licence_fields(distribution) or _empty_licence_fields(distribution):
        return None
    name = canonical(distribution.metadata["Name"])
    known_map = known if known is not None else _KNOWN
    overrides_map = overrides if overrides is not None else {}

    field = (distribution.metadata.get("License") or "").strip()
    if field and field.lower() not in known_map and name not in overrides_map:
        return None

    expression = (distribution.metadata.get("License-Expression") or "").strip()
    claims = [expression] if expression else []
    if field:
        claims.append(overrides_map.get(name, known_map.get(field.lower(), "")))

    for classifier in _licence_classifiers(distribution):
        parts = [part.strip().casefold() for part in classifier.split("::")]
        if parts == ["license", "osi approved"]:
            continue
        if len(parts) < 2 or parts[-1] not in known_map:
            return None
        claims.append(known_map[parts[-1]])

    if not claims:
        return None

    for claim in claims:
        v = verdict(
            name,
            claim,
            allowed=allowed,
            allowed_exceptions=allowed_exceptions,
            mpl_exceptions=mpl_exceptions,
        )
        if v is not None:
            return None

    signatures = {_licence_signature(c) for c in claims if c}
    return claims[0] if len(signatures) == 1 else None


def check_distributions(
    distributions: Iterable[metadata.Distribution],
    *,
    allowed: frozenset[str] = ALLOWED,
    allowed_exceptions: frozenset[str] = ALLOWED_EXCEPTIONS,
    mpl_exceptions: frozenset[str] = frozenset(),
    known: dict[str, str] | None = None,
    overrides: dict[str, str] | None = None,
) -> list[Finding]:
    """Audits installed distributions and returns findings for unreviewed or non-compliant ones."""
    findings = []
    for distribution in distributions:
        name = canonical(distribution.metadata["Name"])
        if _duplicate_licence_fields(distribution):
            findings.append(
                Finding(name, "unknown", "duplicate single-value licence field")
            )
            continue
        licence = license_of_distribution(
            distribution,
            allowed=allowed,
            allowed_exceptions=allowed_exceptions,
            mpl_exceptions=mpl_exceptions,
            known=known,
            overrides=overrides,
        )
        if licence is None:
            raw = (distribution.metadata.get("License") or "").strip()
            expr = (distribution.metadata.get("License-Expression") or "").strip()
            declared = expr or raw or "unknown"
            v = verdict(
                name,
                declared,
                allowed=allowed,
                allowed_exceptions=allowed_exceptions,
                mpl_exceptions=mpl_exceptions,
            )
            reason = v if v is not None else "unreviewed or missing licence declaration"
            findings.append(Finding(name, declared, reason))
            continue
        v = verdict(
            name,
            licence,
            allowed=allowed,
            allowed_exceptions=allowed_exceptions,
            mpl_exceptions=mpl_exceptions,
        )
        if v is not None:
            findings.append(Finding(name, licence, v))
    return findings
