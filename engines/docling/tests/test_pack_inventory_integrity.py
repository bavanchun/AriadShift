"""Locked-wheel packaging mistakes must fail before native notices are emitted."""

import base64
import csv
import hashlib
import sys
import zipfile
from importlib import metadata

import pytest
from conftest import REPOSITORY, native_record_rows

sys.path.insert(0, str(REPOSITORY / "engines/docling/pack"))
import build
import licenses

TRIPLE = "x86_64-unknown-linux-gnu"
EXTENSION = "demo/extension.cpython-314-x86_64-linux-gnu.so"


def installed(site, relative=EXTENSION, notice="MIT License\nCopyright test contributors\n"):
    native = site / relative
    native.parent.mkdir(parents=True, exist_ok=True)
    native.write_bytes(b"\x7fELF" + b"native extension bytes")
    info = site / "demo-1.0.dist-info"
    info.mkdir()
    (info / "METADATA").write_text(
        "Name: demo\nVersion: 1.0\nLicense-Expression: MIT\n", encoding="utf-8"
    )
    rows = []
    for path in [native]:
        digest = base64.urlsafe_b64encode(hashlib.sha256(path.read_bytes()).digest())
        rows.append([relative, "sha256=" + digest.decode().rstrip("="), str(path.stat().st_size)])
    if notice is not None:
        (info / "LICENSE").write_text(notice, encoding="utf-8")
        rows.append(["demo-1.0.dist-info/LICENSE", "", ""])
    with (info / "RECORD").open("w", newline="", encoding="utf-8") as handle:
        csv.writer(handle).writerows(rows)
    return metadata.PathDistribution(info)


@pytest.mark.parametrize(
    "column,value", [(1, "sha256=wrong"), (1, ""), (1, "sha512=wrong"), (2, "1"), (2, "")]
)
def test_native_record_hash_and_size_are_required_and_verified(tmp_path, column, value):
    installed(tmp_path)
    record = tmp_path / "demo-1.0.dist-info/RECORD"
    rows = list(csv.reader(record.read_text(encoding="utf-8").splitlines()))
    rows[0][column] = value
    with record.open("w", newline="", encoding="utf-8") as handle:
        csv.writer(handle).writerows(rows)
    with pytest.raises(SystemExit, match=EXTENSION):
        licenses.check_vendored_native_files(tmp_path, [], TRIPLE)


@pytest.mark.parametrize("relative", ["demo/libmystery.so", "demo/extension.so", "demo/native"])
def test_a_record_owned_nonmodule_needs_a_named_inventory_entry(tmp_path, relative):
    installed(tmp_path, relative)
    with pytest.raises(SystemExit, match=relative):
        licenses.check_vendored_native_files(tmp_path, [], TRIPLE)


def test_vendor_family_is_case_insensitive_inside_data_platlib(tmp_path):
    relative = "demo-1.0.data/platlib/LibQuadMath.SO"
    installed(tmp_path, relative)
    assert licenses._native_family("numpy", "LibQuadMath.SO") == "libquadmath"
    with pytest.raises(SystemExit, match=relative):
        licenses.check_vendored_native_files(tmp_path, [], TRIPLE)


def test_native_hard_links_are_refused(tmp_path):
    installed(tmp_path)
    (tmp_path / "second-copy").hardlink_to(tmp_path / EXTENSION)
    with pytest.raises(SystemExit, match="hard link"):
        licenses.check_vendored_native_files(tmp_path, [], TRIPLE)


@pytest.mark.parametrize("notice", [None, "", " \n\t"])
def test_native_owner_requires_a_nonempty_notice_for_check_and_copy(tmp_path, notice):
    owner = installed(tmp_path, notice=notice)
    with pytest.raises(SystemExit, match="notice"):
        licenses.check_vendored_native_files(tmp_path, [], TRIPLE)
    with pytest.raises(SystemExit, match="notice"):
        licenses.write_notices([owner], tmp_path / "notices")


