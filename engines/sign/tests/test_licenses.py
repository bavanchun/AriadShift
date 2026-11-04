"""Tests for licence validation of the sign engine."""

from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

_core_path = Path(__file__).resolve().parents[2] / "common" / "licenses" / "core.py"
_spec = importlib.util.spec_from_file_location("engines_common_licenses_core", _core_path)
if _spec is None or _spec.loader is None:
    raise ImportError(f"could not load shared licence core from {_core_path}")
_core = importlib.util.module_from_spec(_spec)
sys.modules[_spec.name] = _core
_spec.loader.exec_module(_core)

ALLOWED = _core.ALLOWED
LicenceGate = _core.LicenceGate
parse_expression = _core.parse_expression


def test_sign_pack_allowed_licences():
    # Verify allowed licences for key sign dependencies
    assert parse_expression("MIT")  # pyHanko
    assert parse_expression("Apache-2.0")  # cryptography is Apache-2.0 OR BSD-3-Clause
    assert parse_expression("BSD-3-Clause")  # click, etc.
    assert parse_expression("PSF-2.0")


def test_sign_gate_rejects_copyleft():
    gate = LicenceGate(allowed=ALLOWED)
    assert gate.allowed == ALLOWED
    assert not parse_expression("GPL-3.0-only")
    assert not parse_expression("AGPL-3.0-only")
