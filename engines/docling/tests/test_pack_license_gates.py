"""Conflicting metadata and unreviewed native wheel payloads stop packaging."""

from __future__ import annotations

import shutil
import sys
from importlib import metadata

import pytest
from conftest import REPOSITORY

sys.path.insert(0, str(REPOSITORY / "engines/docling/pack"))

import licenses


@pytest.mark.parametrize(
    ("field", "classifiers", "expression", "allowed"),
    [
        ("GPL-3.0-or-later", ["MIT License"], "", False),
        ("", ["MIT License", "GNU Affero General Public License v3"], "", False),
        ("Unknown licence", ["MIT License"], "", False),
        ("", ["MIT License", "Unknown licence"], "", False),
        ("", ["MIT License", "Other/Proprietary License"], "", False),
        ("", ["OSI Approved", "MIT License"], "", True),
        ("GPL-3.0-only", [], "MIT", False),
        ("", ["GNU General Public License v3"], "MIT", False),
        ("CC0-1.0", [], "", False),
        ("", [], "CC0-1.0", False),
    ],
)
def test_conflicting_or_unknown_metadata_fails_closed(
    tmp_path, field, classifiers, expression, allowed
):
    info = tmp_path / "demo-1.0.dist-info"
    info.mkdir()
    lines = ["Name: demo", "Version: 1.0", f"License: {field}"]
    lines += [f"Classifier: License :: {item}" for item in classifiers]
    if expression:
        lines.append(f"License-Expression: {expression}")
    (info / "METADATA").write_text("\n".join(lines) + "\n")
    assert (licenses.check([metadata.PathDistribution(info)]) == []) is allowed


def test_vendored_native_inventory_rejects_unknown_files_and_stale_entries(tmp_path):
    native = tmp_path / "torch.libs/libgomp-abc.so.1"
    native.parent.mkdir()
    native.write_bytes(b"\x7fELF")
    inventory = [
        {
            "name": "libgomp",
            "distribution": "torch",
            "license": "GPL-3.0-or-later WITH GCC-exception-3.1",
            "notice": "torch.dist-info/licenses/COPYING.RUNTIME",
            "native_files": {"linux": ["torch.libs/libgomp-*.so.1"]},
        }
    ]
    notice = tmp_path / inventory[0]["notice"]
    notice.parent.mkdir(parents=True)
    notice.write_text("GNU GENERAL PUBLIC LICENSE\nGCC RUNTIME LIBRARY EXCEPTION\n")
    with pytest.raises(SystemExit, match="unlisted native"):
        licenses.check_vendored_native_files(tmp_path, [], "x86_64-unknown-linux-gnu")
    assert (
        licenses.check_vendored_native_files(tmp_path, inventory, "x86_64-unknown-linux-gnu")
        == inventory
    )
    native.unlink()
    with pytest.raises(SystemExit, match="match no native files"):
        licenses.check_vendored_native_files(tmp_path, inventory, "x86_64-unknown-linux-gnu")


def test_vendored_native_inventory_requires_notice_and_scans_magic(tmp_path):
    hidden = tmp_path / "numpy.libs/renamed-payload"
    hidden.parent.mkdir()
    hidden.write_bytes(b"\x7fELF" + b"unreviewed")
    with pytest.raises(SystemExit, match="unlisted native"):
        licenses.check_vendored_native_files(tmp_path, [], "x86_64-unknown-linux-gnu")


@pytest.mark.parametrize(
    ("license_expression", "notice_present"),
    [("GPL-3.0-or-later", True), ("AGPL-3.0-only", True), ("MIT", False)],
)
def test_native_inventory_cannot_approve_forbidden_code_or_missing_notices(
    tmp_path, license_expression, notice_present
):
    native = tmp_path / "demo.libs/libdemo.so"
    native.parent.mkdir()
    native.write_bytes(b"\x7fELF")
    notice = tmp_path / "COPYING"
    if notice_present:
        notice.write_text("Upstream licence text.\n")
    inventory = [
        {
            "name": "demo",
            "distribution": "demo",
            "license": license_expression,
            "notice": "COPYING",
            "native_files": {"linux": ["demo.libs/libdemo.so"]},
        }
    ]
    with pytest.raises(SystemExit):
        licenses.check_vendored_native_files(tmp_path, inventory, "x86_64-unknown-linux-gnu")


@pytest.mark.parametrize("magic", [b"\x7fELF", b"MZ", b"\xcf\xfa\xed\xfe", b"\xca\xfe\xba\xbe"])
def test_intel_scanner_checks_native_magic_without_extension(tmp_path, magic):
    (tmp_path / "payload").write_bytes(magic + b"Intel(R) OpenMP")
    with pytest.raises(SystemExit, match="proprietary Intel binary content"):
        licenses.check_vendor_files(tmp_path)


@pytest.mark.parametrize("name", ["scipy", "python-dateutil", "pypdfium2"])
def test_reviewed_distribution_names_cannot_hide_changed_gpl_metadata(tmp_path, name):
    info = tmp_path / f"{name}-1.0.dist-info"
    info.mkdir()
    (info / "METADATA").write_text(
        f"Name: {name}\nVersion: 1.0\nLicense: GPL-3.0-only\nLicense-Expression: MIT\n"
    )
    assert licenses.check([metadata.PathDistribution(info)])


