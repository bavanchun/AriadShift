"""Toolchain export requires positive native licence identities, even after renames."""

import hashlib
import sys
import zipfile
from pathlib import PureWindowsPath

import pytest
from conftest import REPOSITORY

sys.path.insert(0, str(REPOSITORY / "engines/docling/pack"))
import licenses

PAYLOAD = b"\xcf\xfa\xed\xfe" + bytes(16) + b"reviewed synthetic image codec"
NOTICE = (REPOSITORY / "LICENSE").read_bytes()


def export_wheel(tmp_path, payload=PAYLOAD, filename="libcodec.dylib", notice=NOTICE):
    wheel = tmp_path / "codec-1.0-py3-none-any.whl"
    with zipfile.ZipFile(wheel, "w") as archive:
        archive.writestr("codec/" + filename, payload)
        if notice is not None:
            archive.writestr("codec-1.0.dist-info/licenses/LICENSE", notice)
    return wheel


def review(monkeypatch):
    component = {
        "name": "codec-native",
        "distribution": "codec",
        "version": "1.0",
        "license": "Apache-2.0",
        "files": ["codec/libcodec.dylib"],
        "notices": [
            {
                "path": "codec-1.0.dist-info/licenses/LICENSE",
                "sha256": hashlib.sha256(NOTICE).hexdigest(),
            }
        ],
        "export_objects": {"codec/libcodec.dylib": hashlib.sha256(PAYLOAD).hexdigest()},
    }
    monkeypatch.setattr(licenses, "_NATIVE_COMPONENTS", {"codec-native": component})
    return component


@pytest.mark.parametrize(
    "payload",
    [
        b"\x7fELF",
        b"MZ",
        b"\xcf\xfa\xed\xfe",
        b"!<arch>\n",
        b"!<thin>\n",
        b"BC\xc0\xde",
        b"\xde\xc0\x17\x0b",
        b"\x64\x86\x01\x00",
        b"\x64\x86\xff\x00",
        b"\0\0\xff\xff",
    ],
)
@pytest.mark.parametrize("filename", ["libcodec.dylib", "libavif.1.dylib", "payload.data"])
def test_unidentified_native_content_is_refused_regardless_of_filename(tmp_path, payload, filename):
    wheel = export_wheel(tmp_path, payload + bytes(64), filename)
    with pytest.raises(SystemExit, match="unreviewed native export"):
        licenses.check_wheel_files([wheel])


@pytest.mark.parametrize("filename", ["libcodec.dylib", "libavif.1.dylib", "payload.data"])
def test_positive_component_and_notice_review_allows_only_identified_bytes(
    tmp_path, monkeypatch, filename
):
    review(monkeypatch)
    licenses.check_wheel_files([export_wheel(tmp_path, filename=filename)])


@pytest.mark.parametrize("change", ["bytes", "receipt", "missing", "duplicate", "gpl", "unknown"])
def test_native_export_identity_and_licence_cannot_be_inferred_from_an_output_hash(
    tmp_path, monkeypatch, change
):
    component = review(monkeypatch)
    payload = PAYLOAD
    if change == "bytes":
        payload += b"changed"
    elif change == "receipt":
        component["export_objects"]["codec/libcodec.dylib"] = "a" * 64
    elif change == "missing":
        component.pop("export_objects")
    elif change == "duplicate":
        monkeypatch.setitem(licenses._NATIVE_COMPONENTS, "other", dict(component, name="other"))
    else:
        component["license"] = "GPL-3.0-only" if change == "gpl" else "LicenseRef-Unknown"
    with pytest.raises(SystemExit, match="native export"):
        licenses.check_wheel_files([export_wheel(tmp_path, payload)])


@pytest.mark.parametrize("notice", [None, b"", b"changed notice"])
def test_reviewed_native_bytes_require_the_bundled_reviewed_notice(tmp_path, monkeypatch, notice):
    review(monkeypatch)
    with pytest.raises(SystemExit, match=r"native export.*notice"):
        licenses.check_wheel_files([export_wheel(tmp_path, notice=notice)])


def test_sidecar_native_bytes_also_need_a_positive_review(tmp_path):
    artifact = tmp_path / "payload.data"
    artifact.write_bytes(PAYLOAD)
    with pytest.raises(SystemExit, match="unreviewed native export"):
        licenses.check_wheel_files([artifact])


@pytest.mark.parametrize("name", ["libclean.1.dylib", "libavif.16.dylib"])
def test_exported_clean_image_libraries_remain_allowed(tmp_path, monkeypatch, name):
    review(monkeypatch)
    licenses.check_wheel_files([export_wheel(tmp_path, filename=name)])


