# Worker 6-T5 (Opus 5.5): new fuzz targets

Branch `feat/v1-hardening`, worktree `01-AriadShift-wt-v1-hardening`. Nothing was compiled or run (no-build rule). New files were formatted with `rustfmt --edition 2024`.

## Commits

| Hash | Subject |
|---|---|
| `60586a2` | test(fuzz): add targets for archive, package and protocol parsers |
| `b76b149` | ci(fuzz): run the new parser targets in CI and nightly |
| `f57b1c1` | test(fuzz): add the version-checked IR JSON reader target |
| `3b0ae7d` | ci(fuzz): run the ir_json_versioned target |

## Files changed

- New targets in `fuzz/fuzz_targets/`: `archive_preflight.rs`, `docx_meta.rs`, `pack_manifest.rs`, `pack_archive.rs`, `engine_protocol.rs`, `capabilities_json.rs`, `asset_resolve.rs`, `uri_parsers.rs`, `ir_json_versioned.rs`
- `fuzz/Cargo.toml`: 9 `[[bin]]` entries. No new dependencies and no feature flags, so `fuzz/Cargo.lock` should not change.
- Seeds go in `fuzz/seeds/<target>/`. The repo's corpus seeds live in `fuzz/seeds/`; `fuzz/corpus/` is the CI cache.
- `.github/workflows/ci.yml`: the Fuzz job's `targets=` list gains all 9 targets.
- `.github/workflows/fuzz-nightly.yml`: the `targets=` list gains `archive_preflight docx_meta pack_archive`, the decompression and ZIP/XML parsers.
- `docs/ci.md`: the Fuzz job's target list.

## Inventory of parsers of untrusted input

"Covered by" names the fuzz target. Existing targets are marked (existing).

| Parser | Location | Input source | Covered by |
|---|---|---|---|
| Markdown reader | `ariad-core/src/reader/markdown.rs` | user document | `markdown_reader` (existing) |
| YAML front matter | `ariad-core/src/reader/front_matter.rs` | user document | `front_matter` (existing) |
| HTML reader and sink | `ariad-core/src/reader/html*.rs` | user document | `html_reader` (existing) |
| Pandoc JSON AST to IR | `ariad-core/src/pandoc/{ast,to_ir}.rs`, `ariad-host::ir_io::read_json` | Pandoc output from a user document | `pandoc_ast_to_ir` (existing) |
| Docling JSON to IR | `ariad-core/src/docling/` | Docling engine output | `docling_to_ir` (existing) |
| IR JSON bounded reader | `ariad-host::ir_io::read`, `ariad-core::json_depth` | user IR / engine artifact | `ir_json` (existing) |
| IR JSON version-checked reader | `ariad-host::ir_io::read_versioned` (used by `convert.rs:867`) | user IR JSON input | **`ir_json_versioned` (new)** |
| Limits validation | `ariad-core::limits` | API/MCP/cloud request | `limits_validate` (existing) |
| PDF text reader (lexer, objects, cmap, fonts) | `ariad-core/src/reader/pdf/` | user document | `pdf_reader` (existing) |
| Typst writer | `ariad-host::typst_render` | IR from user document | `typst_writer` (existing) |
| ZIP EOCD/Zip64 + central directory + streaming inflate preflight | `ariad-host::archive::{read_eocd_info, parse_central_directory, preflight_archive}` | DOCX/EPUB/Office input | **`archive_preflight` (new)** |
| ZIP entry listing, OLE2/CFB directory walk, content sniffing | `archive::{read_zip_entry_names, read_cfb_entry_names}`, `ariad-core::format::classify`, `convert::classify_file` | any input file (`inspect`, detection) | **`archive_preflight` (new)** |
| DOCX/EPUB metadata stamping: ZIP rewrite + `docProps/core.xml` / OPF quick-xml rewriters ("docx_meta" in the brief; no module of that name exists, the code is `package_meta.rs`) | `ariad-host::package_meta::{stamp_docx, stamp_epub}` | Pandoc output package | **`docx_meta` (new)** |
| Pack manifest `ariad-pack/1` TOML (header pre-parse, strict parse, validate, SemVer) | `ariad-host::packs::Manifest::parse` | network download, unsigned bytes are parsed before verify | **`pack_manifest` (new)** |
| Minisign key and signature parsing, trusted comment | `packs::{TrustedKey::parse, TrustedKeys::verify}` | network download | **`pack_manifest` (new)** |
| Pack component archives: zstd frame joiner, gzip, tar, ZIP, entry and link rules | `packs::scan` (shares `walk` with `extract`) | network download (hash-checked) / author input | **`pack_archive` (new)** |
| Engine protocol request line | `ariad-host::engines::common::read_request` | host → engine stdin | **`engine_protocol` (new)** |
| Engine protocol event lines (`Event`, `Message` serde) | `ariad-core::protocol` | engine stdout, possibly a compromised engine | **`engine_protocol` (new)**, serde layer only; see visibility note |
| Capabilities registry JSON + planner | `ariad-core::planner::{Capabilities, validate, plan, reachable_formats}` | embedded file; `ASHIFT_TEST_CAPABILITIES` in test-probe builds | **`capabilities_json` (new)** |
| Local image embedding: percent-decoding, lexical path checks, cap-std confinement, image sniffing | `ariad-host::assets::resolve` | hrefs in user Markdown/HTML | **`asset_resolve` (new)** |
| `file://` URIs and Windows path syntax | `ariad-host::confine::{parse_file_uri, validate_windows_path_syntax}` | MCP client arguments | **`uri_parsers` (new)** |
| Link scheme filter | `ariad-core::links::allowed_link` | links in user documents | **`uri_parsers` (new)** |
| Pandoc extracted-media ingestion | `ariad-host::media::ingest_media` (`pub(crate) mod media`) | files Pandoc extracted from a user DOCX/EPUB | Not covered. Needs a visibility change (below); its helpers `percent_decode`, `lexical_path` and `read_image` are reached through `asset_resolve` |
| Host-side event rules (artifact confinement, result consistency) | `ariad-host::runner::parse_event` (private) | engine stdout | Not covered. Needs a visibility change (below) |
| Pack extraction writer sink (writes files, links) | `packs::extract::extract` (`pub(crate)`) | network download | Not covered directly: `pack_archive` covers the shared walker and rules; the write sink needs a real install root |
| Pipeline dispatch, `inspect_bytes` | `ariad-core::{pipeline, inspect}` | user document | Routes to the readers above; no separate target |
| Pandoc and LibreOffice version strings | `pandoc_bin::parse_version_line`, `engines::libreoffice::parse_version` | local tool output (trusted) | No target |
| Sandbox argv, watchdog procfs reads | `ariad-host::{sandbox, watchdog}` | host/OS (trusted) | No target |
| Pack index | `packs::index` (`data/packs.toml`, compiled in) | trusted | No target |

