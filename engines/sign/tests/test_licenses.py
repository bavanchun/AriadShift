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


# --- Site-packages tree scan and explicit native approvals -------------------------------------

import base64
import hashlib
import importlib.metadata as md


def _make_dist(site: Path, name: str, version: str, spdx: str, files: dict[str, bytes]) -> None:
    """Writes a minimal installed distribution with a correct RECORD for every file."""
    dist_info = site / f"{name}-{version}.dist-info"
    dist_info.mkdir()
    (dist_info / "METADATA").write_text(
        f"Name: {name}\nVersion: {version}\nLicense: {spdx}\n", encoding="utf-8"
    )
    (dist_info / "LICENSE").write_text("notice text\n", encoding="utf-8")
    rows = []
    for rel, data in files.items():
        target = site / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
        digest = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).decode().rstrip("=")
        rows.append(f"{rel},sha256={digest},{len(data)}")
    rows += [f"{name}-{version}.dist-info/LICENSE,,", f"{name}-{version}.dist-info/RECORD,,"]
    (dist_info / "RECORD").write_text("\n".join(rows) + "\n", encoding="utf-8")


def _tree_findings(site: Path) -> list:
    return licenses.check_site_tree(site, licenses.load_reviewed_inventory())


def _reasons(findings) -> str:
    return "\n".join(f.reason for f in findings)


def test_pth_archive_reference_rejected(tmp_path):
    (tmp_path / "evil.pth").write_text("# comment\n/opt/libs/bundle.zip\n", encoding="utf-8")
    assert "archive reference in site-packages path file: evil.pth" in _reasons(
        _tree_findings(tmp_path)
    )


def test_pth_executable_import_line_rejected(tmp_path):
    (tmp_path / "hook.pth").write_text("import os; os.system('true')\n", encoding="utf-8")
    assert "executable import line in site-packages path file: hook.pth" in _reasons(
        _tree_findings(tmp_path)
    )


def test_pth_plain_path_lines_and_comments_accepted(tmp_path):
    (tmp_path / "paths.pth").write_text("# note\n\nvendor\n", encoding="utf-8")
    assert _tree_findings(tmp_path) == []


def test_archive_suffix_rejected(tmp_path):
    for name in ("bundle.zip", "legacy.egg", "pkg.whl"):
        (tmp_path / name).write_bytes(b"x")
    reasons = _reasons(_tree_findings(tmp_path))
    for name in ("bundle.zip", "legacy.egg", "pkg.whl"):
        assert f"uninspected site-packages archive: {name}" in reasons


def test_zip_magic_in_disguised_file_rejected(tmp_path):
    (tmp_path / "data.bin").write_bytes(b"PK\x03\x04rest-of-zip")
    (tmp_path / "weights.npz").write_bytes(b"PK\x03\x04rest-of-zip")
    reasons = _reasons(_tree_findings(tmp_path))
    assert "uninspected site-packages ZIP archive: data.bin" in reasons
    assert "weights.npz" not in reasons


def test_unsafe_symlinks_rejected(tmp_path):
    outside = tmp_path.parent / f"{tmp_path.name}-outside"
    outside.mkdir()
    (outside / "secret.txt").write_text("x", encoding="utf-8")
    site = tmp_path / "site"
    site.mkdir()
    (site / "escape.txt").symlink_to(outside / "secret.txt")
    (site / "escape-dir").symlink_to(outside, target_is_directory=True)
    reasons = _reasons(_tree_findings(site))
    assert "unsafe site-packages symlink: escape.txt" in reasons
    assert "unsafe site-packages symlink: escape-dir" in reasons


def test_pip_vendored_launcher_executables_rejected(tmp_path):
    """pip is skipped by the metadata audit, but its launcher executables must not pass."""
    _make_dist(
        tmp_path,
        "pip",
        "26.0",
        "MIT",
        {"pip/_vendor/distlib/t64.exe": b"MZ\x90\x00launcher", "pip/__init__.py": b""},
    )
    reasons = _reasons(_tree_findings(tmp_path))
    assert "native file not owned by a reviewed package: pip/_vendor/distlib/t64.exe" in reasons


def test_unowned_executable_and_magic_only_binary_rejected(tmp_path):
    (tmp_path / "tool.exe").write_bytes(b"MZ\x90\x00")
    (tmp_path / "hidden-binary").write_bytes(b"\x7fELF\x02\x01\x01")
    reasons = _reasons(_tree_findings(tmp_path))
    assert "native file not owned by a reviewed package: tool.exe" in reasons
    assert "native file not owned by a reviewed package: hidden-binary" in reasons


def test_magic_only_binary_in_reviewed_pure_package_rejected(tmp_path):
    _make_dist(tmp_path, "asn1crypto", "1.5.1", "MIT", {"asn1crypto/blob": b"\xcf\xfa\xed\xfe\x00"})
    assert "unapproved native object (detected by file magic): asn1crypto/blob" in _reasons(
        _tree_findings(tmp_path)
    )


