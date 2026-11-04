"""Tests for licence validation of the sign engine."""

from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

_pack_licenses_path = Path(__file__).resolve().parents[1] / "pack" / "licenses.py"
_spec = importlib.util.spec_from_file_location("sign_pack_licenses", _pack_licenses_path)
if _spec is None or _spec.loader is None:
    raise ImportError(f"could not load sign pack licenses from {_pack_licenses_path}")
licenses = importlib.util.module_from_spec(_spec)
sys.modules[_spec.name] = licenses
_spec.loader.exec_module(licenses)


def test_sign_pack_allowed_licences():
    """Verify allowed licences for key sign dependencies."""
    assert licenses.parse_expression("MIT")
    assert licenses.parse_expression("Apache-2.0")
    assert licenses.parse_expression("BSD-3-Clause")
    assert licenses.parse_expression("PSF-2.0")
    assert licenses.verdict("cffi", "MIT-0") is None
    assert licenses.verdict("certifi", "MPL-2.0") is None


def test_sign_gate_rejects_copyleft():
    """Verify that copyleft and unreviewed licences are rejected."""
    assert licenses.verdict("test", "GPL-3.0-only") is not None
    assert licenses.verdict("test", "AGPL-3.0-only") is not None
    assert licenses.verdict("test", "MPL-2.0") is not None  # Not in MPL_EXCEPTIONS


def test_sign_locked_environment_audit():
    """Verify that the locked Python environment passes all licence and notice checks."""
    findings = licenses.check_environment()
    assert findings == [], f"Licence findings in environment: {findings}"


def test_reviewed_inventory_covers_31_packages():
    """Verify that licence-review.toml contains all 31 locked packages."""
    inv = licenses.load_reviewed_inventory()
    assert len(inv) == 31