## What each new target asserts

Most targets assert more than "does not panic". If a target fails in its first run, check whether the failing assertion is one of these stricter properties before treating it as a crash:

- `archive_preflight`: preflight is monotonic (small limits Ok ⇒ local limits Ok). Typed errors match their limits. Names that pass preflight are relative, have no `:` or `..`, and are unique. `read_zip_entry_names` matches the central directory. The CFB listing is at most 2048 names. `classify_file` reports the byte size.
- `docx_meta`: inputs starting with `PK\x03\x04` are stamped as whole packages. Any other input is stored as both `docProps/core.xml` and `EPUB/package.opf` in a stored ZIP built in-target with a hand-written CRC32, so the XML rewriters are reachable. A failed stamp leaves the file byte-identical. Neither outcome leaves temporary files behind. A successful stamp keeps the entry names, and a stamped package can be stamped again.
- `pack_manifest`: input layout is `toml [NUL minisig [NUL pubkey]]`. A parsed manifest re-validates, and all its paths are safe relative paths. Env keys are allow-listed. Every triple is selectable. `to_toml` → `parse` gives back an equal manifest. `verify` with compiled keys, an empty key set and the fuzz-supplied key returns only typed errors, and any Ok names a trusted key.
- `pack_archive`: `Unpack::File` always scans as 1 entry of `data.len()` bytes. ZIP entries ≤ `len/46`. The tar.gz and tar.zst paths must not panic. Input is capped at 16 KiB to bound zstd RLE bombs.
- `engine_protocol`: `read_request` agrees with a direct decode of the first line. Request, Event and Message round-trip through JSON and encode to a single line.
- `capabilities_json`: invalid registries are refused by `plan`. Valid ones round-trip. Planning covers every format pair and profile, with all engines and with one engine removed, on registries of up to 48 edges. Routes chain from→to over real edges with available engines, scores stay in [0,1], alternatives are ≤2 and distinct, and `plan` Ok/NoRoute agrees with `reachable_formats`.
- `asset_resolve`: a per-process tree holds `secret.png` next to `base/`, plus images, a large image, a text file, and (Unix) absolute, relative and directory symlinks out of `base/`. No embedded asset equals the secret. Every asset is a PNG within `max_asset_bytes`, and every `AssetRef::Asset` exists.
- `uri_parsers`: a `file://` URI parses only to an absolute path with no NUL. An allowed link is an anchor or an http/https/mailto URL with no surrounding whitespace.
- `ir_json_versioned`: an Ok result has `version == "ariad-ir/0"`, stays within the byte, depth and block limits, and equals the plain `ir_io::read` result.