def test_openmp_output_checksum_does_not_create_a_licence_review(tmp_path):
    wheel = tmp_path / "wrapper-1.0-py3-none-any.whl"
    with zipfile.ZipFile(wheel, "w") as archive:
        archive.writestr("wrapper.py", "# No native code here\n")
    sidecar = tmp_path / "libiomp5md.dll"
    sidecar.write_bytes(b"MZ" + bytes(64))
    with pytest.raises(SystemExit, match="unreviewed native export"):
        licenses.check_wheel_files([wheel], openmp_dll=sidecar)


def test_identified_loose_native_artifact_requires_its_reviewed_notice(tmp_path, monkeypatch):
    review(monkeypatch)
    artifact = tmp_path / "payload.data"
    artifact.write_bytes(PAYLOAD)
    with pytest.raises(SystemExit, match=r"native export.*notice"):
        licenses.check_wheel_files([artifact])
    notice = tmp_path / "codec-1.0.dist-info/licenses/LICENSE"
    notice.parent.mkdir(parents=True)
    notice.write_bytes(NOTICE)
    licenses.check_wheel_files([artifact])


def test_component_receipt_must_reference_its_reviewed_files(tmp_path, monkeypatch):
    component = review(monkeypatch)
    component["files"] = ["some/other/file"]
    with pytest.raises(SystemExit, match=r"native export.*review"):
        licenses.check_wheel_files([export_wheel(tmp_path)])


@pytest.mark.parametrize(
    "path",
    [
        "../LICENSE",
        "/outside/LICENSE",
        "notices\\LICENSE",
        "C:/outside/LICENSE",
        "C:LICENSE",
        "//server/share/LICENSE",
        "\\outside\\LICENSE",
        "notices/../LICENSE",
    ],
)
def test_native_export_notice_paths_cannot_escape_the_artifact(tmp_path, monkeypatch, path):
    component = review(monkeypatch)
    component["notices"][0]["path"] = path
    with pytest.raises(SystemExit, match="native export notice path is unsafe"):
        licenses.check_wheel_files([export_wheel(tmp_path)])


@pytest.mark.parametrize("path", ["/outside/LICENSE", "C:LICENSE", "notices/../LICENSE"])
def test_native_export_notice_paths_are_safe_under_windows_semantics(tmp_path, monkeypatch, path):
    component = review(monkeypatch)
    component["notices"][0]["path"] = path
    wheel = export_wheel(tmp_path)
    with zipfile.ZipFile(wheel) as archive:
        monkeypatch.setattr(licenses, "Path", PureWindowsPath)
        errors = []
        licenses._export_native_review(
            hashlib.sha256(PAYLOAD).hexdigest(), wheel.name, archive, wheel, errors
        )
    assert errors == [f"native export notice path is unsafe: {wheel.name}"]


def test_upstream_notices_must_be_bundled_under_the_reviewed_notice_path(tmp_path, monkeypatch):
    component = review(monkeypatch)
    component["notice_scope"] = "upstream"
    component["notices"][0]["path"] = "codec-LICENSE.txt"
    wheel = export_wheel(tmp_path)
    with pytest.raises(SystemExit, match=r"native export.*notice"):
        licenses.check_wheel_files([wheel])
    with zipfile.ZipFile(wheel, "a") as archive:
        archive.writestr("notices/codec-LICENSE.txt", NOTICE)
    licenses.check_wheel_files([wheel])


def test_licensed_python_extension_does_not_supply_a_native_identity(tmp_path):
    wheel = export_wheel(tmp_path, filename="extension.cpython-314-x86_64-linux-gnu.so")
    with zipfile.ZipFile(wheel, "a") as archive:
        archive.writestr(
            "codec-1.0.dist-info/METADATA",
            "Name: codec\nVersion: 1.0\nLicense-Expression: Apache-2.0\n",
        )
    with pytest.raises(SystemExit, match="unreviewed native export"):
        licenses.check_wheel_files([wheel])


def test_every_native_member_requires_review_even_beside_an_identified_component(
    tmp_path, monkeypatch
):
    review(monkeypatch)
    wheel = export_wheel(tmp_path)
    with zipfile.ZipFile(wheel, "a") as archive:
        archive.writestr("codec/payload.data", b"\x7fELF" + bytes(64))
    with pytest.raises(SystemExit, match="unreviewed native export"):
        licenses.check_wheel_files([wheel])


@pytest.mark.parametrize("filename", ["libcodec.dylib/", "libx264.164.dylib/", "payload.data/"])
@pytest.mark.parametrize("payload", [b"not native, still a directory payload", PAYLOAD])
def test_directory_members_cannot_carry_payloads(tmp_path, monkeypatch, filename, payload):
    review(monkeypatch)
    with pytest.raises(SystemExit, match="directory payload"):
        licenses.check_wheel_files([export_wheel(tmp_path, payload, filename)])


