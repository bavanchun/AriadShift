"""Licence check for the sign engine dependencies and pack."""

from __future__ import annotations

import importlib.util
from pathlib import Path
import sys

_core_path = Path(__file__).resolve().parents[2] / "common" / "licenses" / "core.py"
_spec = importlib.util.spec_from_file_location("engines_common_licenses_core", _core_path)
if _spec is None or _spec.loader is None:
    raise ImportError(f"could not load shared licence core from {_core_path}")
_core = importlib.util.module_from_spec(_spec)
sys.modules[_spec.name] = _core
_spec.loader.exec_module(_core)

ALLOWED = _core.ALLOWED
ALLOWED_DATA = _core.ALLOWED_DATA
ALLOWED_EXCEPTIONS = _core.ALLOWED_EXCEPTIONS
Finding = _core.Finding
LicenceGate = _core.LicenceGate
check_distributions = _core.check_distributions
license_of_distribution = _core.license_of_distribution
parse_expression = _core.parse_expression
verdict = _core.verdict
