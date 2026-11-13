"""Toolchain export requires positive native licence identities, even after renames."""

import copy
import hashlib
import re
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


@pytest.mark.parametrize("path", ["", "codec\0LICENSE"], ids=["empty", "nul"])
@pytest.mark.parametrize("scope", ["site", "upstream"])
@pytest.mark.parametrize("artifact_kind", ["wheel", "loose"])
def test_native_export_notice_paths_reject_empty_and_nul_receipts(
    tmp_path, monkeypatch, path, scope, artifact_kind
):
    component = review(monkeypatch)
    component["notice_scope"] = scope
    component["notices"][0]["path"] = path
    if artifact_kind == "wheel":
        artifact = export_wheel(tmp_path)
        with zipfile.ZipFile(artifact, "a") as archive:
            archive.writestr("notices", NOTICE)
    else:
        artifact = tmp_path / "payload.data"
        artifact.write_bytes(PAYLOAD)
        (tmp_path / "notices").write_bytes(NOTICE)
    # An empty upstream receipt must not resolve to a hash-matching notices file.
    with pytest.raises(SystemExit, match="native export notice path is unsafe"):
        licenses.check_wheel_files([artifact])


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


SEPARATOR = licenses._NOTICE_SEPARATOR
OPENCV_LICENSE = (
    "libjpeg-turbo is covered by the Independent JPEG Group licence.\n"
    f"{SEPARATOR}\n"
    "libpng is covered by the PNG Reference Library License version 2.\n"
    f"{SEPARATOR}\n"
    "The TIFF library is copyright Sam Leffler and Silicon Graphics, Inc.\n"
)
IPP_SECTION = (
    f"\n{licenses._IPP_NOTICE_HEADING}\nIntel Simplified Software License\nIntel IPP ICV library.\n"
)
RAW_THIRD_PARTY = (OPENCV_LICENSE + SEPARATOR + IPP_SECTION).encode()
OPENCV_PAYLOAD = b"\xcf\xfa\xed\xfe" + bytes(16) + b"reviewed synthetic OpenCV extension"
OPENCV_WHEEL = "opencv_python_headless-5.0.0.93-cp37-abi3-macosx_14_0_arm64.whl"
REVIEWED_TERMS = ["IJG", "libpng-2.0", "libtiff"]
UNLISTED = "native export lacks an allowed component licence/review"


def opencv_export(tmp_path, monkeypatch, *, terms=REVIEWED_TERMS, third_party=RAW_THIRD_PARTY):
    """An OpenCV-style wheel whose receipt hashes the raw, unstripped third-party notice."""
    component = {
        "name": "opencv-export",
        "distribution": "opencv-python-headless",
        "version": "5.0.0.93",
        "license": "Apache-2.0 AND IJG AND MIT AND libpng-2.0 AND libtiff",
        "files": ["cv2/cv2.abi3.so"],
        "notices": [
            {"path": "cv2/LICENSE.txt", "sha256": hashlib.sha256(NOTICE).hexdigest()},
            {
                "path": "cv2/LICENSE-3RD-PARTY.txt",
                "sha256": hashlib.sha256(RAW_THIRD_PARTY).hexdigest(),
            },
        ],
        "export_objects": {"cv2/cv2.abi3.so": hashlib.sha256(OPENCV_PAYLOAD).hexdigest()},
    }
    if terms is not None:
        component["reviewed_terms"] = terms
    monkeypatch.setattr(licenses, "_NATIVE_COMPONENTS", {"opencv-export": component})
    wheel = tmp_path / OPENCV_WHEEL
    with zipfile.ZipFile(wheel, "w") as archive:
        archive.writestr("cv2/cv2.abi3.so", OPENCV_PAYLOAD)
        archive.writestr("cv2/LICENSE.txt", NOTICE)
        archive.writestr("cv2/LICENSE-3RD-PARTY.txt", third_party)
    return component, wheel


