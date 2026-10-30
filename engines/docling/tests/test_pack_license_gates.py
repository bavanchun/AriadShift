"""Conflicting metadata and unreviewed native wheel payloads stop packaging."""

from __future__ import annotations

import shutil
import sys
from importlib import metadata

import pytest
from conftest import REPOSITORY, native_record_rows, reviewed_native_file

sys.path.insert(0, str(REPOSITORY / "engines/docling/pack"))

import licenses


def _record_owner(site, owner, paths, version="1.0"):
    info = site / f"{owner}-{version}.dist-info"
    info.mkdir(exist_ok=True)
    (info / "METADATA").write_text(
        f"Name: {owner}\nVersion: {version}\nLicense-Expression: MIT\n", encoding="utf-8"
    )
    (info / "LICENSE").write_text("MIT License\n", encoding="utf-8")
    (info / "RECORD").write_text(
        native_record_rows(site, [*paths, f"{info.name}/LICENSE"]), encoding="utf-8"
    )


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
    lines = ["Name: demo", "Version: 1.0"]
    if field:
        lines.append(f"License: {field}")
    lines += [f"Classifier: License :: {item}" for item in classifiers]
    if expression:
        lines.append(f"License-Expression: {expression}")
    (info / "METADATA").write_text("\n".join(lines) + "\n", encoding="utf-8")
    assert (licenses.check([metadata.PathDistribution(info)]) == []) is allowed


def test_vendored_native_inventory_rejects_unknown_files_and_stale_entries(tmp_path):
    native = tmp_path / "pillow.libs/libXau-abc.so.6"
    native.parent.mkdir()
    native.write_bytes(b"\x7fELF")
    _record_owner(tmp_path, "pillow", ["pillow.libs/libXau-abc.so.6"])
    inventory = [
        {
            "name": "libXau",
            "distribution": "pillow",
            "license": "MIT",
            "notice": "COPYING",
            "native_files": {"linux": ["pillow.libs/libXau-*.so.6"]},
        }
    ]
    notice = tmp_path / inventory[0]["notice"]
    notice.parent.mkdir(parents=True, exist_ok=True)
    notice.write_text(
        "GNU GENERAL PUBLIC LICENSE\nGCC RUNTIME LIBRARY EXCEPTION\n", encoding="utf-8"
    )
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
        notice.write_text("Upstream licence text.\n", encoding="utf-8")
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


@pytest.mark.parametrize("name", ["scipy", "python-dateutil", "pypdfium2", "pywin32"])
def test_reviewed_distribution_names_cannot_hide_changed_gpl_metadata(tmp_path, name):
    info = tmp_path / f"{name}-1.0.dist-info"
    info.mkdir()
    (info / "METADATA").write_text(
        f"Name: {name}\nVersion: 1.0\nLicense: GPL-3.0-only\nLicense-Expression: MIT\n",
        encoding="utf-8",
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
        reference.read_text("METADATA").replace("Version: 2.5.4\n", f"Version: {version}\n"),
        encoding="utf-8",
    )
    record = reference.read_text("RECORD")
    if include_code:
        (tmp_path / "numpy/random/renamed-unreviewed-code.c").write_text(
            "/* Unreviewed code. */\n", encoding="utf-8"
        )
    (info / "RECORD").write_text(record, encoding="utf-8")
    assert (licenses.check([metadata.PathDistribution(info)]) == []) is allowed