The filesystem targets (`archive_preflight`, `docx_meta`, `pack_archive`, `asset_resolve`) write under `std::env::temp_dir()/ariad-fuzz-*-<pid>`. Locally that needs `TMPDIR` on the SSD.

## Seeds (licensing)

- Copied recorded fixtures: `fixtures/epub/en-epub-minimal.epub` (MIT), `fixtures/office/vi-garden-notes.{odt,doc}`, `fixtures/office/vi-route-sheet.{xlsx,xls}` (Apache-2.0).
- Derived from those fixtures: `docx_meta` core.xml and OPF variants with created/modified/date added, and stampable `.xlsx`/`.epub` rebuilds.
- Copied from existing project seeds: `ir_json_versioned` (copies of `fuzz/seeds/ir_json`), `capabilities_json/embedded-capabilities.json` (`crates/ariad-core/data/capabilities.json`).
- Project-authored: pack manifests and signatures (zero Ed25519 signature under the compiled key id `D87B0E33F55EA02B`, plus the repo's public key), tar.gz/tar.zst/zip pack components, protocol lines, Markdown with escaping hrefs, URI strings.
- No DOCX fixture was used because the recorded ones are 25–830 KB.
- `fuzz/seeds/**` is excluded from `typos`.
- One seed, `engine_protocol/convert-events.jsonl`, was written with CRLF. `.gitattributes` normalised it to LF on commit.

## Verify (coordinator)

```sh
source /Volumes/AriadWork/02-Ariadnev-Eco/.cache/env.sh
cd /Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift-wt-v1-hardening
cargo fuzz build -s none   # builds all bins; expect fuzz/Cargo.lock unchanged
for t in archive_preflight docx_meta pack_manifest pack_archive engine_protocol capabilities_json asset_resolve uri_parsers ir_json_versioned; do just fuzz "$t" 60; done
actionlint .github/workflows/ci.yml .github/workflows/fuzz-nightly.yml
```

`just fuzz` is Linux-only. On macOS, run `cargo fuzz run -s none <t> fuzz/corpus/<t> fuzz/seeds/<t> -- -max_total_time=60 -timeout=10 -rss_limit_mb=2048` directly.

## Open concerns (outside my files)

1. **Nightly timeout**: `fuzz-nightly.yml` now runs 9 targets × 600 s = 90 min against `timeout-minutes: 90`, so it will time out. Change `timeout-minutes: 90` to `150` (or split the targets into a matrix). This is required, not optional.
2. **CI Fuzz job timeout**: 18 targets × 60 s plus the build and link of 9 more bins may exceed `timeout-minutes: 35` in `ci.yml`. Suggest `50`.
3. **`justfile` `fuzz` recipe** (not my file): its default `targets=(…)` list should add `"archive_preflight" "docx_meta" "pack_manifest" "pack_archive" "engine_protocol" "capabilities_json" "asset_resolve" "uri_parsers" "ir_json_versioned"`. None of them need `build_options`. A single target (`just fuzz archive_preflight 60`) already works without this change.
4. **Visibility changes for the two uncovered parsers** (not made; core files are outside my ownership). Add a `fuzzing = []` feature to `crates/ariad-host/Cargo.toml` and enable it from `fuzz/Cargo.toml` (`ariad-host = { …, features = ["fuzzing"] }`):
   - `crates/ariad-host/src/runner.rs`: keep `parse_event` private and add a gated wrapper: `#[cfg(feature = "fuzzing")] #[doc(hidden)] pub fn fuzz_parse_event(line: &[u8], output_dir: Option<&str>) -> Result<ariad_core::protocol::Event, RunError> { parse_event(line, output_dir, &mut Vec::new()) }`.
   - `crates/ariad-host/src/lib.rs`: add `#[cfg(feature = "fuzzing")] #[doc(hidden)] pub use media::ingest_media;`.
   Then `engine_protocol` can assert artifact confinement, and a `media_ingest` target can follow the `asset_resolve` design.
5. Not compiled. The likeliest compile risks are small type inferences: `stored_zip(&[(&str, &[u8])])` literals and closure `Copy` in `docx_meta`, and `strip_suffix(b"\r")` in `engine_protocol`. All follow standard patterns.
6. `fuzz/src/lib.rs` (not mine) could hold a shared per-process scratch-dir helper. Four targets each carry a 6-line copy.

Status: DONE_WITH_CONCERNS
