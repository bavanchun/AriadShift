# Engine Packs

Heavy engines (Docling, OCR models) are not part of the base `ashift` install. They ship as **engine packs**: a signed manifest plus per-platform component archives, downloaded on demand and verified before anything is unpacked. This page describes the format, the install layout, and how a pack is built, signed and published. Design context: [ARCHITECTURE.md §7.1](../ARCHITECTURE.md#71-engine-packs).

## Using packs

```bash
ashift engines install docling                 # recommended version from the built-in index
ashift engines install docling --version 0.2.0
ashift engines install docling --manifest https://example.org/pack.toml
ashift engines install docling --from ./docling-pack   # offline, no network
ashift engines                                 # engines, plus a section for installed packs
ashift engines verify docling                  # re-hash every file against the install receipt
ashift engines remove docling [--version 0.2.0]
```

Every command accepts `--json`. Command reference: [cli.md](cli.md#ashift-engines-install). `doctor` reports the data directory, the trusted signing keys and the state of each installed pack. Installing stays a CLI and desktop operation; the MCP server only lists packs.

`--from <dir>` reads `pack.toml`, `pack.toml.minisig` and each component file from `<dir>`, each component named after the last segment of its `url`. It never opens a network connection, so a pack can be carried to an air-gapped machine.

## Manifest (`ariad-pack/1`)

The manifest is TOML. Its JSON Schema is [`schemas/pack.v1.json`](../schemas/pack.v1.json), generated from `ariad_host::packs::Manifest` and drift-tested.

```toml
schema = "ariad-pack/1"
name = "docling"
version = "0.2.0"
description = "Docling with OCR"
license = "MIT AND Apache-2.0 AND BSD-3-Clause"
requires_ashift = ">=0.2.0, <2.0.0"
notices = ["notices/THIRD_PARTY_NOTICES.txt"]

[[targets]]
triple = "x86_64-unknown-linux-gnu"
entrypoint = ["runtime/bin/python3", "-I", "-m", "ariad_docling"]
env = { HF_HUB_OFFLINE = "1", TRANSFORMERS_OFFLINE = "1" }
options = { artifacts_path = "{root}/models", tessdata_path = "{root}/tessdata" }

[[targets.components]]
name = "runtime"
url = "docling-runtime-x86_64-unknown-linux-gnu.tar.zst"
sha256 = "<64 lower-case hex digits>"
size = 412345678
unpack = "tar.zst"
dest = "runtime"
unpacked_size = 1234567890
entries = 23456
```

| Field | Meaning |
|---|---|
| `schema` | Always `ariad-pack/1`. |
| `name` | `[a-z][a-z0-9-]*`, at most 64 bytes. |
| `version` | SemVer. |
| `license` | SPDX summary of everything the pack contains. Packs follow the licensing rules in [AGENTS.md](../AGENTS.md): no AGPL, no non-OSI code, no excluded model licences. |
| `requires_ashift` | SemVer requirement on `ashift`. A mismatch is exit code 5. |
| `engine_protocol` | Optional: `ariad-engine/1` or `ariad-engine/2`. Absent means `ariad-engine/1`, so existing manifests (Docling) need no change. Install and resolve refuse a pack whose value differs from the protocol `ashift` registers for the engine of the same name, and any other value; see [Engine protocol](#engine-protocol). |
| `notices` | Licence and notice files inside the install root. Each must exist after extraction. |
| `targets[].triple` | Rust target triple of the pack's own binaries. |
| `targets[].entrypoint` | Engine argv. The first element is a path relative to the install root; later elements may use `{root}`. |
| `targets[].env` | Engine settings added to the cleared engine environment. Names are allow-listed (`ALLOWED_ENV` in `crates/ariad-host/src/packs/manifest.rs`: Hugging Face offline switches, `TESSDATA_PREFIX`, thread counts, Python runtime switches). Values may use `{root}`. |
| `targets[].options` | Default request `options` for the engine; strings may use `{root}`. |
| `components[].url` | `https://` URL, or a path relative to the manifest's location (the usual case: the manifest and its components are assets of the same release). |
| `components[].sha256`, `size` | Of the downloaded file. |
| `components[].unpack` | `tar.zst`, `tar.gz`, `zip`, or `file` (copied to `<dest>/<name>`). |
| `components[].dest` | Directory inside the install root; `.` is the root. |
| `components[].unpacked_size`, `entries` | Required for archives: regular-file bytes (hard-link copies included) and listed entries. Extraction stops at the declared value plus 1 % and 1 MiB (or 16 entries). |
| `components[].executable` | For `unpack = "file"`: install the file with the executable bit. |

`{root}` is replaced with the absolute install root. A manifest from a newer schema fails with a schema error rather than an unknown-field error, and an `ashift` outside `requires_ashift` refuses the pack before parsing the rest.

### Compatibility

The Docling 0.2.0 pack supports `ashift >=0.2.0, <2.0.0`. Compatibility rests
on the frozen `ariad-engine/1` protocol and `ariad-pack/1` manifest schema,
which the 1.x host line keeps. The host checks `requires_ashift` at install
time and validates the manifest schema separately. Installation does not
perform an engine handshake; it compares the manifest's declared
`engine_protocol` with the protocol `ashift` registers for the engine.

A protocol break must ship together with the 2.0 major release. The
`<2.0.0` bound then refuses that host at installation. The manifest's
`engine_protocol` field is checked at install and resolve (below). Before approving
v1.0.0, release conformance must install the actual 0.2.0 pack on a
1.0.0-versioned host and complete a Docling conversion with the shipped
manifest range unchanged.

### Engine protocol

The wire protocol of each pack engine is registered statically in `ashift` (`EngineSpec.protocol`): `docling` speaks `ariad-engine/1`; `pdfium`, `qpdf`, `tesseract`, `sign`, `verapdf`, `ai` and `inpaint` speak `ariad-engine/2`. The manifest parser checks the declared `engine_protocol` against that registration whenever a manifest is parsed: at install, at every resolve of an installed pack, for an `ASHIFT_PACK_<NAME>` development directory, and when `ashift __pack manifest` generates one. A mismatch fails with an error such as `engine_protocol is 'ariad-engine/1' (the default when the field is absent) but the 'pdfium' engine speaks 'ariad-engine/2'`; an unknown value fails with `engine_protocol 'x' is not supported`. At install nothing is extracted. At resolve the engine is reported as `tool_missing` (exit 5) with a reinstall hint.

A host older than the field rejects a manifest that sets it, because manifests are parsed strictly. A pack that sets `engine_protocol` therefore sets `requires_ashift` to the first release that knows the field.

`ashift __pack manifest` accepts `--engine-protocol <protocol>` or reads the field from the `--base` manifest or template; absent means `ariad-engine/1`.

### Targets

A pack triple describes the pack's binaries, not `ashift`'s. Linux hosts accept `<arch>-unknown-linux-gnu` when a glibc loader is present, then `<arch>-unknown-linux-musl`, so the static musl `ashift` release installs glibc packs. macOS and Windows hosts accept their own triple. A manifest without a matching target is exit code 3, and the message lists the triples it offers.

## Signature

`pack.toml.minisig` is a detached [minisign](https://jedisct1.github.io/minisign/) signature in the prehashed format. `ashift` verifies it with `minisign-verify` against the public keys compiled in from [`crates/ariad-host/data/pack-keys/*.pub`](../crates/ariad-host/data/pack-keys/). Several keys may be listed, so a key can be rotated by shipping the new key one release before packs switch to it.

The trusted comment must contain the tokens `pack:<name>` and `version:<version>` matching the manifest. This stops a valid signature of one version from being paired with another version's manifest. The signature is checked before any other manifest field is trusted, and a failure is exit code 1 (`pack signature verification failed: …`).

Test builds (the `test-probe` feature) also trust the public keys listed in `ASHIFT_TEST_PACK_KEYS`; release builds ignore that variable.

## Download and verification

- Remote URLs must be `https://`. Plain `http://` is accepted only for loopback hosts. Credentials in URLs are refused.
- TLS is rustls with the operating system's trust store (`SSL_CERT_FILE` and `SSL_CERT_DIR` are honoured on Linux). `HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY` and `NO_PROXY` are honoured.
- Connections time out after 30 s; a transfer that stalls for 60 s fails. Connection errors, timeouts, transport-truncated bodies, HTTP 408, 429 and 5xx are retried up to three attempts with backoff. TLS trust failures fail immediately with a CA-bundle hint. A complete response or local file whose size differs from the manifest fails with `size_mismatch` without retrying.
- Each component streams to `packs/.cache/<name>.<sha256>.<id>.part` while it is hashed. A body longer than `size` stops at once; a SHA-256 mismatch fails the install (exit code 1). A verified component is renamed to `packs/.cache/<name>.<sha256>`, so a failed install resumes without downloading it again; a successful install deletes the cached files it used. The pack name keeps independent installers from consuming each other's downloads when components have identical hashes.
- Progress is printed to stderr only when stderr is a terminal.

## Extraction rules

Every archive entry must pass these rules, or the install fails with `unsafe archive entry …` (exit code 1) and leaves nothing behind:

- The name is UTF-8 and relative: no leading `/`, no `..`, no backslash, `:` or control characters; on Windows no reserved device names.
- Only regular files, directories, symbolic links and hard links. Device files, FIFOs and sparse entries are refused.
- No entry is written through an existing symbolic link or file, and no entry replaces an earlier one.
- Symbolic links are relative and resolve inside the install root, both lexically and after canonicalizing every link once all components are unpacked. Windows packs cannot contain symbolic links.
- Hard links must point at an earlier regular file of the same component and are written as copies.
- `receipt.toml` at the top of the root is reserved.
- Entry count and unpacked bytes stay within the declared bounds above; exceeding them is exit code 4.
- Completed extraction must match the declared entry count and unpacked size exactly; a short or otherwise inconsistent archive is refused as corrupt.

Zstd archives may span multiple data frames and contain skippable metadata frames; all data frames form one continuous tar stream.

## Layout

The data directory is `ASHIFT_DATA_DIR` when set, otherwise the platform's local data directory for `com.ariadnev.ariadshift`: `~/.local/share/ariadshift` on Linux, `~/Library/Application Support/com.ariadnev.ariadshift` on macOS, `%LOCALAPPDATA%\ariadnev\ariadshift\data` on Windows.

```text
<data>/packs/
  .locks/<name>.lock         install and remove of one pack are exclusive
  .staging/<name>.<id>/      extraction target; renamed into place on success
  .cache/<name>.<sha256>     verified downloads kept for a resumed install
  <name>/current             active version
  <name>/<version>/          install root
  <name>/<version>/receipt.toml
```

An install extracts into `.staging`, checks links, notices and the entrypoint, writes the receipt, then renames the directory into place and marks it active. Reinstalling an identical, intact version re-hashes its files before returning `already_installed`; a damaged one is replaced. Leftovers of interrupted installs are swept under the pack's lock. Several versions may be installed; the newest install is active, and removing the active version activates the highest remaining one.

`receipt.toml` (`ariad-pack-receipt/1`) holds the manifest and signature as installed, the install time, the `ashift` version, the signing key id, and the path, size and SHA-256 of every file plus every link target. `ashift engines verify` re-hashes against it and re-checks the signature with the keys of the running `ashift`.

## Resolution

`ariad_host::packs::resolve(name)` returns the active version's `InstalledPack`: install root, target, program (`entrypoint[0]` under the root), arguments, environment and options with `{root}` substituted. Create `runner::EngineCommand::from_pack(ashift, &pack)` and pass it to `runner::run_command` or `SessionPool::lease`. The command adds the pack's allow-listed environment to the cleared engine environment and grants its install root read and execute access through the Linux sandbox. The explicit environment and root participate in the session-pool key. `runner::run_with_env` and `describe_with_env` remain available for callers supplying their own launch policy. A pack built for another platform or another `ashift` range does not resolve.

`ASHIFT_PACK_<NAME>=<dir>` (name upper-cased, `-` written as `_`) overrides resolution with an unpacked development pack: `<dir>/pack.toml` is read without a signature check and `<dir>` is the root.

Resolution of a sandboxed pack engine (`pdfium` and the other `ariad-engine/2` packs) goes through `ariad_host::engines::resolve_pack(id)`. Besides the checks above, it refuses an entrypoint that leaves the pack root once links are followed (`tool_missing`, exit 5, reinstall hint). A missing, unverifiable or mismatched pack never falls back to another engine: every tool that needs it fails with exit 5 and the pack's install hint.

## Building a pack

`ashift __pack manifest` (hidden) writes an unsigned manifest for built component files. It hashes each file, picks the unpack mode from the extension, and dry-runs the extractor over archives to declare `unpacked_size` and `entries`, so an archive that breaks the extraction rules fails at build time.

```bash
ashift __pack manifest --name docling --version 0.2.0 \
  --target x86_64-unknown-linux-gnu --base pack.base.toml \
  -o dist/pack.toml \
  runtime=dist/docling-0.2.0-x86_64-unknown-linux-gnu-runtime.tar.zst \
  models=dist/docling-0.2.0-models.tar.zst tessdata=dist/docling-0.2.0-tessdata.tar.zst
```

- `--base <file>` supplies the pack fields (`description`, `license`, `requires_ashift`, `notices`) and per-target `entrypoint`, `env` and `options`. Only the `--target` entry's components are replaced; other targets are kept, so per-platform builds merge by passing the previous output as `--base`. The merged manifest is validated, so every target in it needs components; append a new target's entrypoint and options to the previous output before passing it as `--base`.
- `--entrypoint <arg>` (repeat per argv element) sets or overrides the target's entrypoint; without a base it is required.
- `--engine-protocol <protocol>` sets the engine wire protocol (`ariad-engine/1` or `ariad-engine/2`).
- `--url-prefix <url>` prefixes component URLs; by default a URL is the bare file name, relative to the manifest.
- `NAME=FILE` names a component; otherwise the name is the file name without its archive extension.
- Keep each component under 2 GiB (the GitHub release asset limit) and compress zstd archives without `--long` (the decoder's window limit is 128 MiB).

## The docling pack

`engines/docling/pack/build.py` builds the `docling` pack and calls `ashift __pack manifest` as described above; `just pack-docling` builds it for the host. The pack has three components, all unpacked into the install root (`dest = "."`), so each archive holds top-level directories:

| Component | Holds | Per target |
|---|---|---|
| `runtime` | `runtime/` (python-build-standalone CPython 3.14.9 with the locked wheels and the `ariad_docling` engine) and `notices/` (licence files of every bundled distribution, model and data set, plus the AriadShift `LICENSE` and `NOTICE`) | yes |
| `models` | `models/`: the Docling layout (Heron) and TableFormer models and the RapidOCR PP-OCR models, at pinned revisions | shared |
| `tessdata` | `tessdata/`: `tessdata_best` `vie`, `eng`, `osd` and `configs/tsv`, at a pinned commit | shared |

In 1.0, Docling packs ship for macOS arm64 (`aarch64-apple-darwin`), Linux x86_64 (`x86_64-unknown-linux-gnu`) and Linux arm64 (`aarch64-unknown-linux-gnu`). The Windows `x86_64-pc-windows-msvc` Docling pack is planned for v1.x ([#83](https://github.com/bavanchun/AriadShift/issues/83)). On Linux x86_64 the components are about 455 MB, 383 MB and 29 MB (1.9 GB of files in `runtime`). The entrypoint is `runtime/bin/python3 -I -m ariad_docling` (`runtime/python.exe` on Windows) and the default options are `artifacts_path = "{root}/models"` and `tessdata_path = "{root}/tessdata"`. The pack does not bundle the Tesseract program; the engine reports it in `describe` and `ashift doctor` explains how to install it. `requires_ashift` is `>=0.2.0, <2.0.0`, following the [frozen protocol and schema compatibility policy](#compatibility).

For byte-identical builds, run `build.py` with the same uv-managed python-build-standalone CPython 3.14 patch release on both builds, as the Pack CI jobs do through `setup-uv`. The builder uses that interpreter's `tarfile` and bundled zstd; a system Python linked to another zstd version can produce different archive bytes. Keep `SOURCE_DATE_EPOCH` and all pinned inputs identical. The Linux x86_64 Pack job builds again in a separate directory and checks SHA-256 for all three components and `pack.toml`.

The build prunes unused runtime modules and requires every remaining native file outside `site-packages` to match the platform's licence inventory before installing the locked wheels. Unlisted native files and inventory entries with no matching file fail the build. The merged manifest deduplicates licence obligations across targets while retaining nonredundant alternatives and exceptions.

Windows packs use the system Microsoft runtime; see [Docling prerequisites](docling.md#what-it-needs). The build removes `vcruntime*` and `msvcp*` DLLs before and after wheel installation, and the archive writer rejects any remaining copy, including those under `site-packages`. Wheels can refer to hash-suffixed runtime names; their PE import tables are retargeted to the corresponding system DLL names before those copies are removed, without relocating image data.

Docling packs exclude Intel IPP ICV and Intel's OpenMP binaries. On x86_64,
OpenCV is built from the hash-pinned sdist with IPP/ITT and proprietary backends
disabled; Linux uses a pinned manylinux 2.28 build container. Windows replaces
PyTorch's Intel OpenMP with a source-built LLVM runtime and Apache-2.0 MSVC
compatibility wrappers, retaining the DLL name its import table requires.
The stubs DLL is removed and LLVM's licence and wrapper source are included in
the notices. The final gate checks wheel binaries and notice contents as well
as distribution metadata. Build prerequisites, ARM notice handling and real
conversion comparisons are described in [the Docling build](docling.md#the-pack).
Linux and Windows x86_64 also replace the MKL-bearing PyTorch wheels with
source-built OpenBLAS CPU wheels, retaining Apache-2.0 oneDNN. Once a
`toolchain-wheels-v*` release is published by
[`.github/workflows/toolchain-wheels.yml`](../.github/workflows/toolchain-wheels.yml)
and pinned under `[toolchain]` in `engines/docling/pack/pins.toml`, the pack
consumes those hash-verified release wheels and sidecars (falling back to the
pinned source build only while a target's `[toolchain]` entry is omitted during
bootstrap). Official ARM PyTorch wheels remain; BSD-3-Clause ITT is permitted
there and its license is included in the third-party notices, while source-built
toolchain wheels disable ITT (`USE_ITT=0`, `-DWITH_ITT=OFF`,
`-DLIBOMP_USE_ITT_NOTIFY=OFF`). The gate distinguishes ITT/oneDNN from
proprietary MKL, IPP and Intel OpenMP. The owning build and update route is in
the [Docling guide](docling.md#the-pack).

To try a build without signing it, unpack the components into one directory, add `pack.toml`, and point `ASHIFT_PACK_DOCLING` at it; `build.py verify` does exactly that, and with `--offline` runs the conversion in a network namespace. How the inputs are pinned, how the licence gate works and what CI checks: [docling.md](docling.md#the-pack).

## The pdfium pack

The `pdfium` pack provides the PDFium engine (`ariad-engine/2`, stateless per request) for page rendering, thumbnails, images and text with positions. Its pack name is its engine id.

| Item | Value |
|---|---|
| Contents | `ariad-pdfium-engine` binary, pinned `libpdfium` (PDFium `chromium/8125`, from `bblanchon/pdfium-binaries`, SHA-256 pinned per target), and the bundled third-party notices (FreeType, ICU, libjpeg-turbo, OpenJPEG, LCMS, libpng, zlib, HarfBuzz, Abseil, AGG) |
| Targets | `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`. Windows packs are deferred because Windows engines have no filesystem or network sandbox |
| Manifest | `engine_protocol = "ariad-engine/2"`; `requires_ashift` names the release that adds the field |
| Library lookup | The engine loads `libpdfium` from the directory of its own binary. The manifest sets no library-path variable, and `ALLOWED_ENV` has none |
| Sandbox | `sandbox_required`: the pack root is readable and executable but not writable, `/tmp` is private to the job, the job directory is the only writable location and the network is closed. `best-effort` mode refuses to start the engine when isolation is unavailable; only `ASHIFT_SANDBOX=off` runs it unsandboxed |
| Release | Tag `pack-pdfium-v<version>`. Signing happens only in the pack workflow on that tag, under the protected `release` environment; local builds are unsigned |

Install it once a release exists with `ashift engines install pdfium`. Until the embedded index ([`crates/ariad-host/data/packs.toml`](../crates/ariad-host/data/packs.toml)) has a `pdfium` entry, that command reports an unknown pack; install from a release's files with `--manifest <url>` or `--from <dir>`. To use an unpacked build without signing it, point `ASHIFT_PACK_PDFIUM` at a directory holding an unsigned `pack.toml`, the entrypoint it names and `libpdfium` beside it; `ashift doctor` then reports the pack as coming from the development override.

Without the pack, every PDFium tool fails with exit code 5 (`tool_missing`) and the hint `run 'ashift engines install pdfium' to install the pdfium engine pack`. `ashift doctor` lists a `pdfium_engine` check and `ashift engines` a `pdfium` row either way: not installed (with that hint), or installed with the pack version and the protocol `ariad-engine/2`.

## Signing and publishing

1. Create the signing key once with `minisign -G -p ariadshift-packs.pub -s ariadshift-packs.key`. The secret key and its password live only in the `release` environment secrets and an offline backup; never in the repository. Commit the public key as `crates/ariad-host/data/pack-keys/<key-id>.pub` and release an `ashift` that contains it before the first pack signed with it.
2. Build every target and merge the manifests with `ashift __pack manifest --base`.
3. Sign: `minisign -S -s ariadshift-packs.key -m pack.toml -t "pack:docling version:0.2.0"`. Signing keys exist only in the `release` environment; the `pdfium` pack is signed only by its pack workflow on a `pack-pdfium-v*` tag, never on a developer machine.
4. Check locally: `ASHIFT_DATA_DIR=$(mktemp -d) ashift engines install docling --from dist/`.
5. Publish `pack.toml`, `pack.toml.minisig` and the component files as assets of the GitHub release tagged `pack-<name>-v<version>`.
6. Point the built-in index ([`crates/ariad-host/data/packs.toml`](../crates/ariad-host/data/packs.toml)) at the new recommended version. The index's URL template resolves to `https://github.com/bavanchun/AriadShift/releases/download/pack-<name>-v<version>/pack.toml`.

## Exit codes

| Code | Pack failure |
|---|---|
| 1 | Signature, hash, size, download, unsafe archive, corrupt archive, rejected manifest (including a mismatched `engine_protocol`), receipt or I/O failure; another process holds the pack's lock; `verify` found differences |
| 2 | Invalid arguments, a pack name or version that is not well formed, plain HTTP to a non-loopback host |
| 3 | Pack not in the index, pack or version not installed, no build for this platform |
| 4 | An archive unpacks beyond its declared bounds |
| 5 | The pack requires a different `ashift` version |
| 130 | Interrupted; staging and partial downloads are removed |
