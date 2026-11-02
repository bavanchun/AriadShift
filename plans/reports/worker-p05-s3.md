# qpdf pack handoff

Status: incomplete pending macOS and non-x64 CI validation, owner approval,
and a complete review of statically linked GNU runtime notices and provenance.
The release gate refuses publication until both reviews are complete.

## Implementation

All six targets now build qpdf 12.4.2, libjpeg-turbo 3.2.1 and zlib 1.3.2
from verified source archives. The source build selects native crypto and
static zlib and libjpeg. Linux statically links GNU C++ and GCC runtimes; its
`readelf` check allows only glibc/musl runtime dependencies. macOS uses system
libc++/libSystem and rejects dynamic zlib, jpeg, and OpenSSL. The upstream
macOS binaries are not used. The shared builder ad-hoc codesigns macOS unless
an Apple signing identity is supplied. The pack entrypoint is `bin/qpdf` and
the manifest declares `ariad-engine/2` with host version `>=0.3.0, <2.0.0`.

The workflow has six native-runner targets, digest-pinned Linux containers,
manifest merge and a tag-only signing job in the protected release environment.
The signing job requires stable-branch ancestry and the release gate. The
source build step has a ten-minute limit. Linux arm64, musl and macOS are CI
only and have not been verified locally. A macOS toolchain failure needs to be
reported after the PM's ten-minute investigation limit; do not debug toolchains.

The qpdf-specific IJG exception is restricted to unmodified libjpeg-turbo 3.2.1
and remains `pending_owner_approval = true`. The GNU static runtime review has
its own `runtime_review_complete = false` gate: the manylinux x64 static build
uses GCC 14.2.1 from source RPM
`gcc-toolset-14-gcc-14.2.1-11.el8_10.src.rpm`. GPL-3 and GCC runtime exception
texts are included, but the exact toolchain source, complete notices and source
offer for all target images still need review. No signed release or index entry
exists. The owner item is in the main checkout's coordination file.

`just pack-qpdf <qpdf_dir>` assembles an unsigned host pack at
`target/packs/qpdf`; `just pack-qpdf-verify` checks it. The manager's GnuTLS
binary cache is allowed only for local development, and its manifest marks its
additional shared dependencies unreviewed; CI release components disallow them.
Set `ASHIFT_PACK_QPDF` to the absolute pack directory. The host adapter is
being implemented separately.

## Validation

A clean Linux x64 source build in the pinned manylinux container passed:
`qpdf --show-crypto` printed `native`, and `readelf -d` listed only
`libm.so.6`, `libc.so.6` and `ld-linux-x86-64.so.2`. The output was qpdf
12.4.2. The earlier stale-cache build dynamically linked system zlib; it was
superseded by this clean build. The build-locked `just pack-qpdf` with the manager's prepared Linux cache
passed, as did a separate assembly from the clean source-built binary.
`QPDF_TEST_PACK` checks passed on both packs, including missing-notice refusal;
release guard and archive hash rejection tests passed. `ashift engines`
recognized the cache pack as a development override but reported its engine
description missing because the separate qpdf adapter is not in this base.
The CI scope suite passed 93 checks; ShellCheck and actionlint passed. macOS,
Linux arm64 and musl builds have not run locally.

## Verified upstream inputs

The qpdf checksum file is PGP clear-signed. GPG verified it using the upstream
key at https://q.ql.org/pubkey.asc with fingerprint
`C2C9 6B10 011F E009 E6D1 DF82 8A75 D109 9801 2C7E`, matching the source
README. The key is not certified in the local trust database. The first five
hashes appear in the signed file. The source tarball, both macOS ZIPs, and the
Linux x64 development ZIP were also hashed locally.

| File | SHA-256 |
| --- | --- |
| qpdf-12.4.2.tar.gz | `8a58af5b6141319287c1883bec8bd1bd545b7567b7fc5e6ce5d25a1c85f36397` |
| qpdf-12.4.2-bin-macos-arm64.zip | `62e46987a30ea167cbc530ccb22690aec3d8c812ed09979a941bcee92e504b79` |
| qpdf-12.4.2-bin-macos-x86_64.zip | `dd3b01f4414d198529bb0f13bba59c2f016a328bb3506695087a96fb80fc1481` |
| qpdf-12.4.2-bin-linux-x86_64.zip | `db367d897829f22c4198ce1094143c9d467bd6ee7dfabc44ba6f02056b24f8b1` |
| qpdf-12.4.2-bin-linux-aarch64.zip | `8fd9d009eb0838398180a603f2cf76531e5c68a4ecc8847598e0a1ccb1d3b600` |
| libjpeg-turbo-3.2.1.tar.gz | `c27ad72a68da151177929aa969ae773a2da5ac2af9ce946079314fdac8cece7f` |
| zlib-1.3.2.tar.gz | `bb329a0a2cd0274d05519d61c667c062e06990d72e125ee2dfa8de64f0119d16` |


The libjpeg-turbo digest matches its GitHub release API and the zlib digest
matches https://zlib.net/zlib.html; both downloaded tarballs were hashed.
Notices copied from those pinned sources are qpdf LICENSE.txt and NOTICE.md,
libjpeg-turbo LICENSE.md and README.ijg, and zlib README. OpenSSL notices and
macOS libjpeg 3.2.0 notices are not yet inventoried.

## Observed Linux development manifest

This is the exact unsigned manifest from the local source-built x64 component,
not a signed release manifest. Its component hash and size are build-specific.

```toml
schema = "ariad-pack/1"
name = "qpdf"
version = "0.1.0"
description = "qpdf document engine"
license = "Apache-2.0 AND IJG AND BSD-3-Clause AND Zlib AND GPL-3.0-or-later WITH GCC-exception-3.1"
requires_ashift = ">=0.3.0, <2.0.0"
engine_protocol = "ariad-engine/2"
notices = ["notices/gcc-GPL-3.txt", "notices/gcc-runtime-exception.txt", "notices/libjpeg-turbo-LICENSE.txt", "notices/libjpeg-turbo-README.ijg.txt", "notices/qpdf-LICENSE.txt", "notices/qpdf-NOTICE.txt", "notices/zlib-README.txt"]

[[targets]]
triple = "x86_64-unknown-linux-gnu"
entrypoint = ["bin/qpdf"]
executables = ["bin/qpdf"]

[[targets.components]]
name = "engine"
url = "qpdf-0.1.0-x86_64-unknown-linux-gnu-engine.tar.zst"
sha256 = "acc251c7e44827e307ad09a616ec807c136cffc7b036565c3598ce0a7ea97f2f"
size = 1722429
unpack = "tar.zst"
dest = "."
unpacked_size = 6238892
entries = 10
```

## Remaining work

Run the six-target workflow and validate macOS and the three Linux targets not
available locally. Finish the static GNU runtime licence review, notices,
provenance and corresponding-source offer for every relevant target. Obtain
owner approval of the qpdf IJG exception before enabling publication. Once a
signed release exists, the integrator must add the actual URL, manifest hash
and signing key to `crates/ariad-host/data/packs.toml`; those values do not yet
exist and must not be invented. The final six-target manifest depends on the
actual artifacts and cannot be printed before the workflow completes.