@pytest.mark.parametrize("interpreter", ["python", "python3", "python3.14"])
def test_runtime_magic_uses_python_notice_and_rejects_unknown_executables(tmp_path, interpreter):
    binary = tmp_path / f"bin/{interpreter}"
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
    (site / "COPYING").write_text(text, encoding="utf-8")
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
    assert len(copied) == 6
    assert text in [p.read_text(encoding="utf-8") for p in copied]
    summary = (notices / "vendored-libraries.txt").read_text(encoding="utf-8")
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
    (info / "METADATA").write_text("Name: pillow\nVersion: 13.0.0\n", encoding="utf-8")
    (info / "licenses/LICENSE").write_text("Unrelated MIT licence text.\n", encoding="utf-8")
    (info / "RECORD").write_text(
        native_record_rows(tmp_path, ["pillow.libs/libavif-abc.so.16"])
        + "pillow-13.0.0.dist-info/licenses/LICENSE,,\n",
        encoding="utf-8",
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
    (tmp_path / "COPYING").write_text("FreeType licence text fixture.\n", encoding="utf-8")
    _record_owner(tmp_path, owner, ["pillow.libs/libfreetype-abc.so.6"])
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
    (info / "METADATA").write_text(
        "Name: pillow\nVersion: 13.0.0\nLicense-Expression: FTL\n", encoding="utf-8"
    )
    assert licenses.check([metadata.PathDistribution(info)])


@pytest.mark.parametrize("owner", ["shapely", "rtree"])
def test_shared_library_exception_requires_unchanged_objects_and_exact_source(tmp_path, owner):
    reference = metadata.distribution(owner)
    relative = reviewed_native_file(reference, licenses._REVIEWED_SHARED_FILES)
    native = tmp_path / relative
    native.parent.mkdir(parents=True)
    shutil.copyfile(reference.locate_file(relative), native)
    info = tmp_path / f"{owner}-{reference.version}.dist-info"
    info.mkdir()
    (info / "METADATA").write_text(reference.read_text("METADATA"), encoding="utf-8")
    (info / "RECORD").write_text(native_record_rows(tmp_path, [relative]), encoding="utf-8")
    if owner == "shapely":
        notice = "licenses/LICENSE_GEOS"
        (info / "licenses").mkdir()
        shutil.copyfile(reference.locate_file(f"{info.name}/{notice}"), info / notice)
    inventory = licenses.vendored_libraries(tmp_path, "x86_64-unknown-linux-gnu")
    licenses.check_vendored_native_files(tmp_path, inventory, "x86_64-unknown-linux-gnu")
    notices = tmp_path / "notices"
    licenses.write_vendored_notices(tmp_path, notices, inventory)
    summary = (notices / "vendored-libraries.txt").read_text(encoding="utf-8")
    version, source = licenses._REVIEWED_SHARED_SOURCES[owner]
    assert version in summary and source in summary
    if owner == "rtree":
        full_text = next((notices / "vendored").iterdir()).read_text(encoding="utf-8")
        assert "Marios Hadjieleftheriou" in full_text and "Howard Butler" in full_text
    item = inventory[0]
    item.pop("source")
    with pytest.raises(SystemExit, match="missing exact upstream version/source"):
        licenses.check_vendored_native_files(tmp_path, inventory, "x86_64-unknown-linux-gnu")
    item["source"] = source
    native.write_bytes(native.read_bytes() + b"modified")
    (info / "RECORD").write_text(native_record_rows(tmp_path, [relative]), encoding="utf-8")
    with pytest.raises(SystemExit, match="requires an unmodified reviewed shared object"):
        licenses.check_vendored_native_files(tmp_path, inventory, "x86_64-unknown-linux-gnu")
    native.write_bytes(b"!<arch>\nstatic library")
    (info / "RECORD").write_text(native_record_rows(tmp_path, [relative]), encoding="utf-8")
    with pytest.raises(SystemExit, match="requires an unmodified reviewed shared object"):
        licenses.check_vendored_native_files(tmp_path, inventory, "x86_64-unknown-linux-gnu")


@pytest.mark.parametrize("relative", licenses._REVIEWED_SHARED_FILES)
def test_reviewed_shared_files_have_a_named_library_and_owner(relative):
    owner = "shapely" if relative.startswith("shapely") else "rtree"
    family = licenses._native_family(owner, relative.rsplit("/", 1)[-1])
    assert family is not None and owner in licenses.NATIVE_LIBRARIES[family][1]


def test_quadmath_bundles_full_lgpl_and_retains_wheel_attribution(tmp_path):
    reference = metadata.distribution("numpy")
    relative = reviewed_native_file(
        reference, [path for path in licenses._GNU_SHARED_FILES if "libquadmath" in path]
    )
    native = tmp_path / relative
    native.parent.mkdir()
    shutil.copyfile(reference.locate_file(relative), native)
    _record_owner(tmp_path, "numpy", [relative], reference.version)
    attribution = "libquadmath: Copyright Free Software Foundation; LGPL-2.1-or-later.\n"
    (tmp_path / "COPYING").write_text(attribution, encoding="utf-8")
    inventory = [
        {
            "name": "libquadmath",
            "distribution": "numpy",
            "license": "LGPL-2.1-or-later",
            "notice": "COPYING",
            "native_files": {"linux": [relative]},
        }
    ]
    licenses.check_vendored_native_files(tmp_path, inventory, "x86_64-unknown-linux-gnu")
    notices = tmp_path / "notices"
    licenses.write_vendored_notices(tmp_path, notices, inventory)
    copied = [p.read_bytes() for p in (notices / "vendored").iterdir()]
    assert attribution.encode() in copied
    full_text = licenses._libquadmath_licence_text().read_bytes()
    assert full_text in copied
    assert b"GNU LESSER GENERAL PUBLIC LICENSE" in full_text
    summary = (notices / "vendored-libraries.txt").read_text(encoding="utf-8")
    assert summary.count("  Text:") == 6
    assert licenses._LGPL_21_SOURCE in summary
