# qpdf pack investigation

Status: incomplete. No pack or release workflow has been delivered.

## Decision required

The approved D4 requires static libjpeg-turbo on Linux and permits the upstream
macOS binaries only after a licence review. The pinned libjpeg-turbo 3.2.1
`LICENSE.md` states that its libjpeg API library is covered by IJG. SPDX lists
IJG as not OSI-approved: https://spdx.org/licenses/IJG.html. The repository
rules prohibit non-OSI code in distributed artifacts, and D4 explicitly forbids
new licence exceptions without a maintainer decision. The existing Docling
exception is confined to that out-of-process pack and cannot be applied here.
The macOS binaries were not independently reviewed, so their exact dependency
and licence inventory is not established. The manager needs to resolve the
Linux conflict and complete the macOS licence review before a distributable
pack can be finalized. No LGPL fallback was selected.

## Verified upstream inputs

The qpdf SHA-256 file is itself a PGP clear-signed message. `gpg --verify`
accepted it using the upstream key at https://q.ql.org/pubkey.asc; its
fingerprint is `C2C9 6B10 011F E009 E6D1 DF82 8A75 D109 9801 2C7E`.
The fingerprint matches the 12.4.2 source README. GPG reports the key as not
certified by a locally trusted signature. The following five hashes appear in
that signed message; the source tarball and development Linux x64 zip were
also downloaded and hashed locally.

| File | SHA-256 |
| --- | --- |
| qpdf-12.4.2.tar.gz | `8a58af5b6141319287c1883bec8bd1bd545b7567b7fc5e6ce5d25a1c85f36397` |
| qpdf-12.4.2-bin-macos-arm64.zip | `62e46987a30ea167cbc530ccb22690aec3d8c812ed09979a941bcee92e504b79` |
| qpdf-12.4.2-bin-macos-x86_64.zip | `dd3b01f4414d198529bb0f13bba59c2f016a328bb3506695087a96fb80fc1481` |
| qpdf-12.4.2-bin-linux-x86_64.zip | `db367d897829f22c4198ce1094143c9d467bd6ee7dfabc44ba6f02056b24f8b1` |
| qpdf-12.4.2-bin-linux-aarch64.zip | `8fd9d009eb0838398180a603f2cf76531e5c68a4ecc8847598e0a1ccb1d3b600` |
| libjpeg-turbo-3.2.1.tar.gz | `c27ad72a68da151177929aa969ae773a2da5ac2af9ce946079314fdac8cece7f` |
| zlib-1.3.2.tar.gz | `bb329a0a2cd0274d05519d61c667c062e06990d72e125ee2dfa8de64f0119d16` |

The libjpeg-turbo digest matches its GitHub release API; the zlib digest
matches https://zlib.net/zlib.html. Both source archives were downloaded and
hashed locally. qpdf source: https://github.com/qpdf/qpdf/releases/tag/v12.4.2;
libjpeg source: https://github.com/libjpeg-turbo/libjpeg-turbo/releases/tag/3.2.1.

## Handoff and outstanding work

No manifest, pack index entry, signed component, notice inventory, workflow,
recipe, or production build script exists yet. Therefore exact manifest and
index bytes, notice completeness, and install instructions cannot be verified.
The intended manifest fields are `name = "qpdf"`,
`engine_protocol = "ariad-engine/2"`, and target entrypoint `bin/qpdf`; the
minimum host version must be the release that includes the adapter. The
integrator should update `docs/packs.md` and the architecture row only after
licence and build decisions are resolved and the implementation is verified.
None of the requested pack tests or CI checks can pass before implementation.

Notice candidates verified in the source tarballs are qpdf `LICENSE.txt`
(Apache-2.0), qpdf `NOTICE.md`, libjpeg-turbo `LICENSE.md` and `README.ijg`
(IJG, BSD-3-Clause and Zlib as applicable), and the zlib `README` (Zlib).
The macOS OpenSSL source version and corresponding notice were not verified.
This is not a complete notice inventory.

## Local source-build probe

A disposable, network-disabled `quay.io/pypa/manylinux_2_28_x86_64` container
(image digest `sha256:f6adcfd450f0927a05c66ded9b4c3bb538fbab625b1ca3f35d71ac093b24b57f`)
compiled the pinned zlib and libjpeg-turbo sources. The first qpdf configure
failed because the temporary prefix's `lib64/pkgconfig` was not on
`PKG_CONFIG_PATH`. A second configure used that directory and the D4 CMake
options; it reached qpdf compilation. The probe was stopped before it
finished because the licence decision already blocks distribution. It did not
verify `--show-crypto`, dynamic dependencies, or a complete source build.
The probe is not a CI job and is not a release-ready implementation; the
requested separate source-build job remains outstanding.