def test_native_module_with_record_and_notice_is_allowed(tmp_path):
    owner = installed(tmp_path)
    licenses.check_vendored_native_files(tmp_path, [], TRIPLE)
    out = tmp_path / "notices"
    licenses.write_notices([owner], out)
    assert (out / "python/demo-1.0/LICENSE").read_text(encoding="utf-8").startswith("MIT License")


@pytest.mark.parametrize("escape", [False, True])
def test_directory_symlinks_are_refused(tmp_path, escape):
    site = tmp_path / "site"
    site.mkdir()
    destination = tmp_path / "outside" if escape else site / "inside"
    destination.mkdir()
    (site / "linked-directory").symlink_to(destination, target_is_directory=True)
    with pytest.raises(SystemExit, match="linked-directory"):
        licenses.check_vendored_native_files(site, [], TRIPLE)


@pytest.mark.parametrize("escape", [False, True])
def test_native_file_symlinks_are_refused(tmp_path, escape):
    site = tmp_path / "site"
    site.mkdir()
    installed(site)
    native = site / EXTENSION
    destination = tmp_path / "outside" if escape else site / "original"
    native.rename(destination)
    native.symlink_to(destination)
    with pytest.raises(SystemExit, match="symlink"):
        licenses.check_vendored_native_files(site, [], TRIPLE)


def test_non_native_symlink_escaping_site_is_refused(tmp_path):
    site = tmp_path / "site"
    site.mkdir()
    (site / "readme.txt").symlink_to(tmp_path / "missing")
    with pytest.raises(SystemExit, match=r"readme\.txt"):
        licenses.check_vendored_native_files(site, [], TRIPLE)


@pytest.mark.parametrize("suffix", ["zip", "egg", "whl"])
def test_archived_payloads_are_refused(tmp_path, suffix):
    path = tmp_path / ("hidden." + suffix)
    path.write_bytes(b"PK\x03\x04")
    with pytest.raises(SystemExit, match=path.name):
        licenses.check_vendored_native_files(tmp_path, [], TRIPLE)


@pytest.mark.parametrize(
    "filename", ["payload", "payload.bin", "payload.data", "payload.jar", "payload.pyz"]
)
def test_zip_magic_refuses_renamed_archives(tmp_path, filename):
    path = tmp_path / filename
    with zipfile.ZipFile(path, "w") as archive:
        archive.writestr("hidden.so", b"\x7fELF" + b"native payload")
    with pytest.raises(SystemExit, match=filename):
        licenses.check_vendored_native_files(tmp_path, [], TRIPLE)


@pytest.mark.parametrize("suffix", ["npz", "docx", "PPTX", "xlsx"])
def test_known_zip_data_formats_remain_allowed(tmp_path, suffix):
    with zipfile.ZipFile(tmp_path / ("data." + suffix), "w") as archive:
        archive.writestr("data.txt", "Document or array data")
    assert licenses._native_files(tmp_path) == set()


@pytest.fixture
def reviewed_data_archive(tmp_path, monkeypatch):
    relative = "torch/bin/model.ptl"
    path = tmp_path / relative
    path.parent.mkdir(parents=True)
    with zipfile.ZipFile(path, "w") as archive:
        archive.writestr("archive/data.pkl", b"Reviewed model data")
    info = tmp_path / "torch-2.14.1.dist-info"
    info.mkdir()
    (info / "METADATA").write_text(
        "Name: torch\nVersion: 2.14.1\nLicense-Expression: BSD-3-Clause\n", encoding="utf-8"
    )
    (info / "RECORD").write_text(native_record_rows(tmp_path, [relative]), encoding="utf-8")
    monkeypatch.setattr(
        licenses,
        "_REVIEWED_DATA_ARCHIVES",
        [
            {
                "distribution": "torch",
                "version": "2.14.1",
                "path": relative,
                "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
            }
        ],
        raising=False,
    )
    return path, info


