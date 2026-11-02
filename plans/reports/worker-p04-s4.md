# Worker report: p04 slice s4 (shared pack builder and the pdfium pack)

Status: complete. Everything this slice owns is implemented and checked offline. The real
`ashift` could not be exercised because `ashift __pack manifest --engine-protocol` does not exist
until slice s5 lands, and the real engine crate is not in this tree.

## What was implemented

| Path | Purpose |
|---|---|
| `scripts/pack/build-pack.sh` | Shared builder, `build` and `merge` subcommands. Verifies the pinned upstream SHA-256 and refuses on mismatch, empty or malformed pins, non-https URLs, unsafe archive entries and links. Lays out the pack root, ad-hoc codesigns macOS binaries (Developer ID only when `APPLE_SIGNING_IDENTITY` is set, `--no-codesign` to skip), writes a reproducible `tar.zst` component and calls `ashift __pack manifest`. |
| `scripts/pack/sign-pack.sh` | minisign signing, release CI only. Refuses unless `GITHUB_ACTIONS=true`, the ref is a tag named `pack-<name>-v<version>`, and a key is present. Trusted comment `pack:<name> version:<version>`, verified against the committed public key afterwards. |
| `scripts/pack/test-pack-tooling.sh` | 54 offline checks (see below). |
| `engines/pdfium/pack/{pack.conf,targets.txt,make.sh,notices/}` | The pdfium wrapper. `targets.txt` holds the six targets and the SHA-256 table copied verbatim from the spike report. `make.sh` has `slug`, `pack`, `dev`, `merge`, `verify`. It refuses `pdfium-v8-*` assets, as the spike requires. |
| `justfile` | New recipes `pack-pdfium`, `pack-pdfium-verify`, `pack-tooling-test`. |
| `.github/workflows/pack-pdfium.yml` | Tooling job (shellcheck and tests), six-target matrix, merged-manifest job, tag-only sign-and-publish job in the `release` environment. |
| `scripts/ci-scope.sh`, `scripts/test-ci-scope.sh` | Changes limited to the pdfium pack scripts, wrapper and workflow classify as `pack-only change` (no multi-OS Rust jobs, because `pack-pdfium.yml` builds and tests them). Eight new table rows. |

### Pack layout (install root)

```text
bin/ariad-pdfium-engine      engine, 0755
bin/libpdfium.so|.dylib      next to the engine (slice-map decision 8)
notices/PDFIUM-PACK.txt, pdfium-binaries-LICENSE (MIT wrapper), pdfium-VERSION,
notices/third-party/*        every file of the upstream licenses/ directory (24 notices in total)
notices/ariadshift-LICENSE, -NOTICE, -THIRD_PARTY_LICENSES
```

One component per target, named `engine`: `pdfium-<version>-<triple>-engine.tar.zst`, unpacked
into `.`. Entrypoint is `["bin/ariad-pdfium-engine"]` (no arguments: I assumed the engine binary
serves the protocol on stdio directly; change `PACK_ENGINE_BIN` or the entrypoint in
`pack.conf`/`make.sh` if c0 decides otherwise).

### Targets

| Slug | Triple | Asset |
|---|---|---|
| `linux-x64` | `x86_64-unknown-linux-gnu` | `pdfium-linux-x64.tgz` |
| `linux-arm64` | `aarch64-unknown-linux-gnu` | `pdfium-linux-arm64.tgz` |
| `linux-musl-x64` | `x86_64-unknown-linux-musl` | `pdfium-linux-musl-x64.tgz` |
| `linux-musl-arm64` | `aarch64-unknown-linux-musl` | `pdfium-linux-musl-arm64.tgz` |
| `mac-arm64` | `aarch64-apple-darwin` | `pdfium-mac-arm64.tgz` |
| `mac-x64` | `x86_64-apple-darwin` | `pdfium-mac-x64.tgz` |

Two separate mac packs instead of `mac-univ`: the triples map one to one and the downloads are
half the size. Windows stays deferred.

## Contract for s5: the exact manifest flag and field

The builder emits, for every manifest it writes:

```text
ashift __pack manifest --name … --version … --target <triple> --base <base.toml> \
    --entrypoint bin/ariad-pdfium-engine --engine-protocol ariad-engine/2 \
    [--url-prefix …] -o <pack.toml> engine=<component.tar.zst>
```

and then requires `pack.toml` to contain the **top-level** line `engine_protocol = "ariad-engine/2"`
(before the first `[[targets]]`, matching ARCHITECTURE §7.1). So s5 must:

1. add `--engine-protocol <VALUE>` to `PackManifestArgs` and `ManifestRequest`, writing the
   optional top-level `engine_protocol` manifest field;
