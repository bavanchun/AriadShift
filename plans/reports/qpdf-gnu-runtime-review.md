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

## Source and provenance gaps

The locally available pinned x64 manylinux image reports GCC 14.2.1 20250110
(Red Hat 14.2.1-11). RPM ownership identifies `gcc-toolset-14-gcc`,
`gcc-toolset-14-gcc-c++` and `gcc-toolset-14-libstdc++-devel`, all
14.2.1-11.el8_10 and all naming source RPM
`gcc-toolset-14-gcc-14.2.1-11.el8_10.src.rpm`. The local image archives have
SHA-256 `308fa896e2bdae71da3b8dc67fc577d3951c74e8bc7a6e6c96e9da7ee1911784`
(libstdc++.a) and
`99358806eb985f6202a57b56ea2f105d1e4c987729421cc136b4b7c0678a3b9a`
(libgcc.a). These are local-image receipts, not a CI link map. Arm64 logs
show the same compiler version but do not establish package ownership or
archive hashes.

A same-named [Rocky Linux source RPM](https://download.rockylinux.org/pub/rocky/8.10/AppStream/source/tree/Packages/g/gcc-toolset-14-gcc-14.2.1-11.el8_10.src.rpm)
was downloaded and hashed `fabd21d53718b7a75100536711fd2d5ef61e1f6a1320837985fd5ae7ced852cc`.
Its vendor and contents have not been tied to the AlmaLinux image package
build; it is a candidate, not a corresponding-source receipt. An attempted
AlmaLinux source URL returned 404. Obtain and hash the actual vendor SRPM,
prove both image packages derive from it, and preserve build instructions.

For musl, the historical [Alpine aports GCC recipe](https://github.com/alpinelinux/aports/blob/fd4fecacbfd0cd40be42efa3c9d72bc03a88428c/main/gcc/APKBUILD)
has SHA-256 `0c6e047f661398a9da936bfe8ab76067668f0ddb90109155f59e66dea6107988`,
identifies 15.2.0-r5, references GCC 15.2.0 and names Alpine patches. The
recipe is historical evidence, not proof that either installed APK was built
from that revision. The unpinned APKs, per-architecture package hashes,
source archives and patched build recipe remain to be bound to each binary.
A download of the GCC 15.2.0 upstream archive timed out after 120 seconds;
its SHA-256 was therefore not established in this review.
The [GCC archive index](https://ftp.gnu.org/gnu/gcc/) is a source reference,
not proof of vendor corresponding source. No release-tag runtime build
provenance exists in the reviewed run.

## Notices and gate requirements

The existing GPL-3.0 and GCC RLE 3.1 texts match the Docling copies byte for
byte. This change adds the complete Docling LGPL-2.1 text and a written
three-year source offer; `make.sh` sends the notices directory to the shared
pack builder. The new source-pointer notice gives the GCC index and explicitly
requires verification. The text alone cannot establish that maintainers can
fulfil the offer for these builds. Historic PR artifacts predate these files.
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
release is not inferred from ABI versions. Source archives and images from a
similar distribution or matching filenames are not interchangeable proof.

Validation: the four artifacts were downloaded with `gh run download` and
inspected as above; `bash scripts/test-pack-qpdf.sh` passed. The release gate
was deliberately not changed.