def test_bound_native_terms_are_the_permissive_static_table_terms():
    assert licenses._BOUND_NATIVE_TERMS == frozenset(
        {
            "0BSD",
            "BSD-2-Clause",
            "BSD-3-Clause",
            "FTL",
            "IJG",
            "MIT",
            "Zlib",
            "libpng-2.0",
            "libtiff",
        }
    )


def test_reviewed_terms_are_accepted_with_their_raw_notice_text(tmp_path, monkeypatch):
    _, wheel = opencv_export(tmp_path, monkeypatch)
    licenses.check_wheel_files([wheel])


@pytest.mark.parametrize("term", REVIEWED_TERMS)
@pytest.mark.parametrize("listed", ["without-term", "no-field"])
def test_licence_terms_beyond_the_allow_list_need_a_reviewed_term(
    tmp_path, monkeypatch, term, listed
):
    terms = None if listed == "no-field" else [item for item in REVIEWED_TERMS if item != term]
    _, wheel = opencv_export(tmp_path, monkeypatch, terms=terms)
    with pytest.raises(SystemExit, match=UNLISTED):
        licenses.check_wheel_files([wheel])


@pytest.mark.parametrize(
    "term",
    [
        "GPL-3.0-only",
        "LGPL-2.1-only",
        "LGPL-2.1-or-later",
        "GPL-3.0-or-later WITH GCC-exception-3.1",
        "LicenseRef-Unbound",
    ],
)
def test_reviewed_terms_cannot_approve_unbound_or_copyleft_terms(tmp_path, monkeypatch, term):
    component, wheel = opencv_export(tmp_path, monkeypatch, terms=[*REVIEWED_TERMS, term])
    component["license"] += f" AND {term}"
    with pytest.raises(SystemExit, match=UNLISTED):
        licenses.check_wheel_files([wheel])


@pytest.mark.parametrize("terms", ["IJG", [["IJG"]], [1]], ids=["string", "nested", "integer"])
def test_reviewed_terms_must_be_a_list_of_strings(tmp_path, monkeypatch, terms):
    _, wheel = opencv_export(tmp_path, monkeypatch, terms=terms)
    with pytest.raises(SystemExit, match=UNLISTED):
        licenses.check_wheel_files([wheel])


@pytest.mark.parametrize("term", REVIEWED_TERMS)
def test_reviewed_terms_require_their_codec_text_in_the_bundled_notices(
    tmp_path, monkeypatch, term
):
    text = OPENCV_LICENSE
    for marker in licenses._REVIEWED_TERM_MARKERS[term]:
        text = re.sub(re.escape(marker), "redacted", text, flags=re.IGNORECASE)
    raw = (text + SEPARATOR + IPP_SECTION).encode()
    component, wheel = opencv_export(tmp_path, monkeypatch, third_party=raw)
    component["notices"][1]["sha256"] = hashlib.sha256(raw).hexdigest()
    message = f"native export notice omits reviewed {re.escape(term)} text"
    with pytest.raises(SystemExit, match=message):
        licenses.check_wheel_files([wheel])


def test_a_reviewed_term_without_known_markers_is_refused(tmp_path, monkeypatch):
    component, wheel = opencv_export(tmp_path, monkeypatch, terms=[*REVIEWED_TERMS, "0BSD"])
    component["license"] += " AND 0BSD"
    with pytest.raises(SystemExit, match="native export notice omits reviewed 0BSD text"):
        licenses.check_wheel_files([wheel])


def test_stripped_opencv_notice_is_refused_instead_of_the_raw_receipt(tmp_path, monkeypatch):
    stripped = licenses._strip_reviewed_opencv_ipp_notice(RAW_THIRD_PARTY.decode(), "demo")
    assert stripped.encode() != RAW_THIRD_PARTY
    _, wheel = opencv_export(tmp_path, monkeypatch, third_party=stripped.encode())
    with pytest.raises(SystemExit, match="unreviewed OpenCV IPP notice layout"):
        licenses.check_wheel_files([wheel])