@pytest.mark.parametrize(
    ("version", "include_code", "allowed"),
    [("2.5.4", False, True), ("2.5.4", True, False), ("2.5.5", False, False)],
)
def test_numpy_cc0_exception_requires_reviewed_metadata_only(
    tmp_path, version, include_code, allowed
):
    reference = metadata.distribution("numpy")
    shutil.copytree(
        reference.locate_file("numpy"),
        tmp_path / "numpy",
        ignore=shutil.ignore_patterns("__pycache__"),
    )
    info = tmp_path / f"numpy-{version}.dist-info"
    info.mkdir()
    (info / "METADATA").write_text(
        reference.read_text("METADATA").replace("Version: 2.5.4\n", f"Version: {version}\n")
    )
    record = reference.read_text("RECORD")
    if include_code:
        (tmp_path / "numpy/random/renamed-unreviewed-code.c").write_text("/* Unreviewed code. */\n")
    (info / "RECORD").write_text(record)
    assert (licenses.check([metadata.PathDistribution(info)]) == []) is allowed


def test_runtime_magic_uses_python_notice_and_rejects_unknown_executables(tmp_path):
    binary = tmp_path / "bin/python3.14"
    binary.parent.mkdir()
    binary.write_bytes(b"\x7fELF")
    inventory = [{"name": "cpython", "native_files": {"linux": ["lib/libpython*.so"]}}]
    licenses.check_runtime_files(tmp_path, inventory, "x86_64-unknown-linux-gnu")
    (tmp_path / "bin/unlisted").write_bytes(b"\x7fELF")
    with pytest.raises(SystemExit, match="unlisted native runtime file"):
        licenses.check_runtime_files(tmp_path, inventory, "x86_64-unknown-linux-gnu")


def test_vendored_notices_preserve_full_text_and_manifest_obligations(tmp_path):
    site = tmp_path / "site"
    site.mkdir()
    text = "GNU GENERAL PUBLIC LICENSE\nGCC RUNTIME LIBRARY EXCEPTION\n"
    (site / "COPYING").write_text(text)
    inventory = [
        {
            "name": "libgomp",
            "distribution": "torch",
            "license": "GPL-3.0-or-later WITH GCC-exception-3.1",
            "notice": "COPYING",
            "native_files": {"linux": ["torch/lib/libgomp.so.1"]},
        }
    ]
    notices = tmp_path / "notices"
    licenses.write_vendored_notices(site, notices, inventory)
    copied = list((notices / "vendored").iterdir())
    assert len(copied) == 1
    assert copied[0].read_text() == text
    summary = (notices / "vendored-libraries.txt").read_text()
    assert "torch/lib/libgomp.so.1" in summary
    assert "GCC-exception-3.1" in summary
    expression = licenses.pack_expression([], [item["license"] for item in inventory])
    assert expression == "(GPL-3.0-or-later WITH GCC-exception-3.1)"


def test_wheel_notice_content_changes_require_review(tmp_path):
    native = tmp_path / "pillow.libs/libavif-abc.so.16"
    native.parent.mkdir()
    native.write_bytes(b"\x7fELF")
    info = tmp_path / "pillow-13.0.0.dist-info"
    (info / "licenses").mkdir(parents=True)
    (info / "METADATA").write_text("Name: pillow\nVersion: 13.0.0\n")
    (info / "licenses/LICENSE").write_text("Unrelated MIT licence text.\n")
    (info / "RECORD").write_text(
        "pillow.libs/libavif-abc.so.16,,\npillow-13.0.0.dist-info/licenses/LICENSE,,\n"
    )
    with pytest.raises(SystemExit, match="native licence notice digest mismatch"):
        licenses.vendored_libraries(tmp_path, "x86_64-unknown-linux-gnu")


@pytest.mark.parametrize(
    ("expression", "owner", "allowed"),
    [("FTL", "pillow", True), ("GPL-2.0-or-later", "pillow", False), ("FTL", "torch", False)],
)
def test_freetype_exception_is_bound_to_pillow_and_the_permissive_alternative(
    tmp_path, expression, owner, allowed
):
    native = tmp_path / "pillow.libs/libfreetype-abc.so.6"
    native.parent.mkdir()
    native.write_bytes(b"\x7fELF")
    (tmp_path / "COPYING").write_text("FreeType licence text fixture.\n")
    inventory = [
        {
            "name": "libfreetype",
            "distribution": owner,
            "license": expression,
            "notice": "COPYING",
            "native_files": {"linux": ["pillow.libs/libfreetype-abc.so.6"]},
        }
    ]
    if allowed:
        assert (
            licenses.check_vendored_native_files(tmp_path, inventory, "x86_64-unknown-linux-gnu")
            == inventory
        )
    else:
        with pytest.raises(SystemExit, match="excluded or unreviewed native licence"):
            licenses.check_vendored_native_files(tmp_path, inventory, "x86_64-unknown-linux-gnu")


def test_native_freetype_exception_does_not_allow_python_distributions(tmp_path):
    info = tmp_path / "pillow-13.0.0.dist-info"
    info.mkdir()
    (info / "METADATA").write_text("Name: pillow\nVersion: 13.0.0\nLicense-Expression: FTL\n")
    assert licenses.check([metadata.PathDistribution(info)])