def test_reviewed_model_archive_requires_exact_bytes_and_record(tmp_path, reviewed_data_archive):
    path, info = reviewed_data_archive
    assert licenses._native_files(tmp_path) == set()
    path.write_bytes(path.read_bytes() + b"changed model data")
    (info / "RECORD").write_text(
        native_record_rows(tmp_path, [path.relative_to(tmp_path)]), encoding="utf-8"
    )
    with pytest.raises(SystemExit, match=r"uninspected.*model\.ptl"):
        licenses._native_files(tmp_path)


@pytest.mark.parametrize("change", ["owner", "version", "record", "duplicate-owner"])
def test_reviewed_model_archive_requires_its_unique_record_owner(
    tmp_path, reviewed_data_archive, change
):
    _, info = reviewed_data_archive
    if change in {"owner", "version"}:
        text = (info / "METADATA").read_text(encoding="utf-8")
        text = (
            text.replace("Name: torch", "Name: other")
            if change == "owner"
            else text.replace("Version: 2.14.1", "Version: 2.14.2")
        )
        (info / "METADATA").write_text(text, encoding="utf-8")
    elif change == "record":
        (info / "RECORD").write_text("torch/bin/model.ptl,sha256=wrong,1\n", encoding="utf-8")
    else:
        other = tmp_path / "other-1.dist-info"
        other.mkdir()
        (other / "METADATA").write_text("Name: other\nVersion: 1\n", encoding="utf-8")
        (other / "RECORD").write_bytes((info / "RECORD").read_bytes())
    with pytest.raises(SystemExit, match=r"uninspected.*model\.ptl"):
        licenses._native_files(tmp_path)


@pytest.mark.parametrize("relative", ["torch/bin/unknown.ptl", "other/model.ptl"])
def test_unreviewed_model_archive_paths_are_refused(tmp_path, reviewed_data_archive, relative):
    path, info = reviewed_data_archive
    destination = tmp_path / relative
    destination.parent.mkdir(exist_ok=True)
    path.rename(destination)
    (info / "RECORD").write_text(native_record_rows(tmp_path, [relative]), encoding="utf-8")
    with pytest.raises(SystemExit, match="uninspected"):
        licenses._native_files(tmp_path)


def test_pth_reference_to_an_external_archive_is_refused(tmp_path):
    (tmp_path / "extra.pth").write_text("# wheel path\n../outside/hidden.WHL\n", encoding="utf-8")
    with pytest.raises(SystemExit, match=r"extra\.pth"):
        licenses.check_vendored_native_files(tmp_path, [], TRIPLE)


@pytest.mark.parametrize("field", ["License", "License-Expression", "License-File"])
def test_present_empty_licence_headers_fail_both_metadata_apis(tmp_path, field):
    owner = installed(tmp_path)
    declaration = "License: MIT\n" if field == "License-Expression" else "License-Expression: MIT\n"
    (tmp_path / "demo-1.0.dist-info/METADATA").write_text(
        "Name: demo\nVersion: 1.0\n" + declaration + field + ": \n", encoding="utf-8"
    )
    assert licenses.check([owner])
    with pytest.raises(SystemExit, match="licence gate failed"):
        licenses.pack_expression([owner], [])


def test_macho_bundle_extension_with_a_plain_so_name_is_allowed(tmp_path):
    relative = "demo/bundle.so"
    installed(tmp_path, relative)
    native = tmp_path / relative
    native.write_bytes(b"\xcf\xfa\xed\xfe" + bytes(8) + (8).to_bytes(4, "little"))
    (tmp_path / "demo-1.0.dist-info/RECORD").write_text(
        native_record_rows(tmp_path, [relative, "demo-1.0.dist-info/LICENSE"]), encoding="utf-8"
    )
    licenses.check_vendored_native_files(tmp_path, [], TRIPLE)


