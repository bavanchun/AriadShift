"""Every native payload and every declared licence claim must be accounted for."""

from __future__ import annotations

import shutil
import sys
from importlib import metadata
from pathlib import Path

import pytest
from conftest import REPOSITORY, native_record_rows

sys.path.insert(0, str(REPOSITORY / "engines/docling/pack"))
import licenses

TARGET = "x86_64-unknown-linux-gnu"


def distribution(site, owner="demo", fields="License-Expression: MIT\n", files=()):
    info = site / f"{owner}-1.0.dist-info"
    info.mkdir(parents=True, exist_ok=True)
    (info / "METADATA").write_text(f"Name: {owner}\nVersion: 1.0\n{fields}")
    (info / "LICENSE").write_text("MIT License\n")
    (info / "RECORD").write_text(native_record_rows(site, [*files, f"{info.name}/LICENSE"]))
    return metadata.PathDistribution(info)


@pytest.mark.parametrize(
    "fields",
    [
        "License-Expression: MIT OR LicenseRef-Unknown\n",
        "License-Expression: MIT AND (Apache-2.0 OR LicenseRef-Unknown)\n",
        "License: Apache-2.0\nLicense-Expression: MIT\n",
        "License: MIT\nClassifier: License :: OSI Approved :: Apache Software License\n",
        "License-Expression: MIT\nClassifier: License :: OSI Approved :: Apache Software License\n",
        "License: MIT\nLicense: GPL-3.0-only\n",
        "License: MIT\nLicense: MIT\n",
        "License-Expression: MIT\nLicense-Expression: Apache-2.0\n",
        "License-Expression: MIT\nClassifier: license :: GPL\n",
        "License-Expression: MIT\nClassifier: LICENSE :: Unknown licence\n",
    ],
)
def test_every_metadata_claim_must_be_known_consistent_and_single_valued(tmp_path, fields):
    item = distribution(tmp_path, fields=fields)
    assert licenses.check([item])
    with pytest.raises(SystemExit, match="licence gate failed"):
        licenses.pack_expression([item], [])


@pytest.mark.parametrize(
    "fields",
    [
        (
            "License: MIT\nLicense-Expression: MIT\n"
            "Classifier: license :: OSI Approved :: MIT License\n"
        ),
        "License-Expression: MIT OR Apache-2.0\n",
        "Classifier: LICENSE :: OSI Approved\nClassifier: license :: OSI Approved :: MIT License\n",
    ],
)
def test_consistent_metadata_and_known_alternatives_remain_allowed(tmp_path, fields):
    item = distribution(tmp_path, fields=fields)
    assert not licenses.check([item])
    assert licenses.pack_expression([item], [])


@pytest.mark.parametrize(
    ("relative", "payload"),
    [
        ("other/unknown.so", b"\x7fELF"),
        ("other/no-extension", b"\x7fELF"),
        ("other/extension.cpython-314-x86_64-linux-gnu.so", b"\x7fELF"),
        ("other/unknown.pyd", b"MZ"),
        ("other/unknown.dylib", b"\xcf\xfa\xed\xfe"),
        ("other/hidden-archive", b"!<arch>\n"),
    ],
)
def test_total_scan_refuses_native_files_without_record_ownership(tmp_path, relative, payload):
    path = tmp_path / relative
    path.parent.mkdir(parents=True)
    path.write_bytes(payload)
    distribution(tmp_path)
    with pytest.raises(SystemExit, match=relative):
        licenses.check_vendored_native_files(tmp_path, [], TARGET)


@pytest.mark.parametrize(
    "relative",
    [
        "demo/extension.cpython-314-x86_64-linux-gnu.so",
        "demo/extension.abi3.so",
        "demo/extension.pyd",
    ],
)
def test_record_owned_extensions_need_a_licensed_unique_owner(tmp_path, relative):
    native = tmp_path / relative
    native.parent.mkdir(parents=True)
    native.write_bytes(b"MZ" if relative.endswith(".pyd") else b"\x7fELF")
    owner = distribution(tmp_path, files=[relative])
    licenses.check_vendored_native_files(tmp_path, [], TARGET)
    (Path(owner.locate_file("demo-1.0.dist-info")) / "METADATA").write_text(
        "Name: demo\nVersion: 1.0\nLicense-Expression: GPL-3.0-only\n"
    )
    with pytest.raises(SystemExit, match=relative):
        licenses.check_vendored_native_files(tmp_path, [], TARGET)


def test_native_record_ownership_must_not_overlap(tmp_path):
    relative = "demo/extension.abi3.so"
    native = tmp_path / relative
    native.parent.mkdir()
    native.write_bytes(b"\x7fELF")
    distribution(tmp_path, files=[relative])
    distribution(tmp_path, owner="other", files=[relative])
    with pytest.raises(SystemExit, match=relative):
        licenses.check_vendored_native_files(tmp_path, [], TARGET)


