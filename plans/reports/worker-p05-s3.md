# qpdf pack handoff

Status: incomplete. The PM selected unmodified libjpeg-turbo 3.2.1 with a
named exception pending owner approval. The source-build stop rule was met:
the local qpdf build succeeded with native crypto but loaded system libz and
libm, outside the requested dynamic dependency allowlist. The initial CMake
cache had selected system zlib; the new clean-build script has not been run.
No upstream Linux fallback was selected. The experimental source-build CI job
is retained separately; it does not publish.

## Delivered and checked

`engines/qpdf/pack/licence-review.toml` names only libjpeg-turbo 3.2.1 and
keeps `pending_owner_approval = true`. Its release gate rejects pending,
malformed or mismatched reviews. The tag workflow invokes it in the protected
release environment and then unconditionally refuses release because the pack
is unfinished. The local test covers pending refusal, approved review parsing,
wrong-component refusal, and hash mismatch. It does not test a real signature
or publish operation. The workflow and pack assembler are scaffolding, not a
completed six-target pack. `make.sh dev` rejects prepared directories with
shared libraries, including the manager's GnuTLS development cache, because
its component notice inventory has not been reviewed. No `just pack-qpdf`
recipe or final manifest has been delivered.

`bash scripts/test-pack-qpdf.sh` passed with both real macOS ZIPs;
`bash scripts/test-ci-scope.sh` passed 90 checks. ShellCheck 0.11.0 and
actionlint passed using their explicit installed binary paths. The first
ShellCheck/actionlint invocation failed because the system ShellCheck shim had
no selected version; both passed on rerun with the installed binary.

## macOS binary inventory

Both ZIPs were downloaded and matched the signed hashes below. Each contains
`bin/qpdf`, `bin/fix-qdf`, `bin/zlib-flate`, one
`lib/libqpdf.30.4.2.dylib`, and relative symlink `libqpdf.30.dylib` to it.
Neither ZIP contains licence files. `llvm-objdump --macho --dylibs-used` shows
qpdf depends on libqpdf, libc++, and libSystem; libqpdf depends on system
libz, libc++, and libSystem. Both libqpdf binaries embed the string
`libjpeg-turbo version 3.2.0 (build 20260630)`. Defined OpenSSL symbols such as
`_OPENSSL_init_crypto` and `_EVP_*` are present in libqpdf, with no dynamic
OpenSSL dependency. Its exact OpenSSL version and source/notice have not been
verified. Thus the PM's 3.2.1-only review does not establish coverage of the
3.2.0 binary, and OpenSSL is an additional bundled component. These archives
are not used by the scaffold. The finding was recorded in result.md and the
owner-pending coordination file before any use.

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

## Outstanding work

Resolve the source-build dependency check, then validate the clean script and
all four Linux targets under the stop rule. Determine the correct old-glibc and
Alpine container digests for every architecture. Obtain the macOS decision
and exact OpenSSL provenance, then implement safe ZIP extraction, relative
symlink preservation, macOS signing, cross-architecture verification and the
six-target pack workflow. Complete `just` recipes, manifest merge, signing,
publishing, integration tests and the target index entry after the first signed
release. The planned manifest is `ariad-pack/1`, name `qpdf`, protocol
`ariad-engine/2`, entrypoint `bin/qpdf`, and a minimum host version containing
the adapter; final component hashes and URLs do not yet exist. Linux arm64 and
both musl builds remain unverified locally.

## Decision needed

The owner must approve the narrowly scoped 3.2.1 exception before distribution.
The PM must decide separately whether the upstream macOS libjpeg-turbo 3.2.0
and statically linked OpenSSL may be included and how their exact notices and
provenance will be established.
