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


def test_license_gate_tampered_record_rejected(tmp_path):
    """Verify that a native file with mismatched RECORD hash is rejected."""
    dist_info = tmp_path / "cffi-2.1.1.dist-info"
    dist_info.mkdir()
    (dist_info / "METADATA").write_text("Name: cffi\nVersion: 2.1.1\nLicense: MIT-0\n", encoding="utf-8")
    (dist_info / "LICENSE").write_text("MIT-0 notice text\n", encoding="utf-8")

    so_file = tmp_path / "_cffi_backend.cpython-314-x86_64-linux-gnu.so"
    so_file.write_bytes(b"\x7fELFfake_cffi_binary_content")

    # Write RECORD with tampered/wrong sha256
    record_content = (
        "_cffi_backend.cpython-314-x86_64-linux-gnu.so,sha256=invalidhashvalue123456789012345678901234567890,26\n"
        "cffi-2.1.1.dist-info/METADATA,,\n"
        "cffi-2.1.1.dist-info/LICENSE,,\n"
        "cffi-2.1.1.dist-info/RECORD,,\n"
    )
    (dist_info / "RECORD").write_text(record_content, encoding="utf-8")

    import importlib.metadata as md
    dists = list(md.distributions(path=[str(tmp_path)]))
    assert len(dists) == 1
    inv = licenses.load_reviewed_inventory()
    findings = licenses.check_distribution(dists[0], inv, site_root=tmp_path)
    assert any("RECORD SHA-256 or size mismatch" in f.reason for f in findings)


def test_license_gate_missing_record_hash_rejected(tmp_path):
    """Verify that a native file with missing hash in RECORD is rejected."""
    dist_info = tmp_path / "cffi-2.1.1.dist-info"
    dist_info.mkdir()
    (dist_info / "METADATA").write_text("Name: cffi\nVersion: 2.1.1\nLicense: MIT-0\n", encoding="utf-8")
    (dist_info / "LICENSE").write_text("MIT-0 notice text\n", encoding="utf-8")

    so_file = tmp_path / "_cffi_backend.cpython-314-x86_64-linux-gnu.so"
    so_file.write_bytes(b"\x7fELFfake_cffi_binary_content")

    # Missing hash/size in RECORD
    record_content = (
        "_cffi_backend.cpython-314-x86_64-linux-gnu.so,,\n"
        "cffi-2.1.1.dist-info/RECORD,,\n"
    )
    (dist_info / "RECORD").write_text(record_content, encoding="utf-8")

    import importlib.metadata as md
    dists = list(md.distributions(path=[str(tmp_path)]))
    inv = licenses.load_reviewed_inventory()
    findings = licenses.check_distribution(dists[0], inv, site_root=tmp_path)
    assert any("RECORD SHA-256 or size mismatch" in f.reason for f in findings)


def test_license_gate_missing_notice_rejected(tmp_path):
    """Verify that a distribution without any notice/license file in dist-info is rejected."""
    dist_info = tmp_path / "iniconfig-2.3.1.dist-info"
    dist_info.mkdir()
    (dist_info / "METADATA").write_text("Name: iniconfig\nVersion: 2.3.1\nLicense: MIT\n", encoding="utf-8")
    (dist_info / "RECORD").write_text("iniconfig-2.3.1.dist-info/RECORD,,\n", encoding="utf-8")

    import importlib.metadata as md
    dists = list(md.distributions(path=[str(tmp_path)]))
    inv = licenses.load_reviewed_inventory()
    findings = licenses.check_distribution(dists[0], inv, site_root=tmp_path)
    assert any("missing or empty license or notice file" in f.reason for f in findings)


def test_license_gate_empty_notice_rejected(tmp_path):
    """Verify that an empty (0-byte) notice file in dist-info is rejected."""
    dist_info = tmp_path / "iniconfig-2.3.1.dist-info"
    dist_info.mkdir()
    (dist_info / "METADATA").write_text("Name: iniconfig\nVersion: 2.3.1\nLicense: MIT\n", encoding="utf-8")
    (dist_info / "LICENSE").write_bytes(b"")  # 0 bytes
    (dist_info / "RECORD").write_text("iniconfig-2.3.1.dist-info/RECORD,,\n", encoding="utf-8")

    import importlib.metadata as md
    dists = list(md.distributions(path=[str(tmp_path)]))
    inv = licenses.load_reviewed_inventory()
    findings = licenses.check_distribution(dists[0], inv, site_root=tmp_path)
    assert any("missing or empty license or notice file" in f.reason for f in findings)


def test_license_gate_unapproved_native_file_rejected(tmp_path):
    """Verify that an unapproved native object in a native package is rejected."""
    import base64
    import hashlib

    dist_info = tmp_path / "cffi-2.1.1.dist-info"
    dist_info.mkdir()
    (dist_info / "METADATA").write_text("Name: cffi\nVersion: 2.1.1\nLicense: MIT-0\n", encoding="utf-8")
    (dist_info / "LICENSE").write_text("MIT-0 notice text\n", encoding="utf-8")

    # Create unapproved native file
    unapproved_so = tmp_path / "unexpected_backend.so"
    data = b"\x7fELFunapproved_native_bytes"
    unapproved_so.write_bytes(data)
    digest = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).decode().rstrip("=")

    record_content = (
        f"unexpected_backend.so,sha256={digest},{len(data)}\n"
        "cffi-2.1.1.dist-info/RECORD,,\n"
    )
    (dist_info / "RECORD").write_text(record_content, encoding="utf-8")

    import importlib.metadata as md
    dists = list(md.distributions(path=[str(tmp_path)]))
    inv = licenses.load_reviewed_inventory()
    findings = licenses.check_distribution(dists[0], inv, site_root=tmp_path)
    assert any("unapproved native object" in f.reason for f in findings)


def test_license_gate_native_in_non_native_package_rejected(tmp_path):
    """Verify that a native object in a non-native package is rejected."""
    dist_info = tmp_path / "asn1crypto-1.5.1.dist-info"
    dist_info.mkdir()
    (dist_info / "METADATA").write_text("Name: asn1crypto\nVersion: 1.5.1\nLicense: MIT\n", encoding="utf-8")
    (dist_info / "LICENSE").write_text("MIT notice text\n", encoding="utf-8")

    so_file = tmp_path / "asn1crypto" / "_fast_parser.so"
    so_file.parent.mkdir()
    so_file.write_bytes(b"\x7fELFfast_parser")

    record_content = (
        "asn1crypto/_fast_parser.so,,\n"
        "asn1crypto-1.5.1.dist-info/RECORD,,\n"
    )
    (dist_info / "RECORD").write_text(record_content, encoding="utf-8")

    import importlib.metadata as md
    dists = list(md.distributions(path=[str(tmp_path)]))
    inv = licenses.load_reviewed_inventory()
    findings = licenses.check_distribution(dists[0], inv, site_root=tmp_path)
    assert any("unexpected native binary object in non-native package" in f.reason for f in findings)