def test_moving_a_listed_library_cannot_hide_it_as_a_python_module(tmp_path):
    reference = metadata.distribution("shapely")
    relative = next(str(p) for p in reference.files if str(p).startswith("shapely.libs/libgeos_c"))
    moved = "shapely/relocated/" + Path(relative).name
    native = tmp_path / moved
    native.parent.mkdir(parents=True)
    shutil.copyfile(reference.locate_file(relative), native)
    info = tmp_path / f"shapely-{reference.version}.dist-info"
    info.mkdir()
    (info / "METADATA").write_text(reference.read_text("METADATA"))
    (info / "RECORD").write_text(native_record_rows(tmp_path, [moved]))
    with pytest.raises(SystemExit, match=moved):
        licenses.check_vendored_native_files(tmp_path, [], TARGET)


@pytest.mark.parametrize("family", ["libgfortran", "libquadmath"])
@pytest.mark.parametrize("payload", [b"!<arch>\nstatic", b"\x7fELF" + b"modified"])
def test_gnu_runtime_entries_refuse_static_and_changed_payloads(tmp_path, family, payload):
    reference = metadata.distribution("numpy")
    native = tmp_path / f"numpy.libs/{family}-changed.so"
    native.parent.mkdir()
    native.write_bytes(payload)
    info = tmp_path / f"numpy-{reference.version}.dist-info"
    (info / "licenses").mkdir(parents=True)
    (info / "METADATA").write_text(
        f"Name: numpy\nVersion: {reference.version}\nLicense-Expression: MIT\n"
    )
    (info / "RECORD").write_text(native_record_rows(tmp_path, [native.relative_to(tmp_path)]))
    shutil.copyfile(
        reference.locate_file(f"{info.name}/licenses/LICENSE.txt"), info / "licenses/LICENSE.txt"
    )
    with pytest.raises(SystemExit):
        inventory = licenses.vendored_libraries(tmp_path, TARGET)
        licenses.check_vendored_native_files(tmp_path, inventory, TARGET)


@pytest.mark.parametrize("payload", [b"!<arch>\nstatic", b"\x7fELF" + b"modified"])
def test_gomp_entry_refuses_static_and_changed_payloads(tmp_path, payload):
    relative = "torch/lib/libgomp-unreviewed.so.1"
    native = tmp_path / relative
    native.parent.mkdir(parents=True)
    native.write_bytes(payload)
    distribution(tmp_path, owner="torch", files=[relative])
    (tmp_path / "COPYING").write_text("GNU GENERAL PUBLIC LICENSE\nGCC RUNTIME LIBRARY EXCEPTION\n")
    inventory = [
        {
            "name": "libgomp",
            "distribution": "torch",
            "license": "GPL-3.0-or-later WITH GCC-exception-3.1",
            "notice": "COPYING",
            "native_files": {"linux": [relative]},
        }
    ]
    with pytest.raises(SystemExit):
        licenses.check_vendored_native_files(tmp_path, inventory, TARGET)


@pytest.mark.parametrize("family", ["libgfortran", "libquadmath"])
def test_gnu_binding_checks_changes_at_the_reviewed_path(tmp_path, family):
    reference = metadata.distribution("numpy")
    relative = next(
        str(p) for p in reference.files if str(p) in licenses._GNU_SHARED_FILES and family in str(p)
    )
    native = tmp_path / relative
    native.parent.mkdir()
    shutil.copyfile(reference.locate_file(relative), native)
    info = tmp_path / f"numpy-{reference.version}.dist-info"
    (info / "licenses").mkdir(parents=True)
    (info / "METADATA").write_text(
        f"Name: numpy\nVersion: {reference.version}\nLicense-Expression: MIT\n"
    )
    (info / "RECORD").write_text(native_record_rows(tmp_path, [relative]))
    shutil.copyfile(
        reference.locate_file(f"{info.name}/licenses/LICENSE.txt"), info / "licenses/LICENSE.txt"
    )
    inventory = licenses.vendored_libraries(tmp_path, TARGET)
    licenses.check_vendored_native_files(tmp_path, inventory, TARGET)
    native.write_bytes(native.read_bytes() + b"modified")
    (info / "RECORD").write_text(native_record_rows(tmp_path, [relative]))
    with pytest.raises(SystemExit, match="unmodified reviewed ELF shared object"):
        licenses.check_vendored_native_files(tmp_path, inventory, TARGET)
    native.write_bytes(b"!<arch>\nstatic library")
    (info / "RECORD").write_text(native_record_rows(tmp_path, [relative]))
    with pytest.raises(SystemExit, match="unmodified reviewed ELF shared object"):
        licenses.check_vendored_native_files(tmp_path, inventory, TARGET)


def test_a_record_entry_does_not_turn_a_vendor_library_into_a_python_module(tmp_path):
    relative = "demo.libs/libunknown.so"
    native = tmp_path / relative
    native.parent.mkdir()
    native.write_bytes(b"\x7fELF")
    distribution(tmp_path, files=[relative])
    with pytest.raises(SystemExit, match=relative):
        licenses.check_vendored_native_files(tmp_path, [], TARGET)


def test_total_scan_detects_thin_archives_without_an_extension(tmp_path):
    relative = "demo/hidden-static-payload"
    native = tmp_path / relative
    native.parent.mkdir()
    native.write_bytes(b"!<thin>\n")
    distribution(tmp_path)
    with pytest.raises(SystemExit, match=relative):
        licenses.check_vendored_native_files(tmp_path, [], TARGET)