@pytest.mark.parametrize(
    ("filename", "reason"),
    [("libcodec.dylib/", "unreviewed native export"), ("libx264.164.dylib/", "forbidden video")],
)
def test_native_directory_payloads_still_reach_content_and_video_checks(tmp_path, filename, reason):
    with pytest.raises(SystemExit, match=reason):
        licenses.check_wheel_files(
            [export_wheel(tmp_path, b"\xcf\xfa\xed\xfe" + bytes(64), filename)]
        )


def test_genuine_empty_directory_members_remain_allowed(tmp_path):
    wheel = tmp_path / "python-1.0-py3-none-any.whl"
    with zipfile.ZipFile(wheel, "w") as archive:
        archive.writestr("codec/", b"")
        archive.writestr("codec/assets/", b"")
        archive.writestr("codec/module.py", b"# No native payload\n")
    licenses.check_wheel_files([wheel])


def test_empty_directory_with_compressed_payload_bytes_is_refused(tmp_path):
    wheel = export_wheel(tmp_path, b"", "empty/")
    with zipfile.ZipFile(wheel, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        archive.writestr("codec/empty/", b"")
    with zipfile.ZipFile(wheel) as archive:
        assert archive.infolist()[0].compress_size > 0
    with pytest.raises(SystemExit, match="directory payload"):
        licenses.check_wheel_files([wheel])


@pytest.mark.parametrize(
    "name",
    [
        "../payload.data",
        "/codec/payload.data",
        "C:/payload.data",
        "codec\\payload.data",
        "codec/../payload.data",
        "codec/./payload.data",
        "codec//payload.data",
        "codec/payload.data\0hidden",
        "codec/payload.data\n",
        "codec/payload.data\x7f",
    ],
)
@pytest.mark.parametrize("separator", ["/", "\\"], ids=["posix-writer", "windows-writer"])
def test_ambiguous_export_member_paths_are_refused_before_licence_approval(
    tmp_path, monkeypatch, name, separator
):
    wheel = tmp_path / "paths-1.0-py3-none-any.whl"
    # ZipInfo strips NULs and normalizes backslashes on Windows. Write a safe,
    # same-length placeholder, then patch both the local and central names.
    stored_name = "x" * len(name.encode())
    with monkeypatch.context() as writer:
        writer.setattr(zipfile.os, "sep", separator)
        with zipfile.ZipFile(wheel, "w") as archive:
            archive.writestr(stored_name, b"plain data")
    raw = wheel.read_bytes()
    assert raw.count(stored_name.encode()) == 2
    wheel.write_bytes(raw.replace(stored_name.encode(), name.encode()))
    with zipfile.ZipFile(wheel) as archive:
        assert archive.infolist()[0].orig_filename == name
    with pytest.raises(SystemExit, match="unsafe native export archive path"):
        licenses.check_wheel_files([wheel])


@pytest.mark.parametrize(
    "names",
    [
        ("codec/file", "codec/file"),
        ("codec/", "codec/"),
        ("codec/File", "codec/file"),
        ("Codec/", "codec/"),
        ("codec", "codec/"),
        ("codec", "codec/payload.data"),
        ("Codec", "codec/payload.data"),
    ],
)
def test_export_paths_cannot_duplicate_or_disagree_about_file_directory_identity(tmp_path, names):
    wheel = tmp_path / "paths-1.0-py3-none-any.whl"
    with zipfile.ZipFile(wheel, "w") as archive:
        for name in names:
            archive.writestr(name, b"")
    with pytest.raises(SystemExit, match="duplicate native export archive paths"):
        licenses.check_wheel_files([wheel])


@pytest.mark.parametrize("attributes", [0o040755 << 16, 0x10])
def test_directory_attributes_cannot_hide_payload_without_a_trailing_slash(tmp_path, attributes):
    wheel = tmp_path / "paths-1.0-py3-none-any.whl"
    member = zipfile.ZipInfo("codec/payload.data")
    member.external_attr = attributes
    with zipfile.ZipFile(wheel, "w") as archive:
        archive.writestr(member, b"plain directory payload")
    with pytest.raises(SystemExit, match="directory payload"):
        licenses.check_wheel_files([wheel])


def test_empty_directory_cannot_satisfy_an_expected_openmp_replacement(tmp_path):
    wheel = export_wheel(tmp_path, b"", "libiomp5md.dll/")
    expected = {f"{wheel.name}!/codec/libiomp5md.dll/": hashlib.sha256(PAYLOAD).hexdigest()}
    with pytest.raises(SystemExit, match="missing LLVM OpenMP replacement"):
        licenses.check_wheel_files([wheel], approved_openmp=expected)
