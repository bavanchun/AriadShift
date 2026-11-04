"""Tests for the shared licence gate in engines/common/licenses."""

from __future__ import annotations

from email.message import Message
from importlib import metadata
import importlib.util
from pathlib import Path
import sys

import pytest

_core_path = Path(__file__).resolve().parents[1] / "licenses" / "core.py"
_spec = importlib.util.spec_from_file_location("common_licenses", _core_path)
if _spec is None or _spec.loader is None:
    raise ImportError(f"could not load shared licence core from {_core_path}")
_core = importlib.util.module_from_spec(_spec)
sys.modules[_spec.name] = _core
_spec.loader.exec_module(_core)

ALLOWED = _core.ALLOWED
ALLOWED_EXCEPTIONS = _core.ALLOWED_EXCEPTIONS
Finding = _core.Finding
LicenceGate = _core.LicenceGate
check_distributions = _core.check_distributions
license_of_distribution = _core.license_of_distribution
parse_expression = _core.parse_expression
verdict = _core.verdict


class DummyDistribution(metadata.Distribution):
    def __init__(self, name: str, **fields: str | list[str]) -> None:
        self._message = Message()
        self._message["Name"] = name
        self._message["Version"] = "1.0"
        for key, value in fields.items():
            for item in value if isinstance(value, list) else [value]:
                self._message[key.replace("_", "-")] = item

    @property
    def metadata(self):  # type: ignore[override]
        return self._message

    def read_text(self, filename):
        return None

    def locate_file(self, path):
        raise NotImplementedError


def test_parse_expression_allowed_and_denied():
    assert parse_expression("MIT")
    assert parse_expression("Apache-2.0")
    assert parse_expression("Apache-2.0 WITH LLVM-exception AND MIT")
    assert parse_expression("BSD-3-Clause OR MIT")
    assert not parse_expression("GPL-3.0-only")
    assert not parse_expression("AGPL-3.0")
    assert not parse_expression("Proprietary")


def test_custom_gate_with_exceptions():
    gate = LicenceGate(
        allowed=ALLOWED,
        allowed_exceptions=ALLOWED_EXCEPTIONS,
        mpl_exceptions=frozenset({"my-mpl-pkg"}),
    )

    d_mpl = DummyDistribution("my-mpl-pkg", License_Expression="MPL-2.0")
    assert gate.check([d_mpl]) == []

    d_unapproved_mpl = DummyDistribution("other-pkg", License_Expression="MPL-2.0")
    findings = gate.check([d_unapproved_mpl])
    assert len(findings) == 1
    assert "not in the allow-list" in findings[0].reason

    d_mit = DummyDistribution("mit-pkg", License_Expression="MIT")
    assert gate.check([d_mit]) == []

    d_gpl = DummyDistribution("gpl-pkg", License_Expression="GPL-3.0-only")
    findings = gate.check([d_gpl])
    assert len(findings) == 1
    assert "excluded licence" in findings[0].reason


def test_verdict_reports_findings():
    v_bad = verdict("bad-pkg", "GPL-3.0")
    assert v_bad == "excluded licence"

    v_good = verdict("good-pkg", "MIT")
    assert v_good is None