def test_native_hard_link_rejected(tmp_path):
    (tmp_path / "a.so").write_bytes(b"\x7fELFx")
    (tmp_path / "b.so").hardlink_to(tmp_path / "a.so")
    assert "native file is a hard link" in _reasons(_tree_findings(tmp_path))


def test_check_environment_runs_tree_scan_for_site_root(tmp_path):
    (tmp_path / "bundle.zip").write_bytes(b"x")
    findings = licenses.check_environment([], site_root=tmp_path)
    assert any("uninspected site-packages archive" in f.reason for f in findings)


def test_lxml_libiconv_notice_recorded_and_bundled():
    lxml = licenses.load_reviewed_inventory()["lxml"]
    assert "libiconv-LICENSE.txt" in lxml["notices"]
    assert "libxml2-libxslt-LICENSES.txt" in lxml["notices"]
    text = (licenses.NOTICES_DIR / "libiconv-LICENSE.txt").read_text(encoding="utf-8")
    assert "GNU LESSER GENERAL PUBLIC LICENSE" in text
    assert "Version 2.1" in text
    assert "https://ftp.gnu.org/gnu/libiconv/" in text
    wheel_notice = (licenses.NOTICES_DIR / "libxml2-libxslt-LICENSES.txt").read_text(encoding="utf-8")
    assert "**iconv**: LGPL 2.1" in wheel_notice
    pack_script = (Path(__file__).resolve().parents[1] / "pack" / "build.py").read_text(
        encoding="utf-8"
    )
    assert "notices/libiconv-LICENSE.txt" in pack_script


def test_missing_bundled_notice_rejected(tmp_path, monkeypatch):
    monkeypatch.setattr(licenses, "NOTICES_DIR", tmp_path / "no-notices")
    site = tmp_path / "site"
    site.mkdir()
    _make_dist(site, "lxml", "6.1.3", "BSD-3-Clause", {})
    inv = licenses.load_reviewed_inventory()
    (found,) = md.distributions(path=[str(site)])
    findings = licenses.check_distribution(found, inv, site_root=site)
    assert "missing or empty bundled notice: libiconv-LICENSE.txt" in _reasons(findings)


def test_reviewed_inventory_has_no_wildcard_native_approvals():
    for name, pkg in licenses.load_reviewed_inventory().items():
        for entry in pkg.get("native_files", []):
            assert not any(ch in entry for ch in "*?["), f"{name}: wildcard approval {entry}"


def test_native_entry_requires_exact_stem_and_extension_suffix():
    match = licenses.native_entry_matches
    stem = "aiohttp/_http_parser"
    assert match(f"{stem}.cpython-314-x86_64-linux-gnu.so", stem)
    assert match(f"{stem}.cpython-314t-darwin.so", stem)
    assert match(f"{stem}.cp314-win_amd64.pyd", stem)
    assert match("cryptography/hazmat/bindings/_rust.abi3.so", "cryptography/hazmat/bindings/_rust")
    assert match("cryptography/hazmat/bindings/_rust.pyd", "cryptography/hazmat/bindings/_rust")
    # Arbitrary files that merely share the stem are not approved.
    for bad in (
        f"{stem}.so",
        f"{stem}.evil",
        f"{stem}.py",
        f"{stem}.cpython-314-x86_64-linux-gnu.so.bak",
        f"{stem}_extra.cpython-314-x86_64-linux-gnu.so",
        "aiohttp/other.cpython-314-x86_64-linux-gnu.so",
    ):
        assert not match(bad, stem), bad
    # A wildcard entry never matches anything.
    assert not match(f"{stem}.cpython-314-x86_64-linux-gnu.so", "aiohttp/_http_parser.*")
    assert not match("anything.cpython-314-x86_64-linux-gnu.so", "*")


def test_check_distribution_rejects_glob_style_sibling_of_reviewed_native(tmp_path):
    so = "aiohttp/_http_parser.evil.so"
    _make_dist(tmp_path, "aiohttp", "3.14.5", "Apache-2.0 AND MIT", {so: b"\x7fELFpayload"})
    inv = licenses.load_reviewed_inventory()
    (found,) = md.distributions(path=[str(tmp_path)])
    findings = licenses.check_distribution(found, inv, site_root=tmp_path)
    assert f"unapproved native object '{so}' in package 'aiohttp'" in _reasons(findings)


def test_check_distribution_accepts_abi_tagged_reviewed_native(tmp_path):
    so = "aiohttp/_http_parser.cpython-314-x86_64-linux-gnu.so"
    _make_dist(tmp_path, "aiohttp", "3.14.5", "Apache-2.0 AND MIT", {so: b"\x7fELFpayload"})
    inv = licenses.load_reviewed_inventory()
    (found,) = md.distributions(path=[str(tmp_path)])
    findings = licenses.check_distribution(found, inv, site_root=tmp_path)
    assert not any("native" in f.reason for f in findings), _reasons(findings)