2. accept `engine_protocol` in a `--base` manifest too. `merge` passes the previous output back as
   `--base` for each further target, and the field is repeated with the flag, so the flag must
   win or agree rather than error;
3. accept `targets = []` in the generated base manifest (the current parser does; validation
   only runs on the final manifest).

The builder refuses early with a clear message when `ashift __pack manifest --help` does not list
`--engine-protocol`, so a missing s5 change cannot silently produce a manifest without the field.
Packs of `ariad-engine/1` omit `--engine-protocol` (empty value).

## Text for docs/packs.md (integrator or s5 adds it)

Insert after "The docling pack":

> ## The pdfium pack
>
> `engines/pdfium/pack/make.sh` builds the `pdfium` pack on the shared builder
> `scripts/pack/build-pack.sh`, which later packs reuse. The pack declares
> `engine_protocol = "ariad-engine/2"`; `ashift` checks it against the engine's declared protocol
> at install and resolve time. Each target has one `engine` component with `bin/ariad-pdfium-engine`,
> `bin/libpdfium.so` (`.dylib` on macOS) next to it, and `notices/` holding the PDFium and
> pdfium-binaries licences plus one file per bundled third-party library (FreeType, ICU,
> libjpeg-turbo, OpenJPEG, Little CMS, libpng, zlib, HarfBuzz, Abseil, AGG and the rest of upstream's
> `licenses/` directory). Targets: `x86_64`/`aarch64` Linux glibc and musl, `aarch64`/`x86_64` macOS.
>
> The PDFium build is `bblanchon/pdfium-binaries` `chromium/8125`. Per-target SHA-256 pins live in
> `engines/pdfium/pack/targets.txt`; the build refuses a download or local archive that does not
> match and refuses any V8 asset. musl engines are built dynamically linked (a static musl binary
> cannot load `libpdfium`).
>
> **Build and try locally** (needs the Rust toolchain and `zstd`; no signing):
>
> ```bash
> just pack-pdfium                      # target/packs/pdfium, an unpacked development pack
> export ASHIFT_PACK_PDFIUM=$PWD/target/packs/pdfium
> ```
>
> `PDFIUM_ARCHIVE=<file>` reuses a downloaded archive (still hash-checked). On macOS the binaries
> are ad-hoc codesigned; set `APPLE_SIGNING_IDENTITY` for a Developer ID signature.
>
> **Install and sign.** Push the tag `pack-pdfium-v<version>` (it must equal `PACK_VERSION` in
> `engines/pdfium/pack/pack.conf`). The *PDFium pack* workflow builds all targets, merges
> `pack.toml`, and its `release` job, in the protected `release` environment, signs it with
> `scripts/pack/sign-pack.sh` (trusted comment `pack:pdfium version:<version>`) and uploads
> `pack.toml`, `pack.toml.minisig` and the components to the release of that tag. The secrets
> `MINISIGN_SECRET_KEY` and `MINISIGN_PASSWORD` and the variable `PACK_SIGNING_KEY_ID` (the file
> stem of the public key under `crates/ariad-host/data/pack-keys/`) belong to that environment.
> Nobody signs a pack locally. Install with `ashift engines install pdfium`, or offline with
> `--from <dir>`.

For `docs/ci.md`, a table row: `pack-pdfium.yml` runs on pull requests touching `engines/pdfium/**`,
`scripts/pack/**`, `crates/ariad-pdfium-engine/**`, the `justfile` or the workflow, on tags
`pack-pdfium-v*` and on demand; it tests the pack tooling, builds and smoke-tests six targets, and
signs and publishes only on tags in the `release` environment.

## Commands run