def test_altered_opencv_notice_does_not_match_the_raw_receipt(tmp_path, monkeypatch):
    altered = RAW_THIRD_PARTY + b"additional unreviewed text\n"
    _, wheel = opencv_export(tmp_path, monkeypatch, third_party=altered)
    with pytest.raises(SystemExit, match=r"native export lacks its reviewed notice.*LICENSE-3RD"):
        licenses.check_wheel_files([wheel])


def test_raw_opencv_notice_with_its_ipp_section_passes_the_export_scan(tmp_path, monkeypatch):
    # The scan strips the reviewed x86 IPP section, while the receipt hashes the raw bytes.
    assert b"IPP ICV library" in RAW_THIRD_PARTY
    _, wheel = opencv_export(tmp_path, monkeypatch)
    licenses.check_wheel_files([wheel])


PLATFORM = "x86_64-unknown-linux-gnu"
GOMP_PATH = "torch.libs/libgomp-abcdef01.so.1.0.0"
TORCH_WHEEL = "torch-2.14.1+cpu-cp314-cp314-manylinux_2_27_x86_64.manylinux_2_28_x86_64.whl"


def elf(kind=3):
    header = b"\x7fELF\x02\x01\x01\x00" + bytes(8) + kind.to_bytes(2, "little") + b"\x3e\x00"
    return header + b"synthetic GNU OpenMP runtime"


def gnu_receipt(monkeypatch, payload):
    monkeypatch.setattr(licenses, "_NATIVE_COMPONENTS", {})
    monkeypatch.setattr(
        licenses,
        "_GNU_TARGET_SHARED_FILES",
        {(PLATFORM, "torch", GOMP_PATH): hashlib.sha256(payload).hexdigest()},
    )


def gnu_wheel(tmp_path, payload, member=GOMP_PATH, name=TORCH_WHEEL):
    wheel = tmp_path / name
    with zipfile.ZipFile(wheel, "w") as archive:
        archive.writestr(member, payload)
    return wheel


@pytest.mark.parametrize("platform", [PLATFORM, None], ids=["explicit-triple", "wheel-tag"])
def test_listed_gnu_runtime_is_accepted_by_triple_distribution_and_path(
    tmp_path, monkeypatch, platform
):
    gnu_receipt(monkeypatch, elf())
    licenses.check_wheel_files([gnu_wheel(tmp_path, elf())], target=platform)


@pytest.mark.parametrize(
    ("scenario", "expected"),
    [
        ("digest", "unreviewed native export"),
        ("path", "unreviewed native export"),
        ("distribution", "unreviewed native export"),
        ("triple", "unreviewed native export"),
        ("tag", "unreviewed native export"),
        ("executable", "not an ELF shared object"),
        ("archive", "not an ELF shared object"),
    ],
)
def test_unlisted_or_mismatched_gnu_runtimes_are_refused(tmp_path, monkeypatch, scenario, expected):
    reviewed = payload = elf()
    member, name, platform = GOMP_PATH, TORCH_WHEEL, PLATFORM
    if scenario == "digest":
        payload = elf() + b" changed"
    elif scenario == "path":
        member = "torch.libs/libgomp-ffffffff.so.1.0.0"
    elif scenario == "distribution":
        name = TORCH_WHEEL.replace("torch-", "scipy-", 1)
    elif scenario == "triple":
        platform = "aarch64-unknown-linux-gnu"
    elif scenario == "tag":
        name, platform = TORCH_WHEEL.replace("x86_64", "aarch64"), None
    elif scenario == "executable":
        reviewed = payload = elf(2)
    else:
        reviewed = payload = b"!<arch>\n" + bytes(32)
    gnu_receipt(monkeypatch, reviewed)
    wheel = gnu_wheel(tmp_path, payload, member, name)
    with pytest.raises(SystemExit, match=expected):
        licenses.check_wheel_files([wheel], target=platform)