def _lock_with_lxml_edit(tmp_path, edit):
    text = licenses.UV_LOCK_PATH.read_text(encoding="utf-8")
    path = tmp_path / "uv.lock"
    path.write_text(edit(text), encoding="utf-8")
    return path


def test_lxml_libiconv_review_records_exact_source():
    entry = licenses.load_reviewed_inventory()["lxml"]["libiconv"]
    assert entry["version"] == "1.18"
    assert entry["modified"] is False
    assert entry["source"] == "https://ftp.gnu.org/gnu/libiconv/libiconv-1.18.tar.gz"
    assert entry["source_sha256"] == (
        "3b08f5f4f9b4eb82f151a7040bfd6fe6c6fb922efe4b1659c66ea933276965e8"
    )
    notice = (licenses.NOTICES_DIR / "libiconv-LICENSE.txt").read_text(encoding="utf-8")
    assert entry["source"] in notice
    assert entry["source_sha256"] in notice


def test_real_uv_lock_matches_lxml_locked_wheels():
    inv = licenses.load_reviewed_inventory()
    assert licenses.check_locked_wheels(inv) == []
    locked = licenses.load_locked_wheels()[("lxml", "6.1.3")]
    assert locked == inv["lxml"]["libiconv"]["locked_wheels"]


def test_changed_lxml_wheel_hash_rejected(tmp_path):
    wheels = licenses.load_reviewed_inventory()["lxml"]["libiconv"]["locked_wheels"]
    wheel_hash = next(iter(wheels.values()))
    lock = _lock_with_lxml_edit(tmp_path, lambda t: t.replace(wheel_hash, "0" * 64, 1))
    findings = licenses.check_locked_wheels(licenses.load_reviewed_inventory(), lock)
    assert any("SHA-256 differs from review" in f.reason for f in findings)
    assert any(f.distribution == "lxml" for f in findings)
    assert licenses.check_environment([], lock_path=lock)


def test_unreviewed_lxml_wheel_rejected(tmp_path):
    marker = "lxml-6.1.3-cp314-cp314-win_arm64.whl"
    extra = marker.replace("win_arm64", "win_riscv64")

    def add_wheel(text):
        line = next(item for item in text.splitlines() if marker in item)
        return text.replace(line, line + "\n" + line.replace(marker, extra), 1)

    lock = _lock_with_lxml_edit(tmp_path, add_wheel)
    findings = licenses.check_locked_wheels(licenses.load_reviewed_inventory(), lock)
    assert any(f"unreviewed locked wheel {extra}" in f.reason for f in findings)


def test_missing_locked_lxml_wheel_rejected(tmp_path):
    marker = "lxml-6.1.3-cp314-cp314-win_arm64.whl"
    lock = _lock_with_lxml_edit(
        tmp_path,
        lambda t: "\n".join(row for row in t.splitlines() if marker not in row) + "\n",
    )
    findings = licenses.check_locked_wheels(licenses.load_reviewed_inventory(), lock)
    assert any(f"reviewed wheel not locked in uv.lock: {marker}" in f.reason for f in findings)


def test_unreadable_lockfile_rejected(tmp_path):
    findings = licenses.check_locked_wheels(
        licenses.load_reviewed_inventory(), tmp_path / "absent.lock"
    )
    assert any("cannot read locked wheels" in f.reason for f in findings)


def test_extension_suffixes_accept_every_pack_target():
    cases = {
        "lxml/etree.cpython-314-x86_64-linux-gnu.so": "lxml/etree",
        "lxml/etree.cpython-314-aarch64-linux-gnu.so": "lxml/etree",
        "lxml/etree.cpython-314-darwin.so": "lxml/etree",
        "lxml/etree.cp314-win_amd64.pyd": "lxml/etree",
        "_cffi_backend.cpython-314-darwin.so": "_cffi_backend",
        "_cffi_backend.cp314-win_amd64.pyd": "_cffi_backend",
        "cryptography/hazmat/bindings/_rust.abi3.so": "cryptography/hazmat/bindings/_rust",
        "cryptography/hazmat/bindings/_rust.pyd": "cryptography/hazmat/bindings/_rust",
        "yaml/_yaml.cpython-314-x86_64-linux-gnu.so": "yaml/_yaml",
        "yaml/_yaml.cp314-win_amd64.pyd": "yaml/_yaml",
        "aiohttp/_http_parser.cpython-314-aarch64-linux-gnu.so": "aiohttp/_http_parser",
        "multidict/_multidict.cp314-win_amd64.pyd": "multidict/_multidict",
        "yarl/_quoting_c.cpython-314-darwin.so": "yarl/_quoting_c",
        "propcache/_helpers_c.cpython-314-darwin.so": "propcache/_helpers_c",
        "frozenlist/_frozenlist.cp314-win_amd64.pyd": "frozenlist/_frozenlist",
    }
    for relative, entry in cases.items():
        assert licenses._native_name(relative.rsplit("/", 1)[-1]), relative
        assert licenses.native_entry_matches(relative, entry), relative