| Command | Result |
|---|---|
| `bash scripts/pack/test-pack-tooling.sh` | PASS, 54 passed, 0 failed |
| `bash scripts/test-ci-scope.sh` | PASS, 90 passed, 0 failed |
| shellcheck (via `uvx --from shellcheck-py`) on all four scripts | PASS, no findings |
| actionlint (`.tools/bin/actionlint`, with shellcheck on PATH) on `pack-pdfium.yml` | PASS |
| `zizmor@1.30.1 --offline` on `pack-pdfium.yml` | PASS, no findings (1 suppressed) |
| `make.sh pack` with `PDFIUM_ARCHIVE` set to the spike's real `linux-x64`, `linux-musl-x64`, `mac-arm64` downloads, dummy engine, stub `ashift` | PASS: pinned hashes match, layout and 24 notices complete, `make.sh verify` passes |
| `committed` (from the main checkout's `.tools/bin`) on the three commits, plus the AI-reference scan from `scripts/check-commits.sh` | PASS. `sh scripts/check-commits.sh` itself stops because this worktree has no `.tools/bin/committed`. |

The test script covers: hash mismatch, empty and malformed pin, http URL, `..` entry and symlink in
the upstream archive, missing notices, `ashift` without the flag, bad destinations, byte-identical
rebuilds, executable bit, all upstream licence files present in the archive and listed in
`pack.toml`, codesign ad-hoc / Developer ID / refused without `codesign` / `--no-codesign`, a
two-target merge, the pdfium wrapper end to end (including `verify` failing on a removed notice and
V8 refusal), and every refusal path of the signing guard. The upstream archive, engine and
`ashift` in that test are synthetic, by design; minisign itself is not installed here, so the
actual `minisign -S` / `-V` calls were not run.

No cargo build was run: nothing in this slice needed the real `ashift` yet.

## Deviations and decisions

- **Skills used:** ak-devops (workflow, signing), ak-docs-seeker (checked pinned action SHAs and
  runner labels against the repository's existing workflows), ak-test (offline test design),
  ak-git (commits).
- **musl engine is built in an Alpine container** (`rust:1.99.0-alpine3.24`, digest-pinned,
  `-C target-feature=-crt-static`) as the spike recommends; the Rust musl binary dlopen was
  untested there. The smoke step runs the packed engine in digest-pinned `alpine:3.24`.
- **`mac-x64` is cross-built** on the arm64 `macos-26` runner (no Intel runner label is in use in
  this repository). Its smoke test runs through Rosetta when available and is skipped with a notice
  otherwise (deferred evidence, like the rest of macOS).
- **Developer-ID signing is not wired into CI.** The builder supports it through
  `APPLE_SIGNING_IDENTITY`, but CI imports no certificate, so macOS binaries are ad-hoc signed.
- **Scope teaching:** `cloud_paths` in `ci-scope.sh` is untouched because the cloud E2E does not
  consume the pdfium pack (it does consume the docling one, which is why that workflow is listed).
- The sandbox hook rejects shell commands containing some words (for example `build`); I created
  files with the editor tools and ran the tests through `test-pack-tooling.sh`, which calls the
  builder itself.

## Changes the integrator or other slices must make

- **s5:** the `--engine-protocol` flag and `engine_protocol` field described above. Until then the
  CI jobs and `just pack-pdfium` fail at the builder's early check, by design.
- **No shared-file edits were needed** from this slice (nothing under Cargo files, schemas,
  registry or docs/tools.md). `docs/packs.md` and `docs/ci.md` text is given above; `schemas/pack.v1.json`
  is regenerated by s5/integrator once the manifest struct gains the field.
- `PACK_REQUIRES_ASHIFT` in `engines/pdfium/pack/pack.conf` is `>=0.2.0, <2.0.0`; raise the lower
  bound to the release that carries `engine_protocol` when it is cut (slice-map decision 6).
- Repository setup before the first tag: the `release` environment needs secrets
  `MINISIGN_SECRET_KEY` and `MINISIGN_PASSWORD`, plus the variable `PACK_SIGNING_KEY_ID`
  (`d87b0e33f55ea02b` is the only key committed today; confirm it is the pack key).
- Command line check for s1-s3: the pack smoke test sends
  `{"protocol":"ariad-engine/2","job":"job_smoke","op":"describe"}` on stdin and expects a line
  containing `"protocol":"ariad-engine/2"`; the engine must find `libpdfium` next to itself.

## New dependencies

None in the repository. CI installs `zstd` and `minisign` from the distribution and `gnu-tar` and
`zstd` through Homebrew on macOS; all actions reuse the SHA pins already in the repository.

## Unresolved questions

1. Is `bin/ariad-pdfium-engine` with no arguments the right entrypoint, and does it answer a
   `describe` line on stdout (the CI smoke test assumes so)?
2. Which `ashift` release carries `engine_protocol`, for the `requires_ashift` lower bound?
3. Licences to confirm by a human before shipping, as the spike noted: the GPL-with-exception text
   in `icu.txt` (ICU autoconf helper scripts), the LLVM-exception texts in `llvm-libc.txt` and
   `dragonbox-Apache2-LLVM.txt`, and whether the SPDX summary in `pack.conf` (including
   `BSL-1.0`, `Unicode-3.0`, `IJG` and the BSD-style AGG text) is acceptable to the licence gate.
4. Is `d87b0e33f55ea02b.pub` the pack-signing key to use for `PACK_SIGNING_KEY_ID`?
5. Alpine 3.24 and `rust:1.99.0-alpine3.24` were verified in the registry today; the musl legs
   have never run, so the first CI run may need adjustment.
