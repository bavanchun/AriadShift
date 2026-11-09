"""Shared licence gate and notices for AriadShift engine packs.

Extracts the common SPDX validation, expression evaluation, metadata inspection,
and notice gathering mechanics used across Docling, sign, and future engine packs.
"""

from __future__ import annotations

from .core import (
    ALLOWED,
    ALLOWED_DATA,
    ALLOWED_EXCEPTIONS,
    Finding,
    LicenceGate,
    canonical,
    check_distributions,
    license_of_distribution,
    parse_expression,
    verdict,
)

__all__ = [
    "ALLOWED",
    "ALLOWED_DATA",
    "ALLOWED_EXCEPTIONS",
    "Finding",
    "LicenceGate",
    "canonical",
    "check_distributions",
    "license_of_distribution",
    "parse_expression",
    "verdict",
]
