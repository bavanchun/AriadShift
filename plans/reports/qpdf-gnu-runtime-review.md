# qpdf Linux GNU runtime review

Review date: 2026-11-03. Verdict: all four platforms have gaps and must not
ship. The owner approved the named static runtime exception in principle,
conditional on per-platform provenance, notices and corresponding source. This
review does not establish that condition. The gate must remain
`runtime_review_complete = false`; change it to `true` only after all four
platforms pass, the receipt and notice checks below are implemented, and a
release build is bound to the reviewed evidence. The platforms are consistent
in verdict, not in toolchain or source.

## Evidence and limitations

No workflow run on branch `dev` exists in `gh run list --workflow
pack-qpdf.yml --branch dev`. The workflow runs on pull requests, manual
invocation and release tags, not ordinary dev pushes. The latest successful
run was [41491329170](https://github.com/bavanchun/AriadShift/actions/runs/41491329170),
a PR run on `ff774d316557ba82d544917e1f7b854f1c3439d8`, which is an ancestor
of the review base `1bdab0f511d3d3fbd2ee994241dbb03c042d4317`. All four
Linux jobs succeeded; release was skipped. This is not release-tag provenance.
The workflow at the review base uses the image digests below. It passes
`-static-libstdc++ -static-libgcc`; the job logs show compiler versions but do
not record archive paths, hashes, package receipts or link maps. The following
ABI observations are from the downloaded PR artifacts, not a release build.
Absence of dynamic GLIBCXX/GCC versions is expected with static linking and
is not an ABI version or proof of an exact GCC release. ELF comments are
compiler annotations, not source provenance.

| Platform | Workflow image digest (SHA-256) | CI compiler / observed ABI | Verdict |
| --- | --- | --- | --- |
| linux-x64 | `quay.io/pypa/manylinux_2_28_x86_64@sha256:f6adcfd450f0927a05c66ded9b4c3bb538fbab625b1ca3f35d71ac093b24b57f` | GCC 14.2.1; glibc requirements GLIBC_2.2.5 through GLIBC_2.25; x86-64 glibc loader | gap |
| linux-arm64 | `quay.io/pypa/manylinux_2_28_aarch64@sha256:54b271d9562a3686298880df05367909e949ed317e7b1b79b59c696d75f78676` | GCC 14.2.1; GLIBC_2.17 and GLIBC_2.25; AArch64 glibc loader | gap |
| linux-musl-x64 | `alpine:3.24@sha256:be2656061b536c1883c4034790e21d0a09c5aaaf51968b89368a3c0371ea2c9a` | GCC 15.2.0-r5, musl-dev 1.2.6-r3; x86-64 musl loader and libc dependency; no GNU version requirements | gap |
| linux-musl-arm64 | `alpine:3.24@sha256:be2656061b536c1883c4034790e21d0a09c5aaaf51968b89368a3c0371ea2c9a` | GCC 15.2.0-r5, musl-dev 1.2.6-r3; AArch64 musl loader and libc dependency; no GNU version requirements | gap |

The Alpine digest is a multi-platform image reference, not evidence of each
resolved child image digest. Logs record installation of `gcc`, `g++`,
`libstdc++-dev`, `libgcc-static` and `libgcc` at 15.2.0-r5 for both musl
jobs. They do not bind the selected archives to those package files. The
Alpine install uses live, unpinned package repositories even though the base
image is pinned, so future builds can differ.

| Platform | PR binary SHA-256 | PR component archive SHA-256 |
| --- | --- | --- |
| linux-x64 | `2b24be5003816ef87c082a320780de7300dcea712d28c6354840baf4bea721b4` | `95567028bc3b8c89b32b1fcf9cca286510f8bd85c86d994541f5180c476fad3c` |
| linux-arm64 | `11e0939656b39820776ca2edf862badd3e104aeaedc532094a508897a6252a01` | `d6653efe62c785c4e5ed4f7437e9a44531c9b31a916acfd8e8f5ad5c1008b589` |
| linux-musl-x64 | `1be83cf544c8a3db6da3911c8abeb565c5ebc88ce007f2533a8f7f0a49868703` | `163d1c9196eaad7b83134f8f987952e5e6e27965ade9523fe491d4630627a71a` |
| linux-musl-arm64 | `5e273b70e85f52bd8d3b6937c396931f4635caca6a33fd9e26b5e9db56e7b03e` | `fd2bd1b3d6c3727898b5e22a6a5768366996519719531c69d54b03192b977ae8` |

Inspection used `readelf -V`, `readelf -l`, `readelf -d`, `readelf -Ws`,
`readelf -p .comment` and SHA-256 of each extracted binary. Dynamic dependencies
are glibc/libm/loader on glibc and musl libc on musl. Embedded C++ exception
and unwind symbols are present. These observations do not establish the
archive byte identities or full ABI series of the statically linked runtimes.

## Source and provenance evidence

Both pinned manylinux images were inspected locally. Their GCC and C++
development RPMs identify AlmaLinux `gcc-toolset-14-gcc-14.2.1-11.el8_10.src.rpm`
as source. The exact [AlmaLinux vault SRPM](https://repo.almalinux.org/vault/8.10/AppStream/Source/Packages/gcc-toolset-14-gcc-14.2.1-11.el8_10.src.rpm)
was downloaded and has SHA-256
`0131355464ff7b4958d0d668b3166e05c95c9d0bc4af243ff580a10b78b123eb`.
Its AlmaLinux signature and RPM digests passed `rpm -Kv` in the pinned x64
image. Both images report GCC 14.2.1 20250110 (Red Hat 14.2.1-11); RPM owns
libstdc++.a in `gcc-toolset-14-libstdc++-devel` and libgcc.a in
`gcc-toolset-14-gcc`. The SHA-256 of these files in each pinned image is:

| Platform | libstdc++.a | libgcc.a |
| --- | --- | --- |
| linux-x64 | `308fa896e2bdae71da3b8dc67fc577d3951c74e8bc7a6e6c96e9da7ee1911784` | `99358806eb985f6202a57b56ea2f105d1e4c987729421cc136b4b7c0678a3b9a` |
| linux-arm64 | `d8dcea39c8507d21c2274e9353b1ee720f172b303dcb5e1bf4d4cfbe38f72f2c` | `69c8b51af4a99672127fffc2fe892d0d57329e2b4058bcd925edc44fada8c213` |

For both musl platforms, downloaded Alpine v3.24 GCC, libgcc-static and
libstdc++-dev APKs identify GCC 15.2.0-r5 and aports commit
[`423a8ad043d07f2c7546c8ec3e2b0384cda360ae`](https://github.com/alpinelinux/aports/tree/423a8ad043d07f2c7546c8ec3e2b0384cda360ae/main/gcc)
in `.PKGINFO`. The [source and patches snapshot](https://codeload.github.com/alpinelinux/aports/tar.gz/423a8ad043d07f2c7546c8ec3e2b0384cda360ae)
has SHA-256 `e2f1c69afc7f9117488e7d8d2da2daf19ba251c1b9a71c29049b5297eb0eb10e`;
its APKBUILD SHA-256 is `71d831dbef88c2b0ff7b36b7422e878a07751419583a44b06cda2e561cdb82d4`.
The recipe references [GCC 15.2.0](https://ftp.gnu.org/gnu/gcc/gcc-15.2.0/gcc-15.2.0.tar.xz),
SHA-256 `438fd996826b0c82485a29da03a72d71d6e3541a83ec702df4271f6fe025d24e`.
The downloaded archive's SHA-512 matches the APKBUILD:
`89047a2e07bd9da265b507b516ed3635adb17491c7f4f67cf090f0bd5b3fc7f2ee6e4cc4008beef7ca884b6b71dffe2bb652b21f01a702e17b468cca2d10b2de`.

| Platform | libgcc-static APK SHA-256 | libstdc++-dev APK SHA-256 |
| --- | --- | --- |
| linux-musl-x64 | `563244a562f5c48b921f7f0b4a571e373cfcba65175fe2d4f752c5f975738bcd` | `62ce308cdbf127dcac6eee1520a4ffec8dc98b30974653908842fe4cc7d90b43` |
| linux-musl-arm64 | `2ec3dc4be953d842cc51193248367d86829ed699a1ed094fb5149b1b3fc62919` | `8385c811d93b64ee1877005a5473cde8ebabe82d346301f2b153ff8cb6a71510` |

The GCC APK SHA-256 is `d1d482472ed91240731c60dbae8176bc1e32e1dfb694856da6759f6d32716325`
(x64) and `ff3fecfc32c9a1bed4a47a32ccfee6f6f575708258f912c218dce8ed0d892221`
(arm64). These files were downloaded from `https://dl-cdn.alpinelinux.org/alpine/v3.24/main/ARCH/NAME-15.2.0-r5.apk`,
where `ARCH` is `x86_64` or `aarch64` and `NAME` is `libgcc-static` or
`libstdc++-dev` or `gcc`. Extracted libgcc.a hashes are
`aeae756b465a790521bc47a999d0c935c3efad7df06e1b89d334f52d371918e2`
(x64) and `13418fd4e14bf4cd601f3d41addbec08617533454cc6fef2a0ce92ecbf83118f`
(arm64); libstdc++.a hashes are
`3c5a681959e644f7274b39a9efb4dbb56533a57306a456ce3580bd94f31520a5`
(x64) and `b2b03b70be60cd0ae15cdca348fb002313786d229391a6b064c747528de68936`
(arm64). They are current vendor packages matching logged versions; the CI
did not record APK hashes, so byte identity with its installed files is not
independently established.

The musl-dev 1.2.6-r3 APKs name aports commit
[`8f79d39391b46d6dbf1800fb68b8212a21d5ee2e`](https://github.com/alpinelinux/aports/tree/8f79d39391b46d6dbf1800fb68b8212a21d5ee2e/main/musl).
Its [source and patches snapshot](https://codeload.github.com/alpinelinux/aports/tar.gz/8f79d39391b46d6dbf1800fb68b8212a21d5ee2e)
has SHA-256 `ea30c7a4e842d60d44f71796bbd5ab84a2f9cc5c47f4da1558043fca9a9b79d4`;
the APKBUILD has SHA-256 `22c5adb3f3b68ab84e5ff7d20df68447063d1ecb5c7b39afd5a04a1dfd444a35`.
The [musl 1.2.6 archive](https://musl.libc.org/releases/musl-1.2.6.tar.gz)
has SHA-256 `d585fd3b613c66151fc3249e8ed44f77020cb5e6c1e635a616d3f9f82460512a`;
its SHA-512 matches APKBUILD
`1adad96eddb3a2eb0cacb3e363b0046568925fcdd75cf8b0503f2139df1f693d64730779ca0ce8131b7624ab2d37f4247bb1d3393c523de6e30d2b1d7732555c`.
The musl-dev APK SHA-256 is `310b51cff12e1d5ce5f135c558a82e2550427da5465bbd16c9ff9b05e63ee3d9`
(x64) and `45afe0a9f78de3402b6b5d11f5153ed955bf59d59b45344d9c3fed9f4bbf2f66`
(arm64). The URLs use the same Alpine path pattern above with package
`musl-dev-1.2.6-r3.apk`.

The available green run and commit are recorded above and correspond to the
four workflow image references. They are PR build provenance; there was no
release tag and the release job was skipped. A PR run cannot establish the
identity or source inputs of a future tagged release. The [GCC archive
index](https://ftp.gnu.org/gnu/gcc/) provides upstream releases, not vendor
patch provenance on its own.

## Notices and gate requirements

The existing GPL-3.0 and GCC RLE 3.1 texts match the Docling copies byte for
byte. This change adds the complete Docling LGPL-2.1 text and a written
three-year source offer; `make.sh` sends the notices directory to the shared
pack builder. The new source-pointer notice gives the GCC index and explicitly
requires verification. Before distribution, mirror the verified source RPM, Alpine source archives,
aports snapshots and build instructions as immutable release assets, retain
them for at least three years after the last distribution, and record their
URLs and hashes in the source-pointer notice. This is a mirror plan, not
evidence that the assets have already been uploaded or retained. The offer
text alone cannot establish that maintainers can fulfil it for these builds. Historic PR artifacts predate these files.
A fresh release archive must be inspected before distribution.

The gate currently compares a single boolean and does not validate source,
notice bytes or per-platform provenance. Before any later flip, require a
receipt for each platform binding: image and resolved architecture digest;
package names, versions, package hashes, compiler and selected runtime archive
hashes; link evidence; binary and component hashes; ABI observations; source
URL and SHA-256 with vendor patches, build instructions and release-tag build
provenance; and the five GNU notice hashes. Verify that all notice files are
present in every archive and listed in its manifest, and that corresponding
sources are retained and available for the full offer period. Reject missing,
changed or unreviewed receipts and package drift. No fabricated receipt is
added in this review.

Assumptions: PR artifacts are unsigned review evidence only; the exact GCC
release is not inferred from ABI versions. Version-matched APKs are not proof
of byte-identical CI inputs, and a mirror plan is not a published mirror.

Validation: the four artifacts were downloaded with `gh run download` and
inspected as above; `bash scripts/test-pack-qpdf.sh` passed. The release gate
was deliberately not changed.

## Closing steps

The pack workflow records Linux package inventories, image references, compiler
versions and runtime hashes. Alpine receipts hash cached APKs used for a full
package reinstall and the separately downloaded matching `build-base`
metapackage; RPM receipts hash the installed files per package. The release job checks and retains the reviewed source archives and
receipts as release assets. The release gate remains closed until a release-tag
run is reviewed and the Mac PM explicitly authorizes the flag change. Review
the release-tag artifacts, notices and provenance against the requirements
above before that decision; no release is authorized by this change.
