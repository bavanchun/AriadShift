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
requires_ashift = ">=0.2.0, <0.3.0"
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

On Linux x86_64 the components are about 455 MB, 383 MB and 29 MB (1.9 GB of files in `runtime`). The entrypoint is `runtime/bin/python3 -I -m ariad_docling` (`runtime/python.exe` on Windows) and the default options are `artifacts_path = "{root}/models"` and `tessdata_path = "{root}/tessdata"`. The pack does not bundle the Tesseract program; the engine reports it in `describe` and `ashift doctor` explains how to install it. `requires_ashift` is `>=0.2.0, <0.3.0`, the line whose engine protocol the pack speaks.

For byte-identical builds, run `build.py` with the same uv-managed python-build-standalone CPython 3.14 patch release on both builds, as the Pack CI jobs do through `setup-uv`. The builder uses that interpreter's `tarfile` and bundled zstd; a system Python linked to another zstd version can produce different archive bytes. Keep `SOURCE_DATE_EPOCH` and all pinned inputs identical. The Linux x86_64 Pack job builds again in a separate directory and checks SHA-256 for all three components and `pack.toml`.

The build prunes unused runtime modules and requires every remaining native file outside `site-packages` to match the platform's licence inventory before installing the locked wheels. Unlisted native files and inventory entries with no matching file fail the build. The merged manifest deduplicates licence obligations across targets while retaining nonredundant alternatives and exceptions.

Windows packs use the system Microsoft runtime; see [Docling prerequisites](docling.md#what-it-needs). The build removes `vcruntime*` and `msvcp*` DLLs before and after wheel installation, and the archive writer rejects any remaining copy, including those under `site-packages`. Wheels can refer to hash-suffixed runtime names; their PE import tables are retargeted to the corresponding system DLL names before those copies are removed, without relocating image data.

To try a build without signing it, unpack the components into one directory, add `pack.toml`, and point `ASHIFT_PACK_DOCLING` at it; `build.py verify` does exactly that, and with `--offline` runs the conversion in a network namespace. How the inputs are pinned, how the licence gate works and what CI checks: [docling.md](docling.md#the-pack).

## Signing and publishing

1. Create the signing key once with `minisign -G -p ariadshift-packs.pub -s ariadshift-packs.key`. The secret key and its password live only in the `release` environment secrets and an offline backup; never in the repository. Commit the public key as `crates/ariad-host/data/pack-keys/<key-id>.pub` and release an `ashift` that contains it before the first pack signed with it.
2. Build every target and merge the manifests with `ashift __pack manifest --base`.
3. Sign: `minisign -S -s ariadshift-packs.key -m pack.toml -t "pack:docling version:0.2.0"`.
4. Check locally: `ASHIFT_DATA_DIR=$(mktemp -d) ashift engines install docling --from dist/`.
5. Publish `pack.toml`, `pack.toml.minisig` and the component files as assets of the GitHub release tagged `pack-<name>-v<version>`.
6. Point the built-in index ([`crates/ariad-host/data/packs.toml`](../crates/ariad-host/data/packs.toml)) at the new recommended version. The index's URL template resolves to `https://github.com/bavanchun/AriadShift/releases/download/pack-<name>-v<version>/pack.toml`.

## Exit codes

| Code | Pack failure |
|---|---|
| 1 | Signature, hash, size, download, unsafe archive, corrupt archive, receipt or I/O failure; another process holds the pack's lock; `verify` found differences |
| 2 | Invalid arguments, a pack name or version that is not well formed, plain HTTP to a non-loopback host |
| 3 | Pack not in the index, pack or version not installed, no build for this platform |
| 4 | An archive unpacks beyond its declared bounds |
| 5 | The pack requires a different `ashift` version |
| 130 | Interrupted; staging and partial downloads are removed |