@pytest.mark.parametrize(
    ("filename", "explicit", "expected"),
    [
        (TORCH_WHEEL, None, ("torch", PLATFORM)),
        (
            "torch-2.14.1-cp314-cp314-manylinux_2_28_aarch64.whl",
            None,
            ("torch", "aarch64-unknown-linux-gnu"),
        ),
        (OPENCV_WHEEL, None, ("opencv-python-headless", "aarch64-apple-darwin")),
        ("pkg-1.0-cp314-cp314-win_amd64.whl", None, ("pkg", "x86_64-pc-windows-msvc")),
        ("pkg-1.0-py3-none-any.whl", None, ("pkg", None)),
        ("pkg-1.0-py3-none-any.whl", PLATFORM, ("pkg", PLATFORM)),
        ("libgomp.so.1", None, (None, None)),
        ("libgomp.so.1", PLATFORM, (None, PLATFORM)),
    ],
)
def test_wheel_identity_names_the_distribution_and_triple(filename, explicit, expected):
    assert licenses._wheel_identity(filename, explicit) == expected


TORCH_NATIVE_PATH = "torch/lib/libtorch_cpu.so"
TORCH_NOTICE_NAMES = (
    "pytorch-2.14.1-native-notices.txt",
    "llvm-openmp-notice.txt",
    "ittapi.txt",
)
NOTICE_ROOT = REPOSITORY / "engines/docling/pack/licenses"
REAL_GOMP_PATH = "torch.libs/libgomp-6a29c32d.so.1.0.0"


def torch_native_wheel(tmp_path, payload, member=TORCH_NATIVE_PATH):
    wheel = tmp_path / TORCH_WHEEL
    with zipfile.ZipFile(wheel, "w") as archive:
        archive.writestr(member, payload)
        for name in TORCH_NOTICE_NAMES:
            archive.writestr(f"notices/{name}", (NOTICE_ROOT / name).read_bytes())
    return wheel


def review_torch_payload(monkeypatch, payload):
    """Review the synthetic bytes through a copy of the real torch receipt (notices stay real)."""
    component = copy.deepcopy(licenses._NATIVE_COMPONENTS["torch-native"])
    component["export_objects"] = {TORCH_NATIVE_PATH: hashlib.sha256(payload).hexdigest()}
    monkeypatch.setitem(licenses._NATIVE_COMPONENTS, "torch-native", component)


def test_torch_wheel_may_carry_reviewed_itt_markers_with_its_notices(tmp_path, monkeypatch):
    payload = elf() + b" ".join(licenses._ITT_MARKERS)
    review_torch_payload(monkeypatch, payload)
    wheel = torch_native_wheel(tmp_path, payload)
    licenses.check_wheel_files([wheel], target=PLATFORM)


def test_torch_wheel_itt_markers_still_need_the_bundled_ittapi_notice(tmp_path, monkeypatch):
    payload = elf() + b" ".join(licenses._ITT_MARKERS)
    review_torch_payload(monkeypatch, payload)
    wheel = tmp_path / TORCH_WHEEL
    with zipfile.ZipFile(wheel, "w") as archive:
        archive.writestr(TORCH_NATIVE_PATH, payload)
        for name in TORCH_NOTICE_NAMES[:2]:
            archive.writestr(f"notices/{name}", (NOTICE_ROOT / name).read_bytes())
    with pytest.raises(SystemExit, match=r"native export lacks its reviewed notice.*ittapi"):
        licenses.check_wheel_files([wheel], target=PLATFORM)


@pytest.mark.parametrize("allow_itt", [False, True])
@pytest.mark.parametrize("marker", licenses._INTEL_MARKERS, ids=repr)
def test_torch_wheel_never_carries_proprietary_intel_markers(tmp_path, allow_itt, marker):
    wheel = torch_native_wheel(tmp_path, elf() + marker)
    with pytest.raises(SystemExit, match="proprietary Intel binary content"):
        licenses.check_wheel_files([wheel], allow_itt=allow_itt, target=PLATFORM)


@pytest.mark.parametrize("allow_itt", [False, True])
@pytest.mark.parametrize("library", licenses._INTEL_LIBRARIES)
def test_torch_wheel_never_carries_unapproved_intel_libraries(tmp_path, allow_itt, library):
    wheel = torch_native_wheel(tmp_path, elf(), member=f"torch/lib/{library}.so")
    with pytest.raises(SystemExit, match="unapproved Intel library"):
        licenses.check_wheel_files([wheel], allow_itt=allow_itt, target=PLATFORM)