def test_reviewed_external_notices_are_checked_and_bundled(tmp_path, monkeypatch):
    original = licenses._NATIVE_COMPONENTS["onnxruntime-native"]
    component = {**original, "notices": [dict(item) for item in original["notices"]]}
    for item in component["notices"]:
        text = "MIT License\nCopyright test contributors\n" + item["path"]
        path = tmp_path / item["path"]
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
        item["sha256"] = hashlib.sha256(path.read_bytes()).hexdigest()
    monkeypatch.setitem(licenses._NATIVE_COMPONENTS, "onnxruntime-native", component)
    relative = component["files"][0]
    native = tmp_path / relative
    native.parent.mkdir(parents=True)
    native.write_bytes(b"\x7fELF" + b"named distribution library")
    info = tmp_path / f"onnxruntime-{component['version']}.dist-info"
    info.mkdir()
    (info / "METADATA").write_text(
        f"Name: onnxruntime\nVersion: {component['version']}\nLicense-Expression: MIT\n",
        encoding="utf-8",
    )
    extension = "onnxruntime/capi/probe.cpython-314-x86_64-linux-gnu.so"
    (tmp_path / extension).write_bytes(b"\x7fELF" + b"distribution extension")
    (info / "RECORD").write_text(
        native_record_rows(tmp_path, [relative, extension]), encoding="utf-8"
    )
    inventory = licenses.vendored_libraries(tmp_path, TRIPLE)
    licenses.check_vendored_native_files(tmp_path, inventory, TRIPLE)
    notices = tmp_path / "notices"
    licenses.write_notices([metadata.PathDistribution(info)], notices, inventory)
    licenses.write_vendored_notices(tmp_path, notices, inventory)
    assert len(list((notices / "vendored").iterdir())) == 2
    (tmp_path / component["notices"][1]["path"]).write_text("", encoding="utf-8")
    with pytest.raises(SystemExit, match="native notice"):
        licenses.check_vendored_native_files(tmp_path, inventory, TRIPLE)


@pytest.mark.parametrize("triple", [TRIPLE, "aarch64-apple-darwin", "x86_64-pc-windows-msvc"])
def test_runtime_pruning_drops_only_foreign_launcher_resources(tmp_path, triple):
    runtime = tmp_path / "runtime"
    site = build.site_packages(runtime, triple)
    relatives = ["pip/_vendor/distlib/t32.exe", "setuptools/cli.exe"]
    for relative in relatives:
        path = site / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(b"MZ launcher resource")
    for owner in ("pip", "setuptools"):
        info = site / f"{owner}-1.0.dist-info"
        info.mkdir()
        (info / "RECORD").write_text(
            native_record_rows(site, [p for p in relatives if p.startswith(owner)]),
            encoding="utf-8",
        )
    text = site / "pip/keep.py"
    text.write_text("# Keep the Python package itself.\n", encoding="utf-8")
    build.prune_runtime(runtime, triple)
    for relative in relatives:
        assert (site / relative).exists() is build.is_windows(triple)
        owner = relative.split("/")[0]
        assert (
            relative in (site / f"{owner}-1.0.dist-info/RECORD").read_text(encoding="utf-8")
        ) is build.is_windows(triple)
    assert text.is_file()


def test_reviewed_upstream_notice_supplies_a_missing_wheel_licence(tmp_path):
    component = licenses._NATIVE_COMPONENTS["tokenizers-native"]
    relative = component["files"][0]
    native = tmp_path / relative
    native.parent.mkdir()
    native.write_bytes(b"\x7fELF" + b"synthetic Python extension")
    info = tmp_path / f"tokenizers-{component['version']}.dist-info"
    info.mkdir()
    (info / "METADATA").write_text(
        f"Name: tokenizers\nVersion: {component['version']}\nLicense-Expression: Apache-2.0\n",
        encoding="utf-8",
    )
    (info / "RECORD").write_text(native_record_rows(tmp_path, [relative]), encoding="utf-8")
    inventory = licenses.vendored_libraries(tmp_path, TRIPLE)
    licenses.check_vendored_native_files(tmp_path, inventory, TRIPLE)
    notices = tmp_path / "notices"
    licenses.write_notices([metadata.PathDistribution(info)], notices, inventory)
    licenses.write_vendored_notices(tmp_path, notices, inventory)
    assert inventory[0]["notice_scope"] == "upstream"
    text = next((notices / "vendored").iterdir()).read_text(encoding="utf-8")
    assert "Apache License" in text and "Version 2.0, January 2004" in text