@pytest.mark.parametrize("marker", licenses._ITT_MARKERS, ids=repr)
def test_itt_markers_stay_refused_outside_torch_wheels(tmp_path, marker):
    opencv = tmp_path / OPENCV_WHEEL
    with zipfile.ZipFile(opencv, "w") as archive:
        archive.writestr("cv2/cv2.abi3.so", b"\xcf\xfa\xed\xfe" + bytes(16) + marker)
    with pytest.raises(SystemExit, match="proprietary Intel binary content"):
        licenses.check_wheel_files([opencv])
    sidecar = tmp_path / "sidecar.so"
    sidecar.write_bytes(elf() + marker)
    with pytest.raises(SystemExit, match="proprietary Intel binary content"):
        licenses.check_wheel_files([sidecar])
    openmp = tmp_path / "libiomp5md.dll"
    openmp.write_bytes(b"MZ" + bytes(64) + marker)
    wrapper = tmp_path / "wrapper-1.0-py3-none-any.whl"
    with zipfile.ZipFile(wrapper, "w") as archive:
        archive.writestr("wrapper.py", "# No native code here\n")
    with pytest.raises(SystemExit, match="proprietary Intel binary content"):
        licenses.check_wheel_files([wrapper], openmp_dll=openmp)


def test_itt_library_name_stays_refused_in_an_opencv_wheel(tmp_path):
    opencv = tmp_path / OPENCV_WHEEL
    with zipfile.ZipFile(opencv, "w") as archive:
        archive.writestr("cv2/ittnotify.so", elf())
    with pytest.raises(SystemExit, match="unapproved Intel library"):
        licenses.check_wheel_files([opencv])


@pytest.mark.parametrize(
    ("name", "digest"),
    [("opencv-export-macos", "6034c601"), ("opencv-export-linux", "4dc06d80")],
)
def test_opencv_export_receipts_are_complete(name, digest):
    component = licenses._NATIVE_COMPONENTS[name]
    assert component["export_only"] is True
    assert component["reviewed_terms"] == ["IJG", "libpng-2.0", "libtiff"]
    assert component["export_objects"]["cv2/cv2.abi3.so"].startswith(digest)
    assert (
        licenses.verdict(
            component["distribution"],
            component["license"],
            allowed=licenses.ALLOWED | frozenset(component["reviewed_terms"]),
        )
        is None
    )
    assert {item["path"]: item["sha256"][:8] for item in component["notices"]} == {
        "cv2/LICENSE.txt": "09d71905",
        "cv2/LICENSE-3RD-PARTY.txt": "2537f565",
    }


def test_real_torch_receipts_cover_every_reviewed_object_notice_and_gnu_runtime(tmp_path):
    component = licenses._NATIVE_COMPONENTS["torch-native"]
    linux_objects = {
        path: digest
        for path, digest in component["export_objects"].items()
        if "aarch64" not in path
    }
    assert len(linux_objects) == 10
    assert {item["path"] for item in component["notices"]} == set(TORCH_NOTICE_NAMES)
    wheel = tmp_path / TORCH_WHEEL
    errors: list[str] = []
    with zipfile.ZipFile(wheel, "w") as archive:
        for name in TORCH_NOTICE_NAMES:
            archive.writestr(f"notices/{name}", (NOTICE_ROOT / name).read_bytes())
        archive.writestr(REAL_GOMP_PATH, elf())
        reviewed = [*linux_objects.items()]
        reviewed.append(
            (REAL_GOMP_PATH, licenses._GNU_TARGET_SHARED_FILES[(PLATFORM, "torch", REAL_GOMP_PATH)])
        )
        for path, digest in reviewed:
            licenses._export_native_review(
                digest, f"{TORCH_WHEEL}!/{path}", archive, wheel, errors, target=PLATFORM
            )
    assert errors == []
