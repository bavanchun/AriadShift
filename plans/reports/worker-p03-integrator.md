status: complete

# Phase-3 Review-Fix Integration & Gate Report (`p03-int`)

- **Branch**: `feat/pdf-structural-tools`
- **Worktree**: `/home/vchun/Codes/01-AriadShift-wt-p03`
- **Scope**: Merge slices `p03/g1`..`p03/g4`, apply shared-file wiring edits, refresh intentionally changed snapshots, run the full Phase-3 Linux offline validation gate, and lint commits.

## 1. Slice Merge Summary (`git merge --no-ff`)

All four fix-verification slice branches were merged onto `feat/pdf-structural-tools` with `git merge --no-ff`, verified via `git cherry feat/pdf-structural-tools <branch>` (no unmerged `+` commits), and deleted with `git branch -d`:

| Slice | Source Tip | Code Commit(s) | Merge Commit | `git cherry` | Branch Deleted |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `p03/g1` | `a7339c68` | `9e4ee918` (`fix(pdf): harden page-set copier for acroform,annots,dests,labels`) | `7a9529fd` (`chore(pdf): merge p03/g1 page-set engine fixes`) | 0 `+` commits | Yes (`git branch -d p03/g1`) |
| `p03/g2` | `7557586c` | `6e3031ec` (`fix(pdf): preserve catalog entries during in-place reorder`) | `b349ae67` (`chore(pdf): merge p03/g2 reorder and split fixes`) | 0 `+` commits | Yes (`git branch -d p03/g2`) |
| `p03/g3` | `102549da` | `102549da` (`fix(pdf): harden layers order filtering and outline limits`) | `35aae5a3` (`chore(pdf): merge p03/g3 layers and outline hardening`) | 0 `+` commits | Yes (`git branch -d p03/g3`) |
| `p03/g4` | `3398a413` | `bd0f6016`, `acd745b0`, `463d4bc3` (`decompress` real `ObjStm`/`XRef`, `xobject` resources/filters/BBox, attachment quarantine helper) | `0d22f2d0` (`chore(pdf): merge p03/g4 decompress, quarantine and xobject fixes`) | 0 `+` commits | Yes (`git branch -d p03/g4`) |
| `p03/r3` | `5fdc3f50` | `9737d57a`, `21151826`, `5fdc3f50` (disk-image/help/app-package quarantine extensions, truthful skipped-page layer warning) | `20fd2a37` (`chore(pdf): merge p03/r3 quarantine and layer warning fixes`) | 0 `+` commits | Yes (`git branch -d p03/r3`) |
| `p03/r1` | `cb4dee4d` | `cb4dee4d` (`fix(pdf): bound named-destination copy and tree`) | `a88035a5` (`chore(pdf): merge p03/r1 named-destination copy bounds`) | 0 `+` commits | Yes (`git branch -d p03/r1`) |
| `p03/r2` | `cbf5a6db` | `cbf5a6db` (`fix(pdf): cap split output pages and index reorder widget fields`) | `90c8f936` (`chore(pdf): merge p03/r2 split cap and reorder widget fixes`) | 0 `+` commits | Yes (`git branch -d p03/r2`) |

## 2. Shared-File Edits Applied

1. **`crates/ariad-pdf/src/document.rs`** (from `g1` & `g2` reports):
   - Added `pub max_pages: Option<u32>` and `pub fn with_max_pages(mut self, max_pages: Option<u32>) -> Self` on `ToolCtx`.
2. **`crates/ariad-host/src/tools/inprocess.rs`** (from `g1` & `g2` reports):
   - Populated `max_pages: limits.max_pages` in `build_tool_ctx`.
3. **`crates/ariad-pdf/src/tools/mod.rs`** (from `g1`, `g2`, & `g3` reports):
   - Derived `local_ctx` with `max_pages: inputs.iter().filter_map(|i| i.limits.max_pages).chain(ctx.max_pages).min()` in both `dispatch_structural` and `dispatch_structural_inputs`.
   - Switched `"pdf.outline"` dispatch from `dispatch_structural(..., outline::run)` to `dispatch_structural_inputs(..., outline::run_inputs)` so caller `InputDoc.limits` reach `outline` heading detection.
4. **`crates/ariad-pdf/src/tools/page_set.rs` & `crates/ariad-pdf/src/tools/extract_pages.rs`** (from `g1` & `g2` reports):
   - Enforced `output_page_cap(ctx.max_pages)` (`min(10_000, max_pages)`) in `build_page_set_doc_with_warnings` and `extract_pages::run`.
   - Marked `expand_page_range_bounded` `#[cfg(test)]` and removed dead wrappers `build_extracted_doc` and `build_page_set_doc`.
5. **`crates/ariad-pdf/src/tools/reorder.rs`** (from `g2` report):
   - Imported `TAGGED_STRUCTURE_WARNING` and `output_page_cap` from `page_set`; wired `ctx.max_pages` into `expand_reorder_sequence` and `output_page_cap`.
6. **`crates/ariad-pdf/src/tools/split.rs` & `crates/ariad-pdf/tests/split.rs`** (from `g1` & `g2` reports):
   - Wired `ctx.max_pages` into `ensure_total_pages_within_cap` and `expand_page_range_capped`.
   - Switched part construction to `page_set::build_page_set_output` and propagated `warnings` through `PartSink` so splitting a tagged PDF emits `TAGGED_STRUCTURE_WARNING`.
   - Added integration test `output_caps::caller_max_pages_limit_and_tagged_structure_warning_are_enforced`.
7. **`crates/ariad-pdf/src/tools/{merge,alternate_mix,insert_blank,nup,booklet,divide_pages,posterize}.rs`** (Q3 page-amplifying tools):
   - Replaced hardcoded `MAX_OUTPUT_PAGES` comparisons with `page_set::output_page_cap(ctx.max_pages)` (`min(10_000, max_pages)`).
8. **`crates/ariad-host/src/tools/run.rs`, `crates/ariad-host/tests/tool_publish_atomic.rs`, & `docs/tools.md`** (from `g4` report):
   - Returned `(Vec<PublishedArtifact>, Vec<String>)` from `publish_tool_outputs` using `extracted_quarantine_warnings(&staged)` and appended `ConvertWarning { code: "attachment_quarantined", message }` to `ToolOutcome.warnings`.
   - Updated `run_tool_attachments_extract_quarantines_risky_types_without_failing` to assert the 4 quarantine warnings and documented `.bin` quarantine behavior in `docs/tools.md`.
9. **Snapshots & Spellcheck Cleanup**:
   - Refreshed `crates/ariad-cli/tests/snapshots/tools_structural__structural_tools_golden_summary.snap` for the 4 intentionally changed tool outputs (`pdf.merge`, `pdf.overlay`, `pdf.attachments`, `pdf.portfolio` from F1 `2544e7d` input filename/raw-byte preservation and F3 `c92bea03` rotated-base overlay normalization).
   - Updated `crates/ariad-pdf/tests/{layers,reorder,metadata}.rs` and slice reports `worker-p03-{g2,g3}-sonnet.md` so `just spell` (`typos`) passes cleanly.
   - Note on `pdf.decompress` `keep`: Slice `g4` implemented real `/ObjStm` object streams and `/XRef` cross-reference streams via `lopdf` 0.46 `SaveOptions`, so `keep` was retained in `DecompressObjectsMode` and no schema/docs removal was needed.
10. **`docs/tools.md`** (from `r2` report):
    - Updated the `max_pages` row in §7 to state that output-producing tools enforce `min(10_000, max_pages)` and that in `pdf.split` both per-part output pages and cumulative output pages across parts are capped at `min(10_000, max_pages)` (never the input page count), while serialized part bytes are charged against `max_decompressed_bytes`.

## 3. Phase-3 Gate Results

All gate commands were executed through `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh`:

### Round 2 Narrowed Gate (`int2`)

| Gate Command | Result | Test / Check Counts |
| :--- | :--- | :--- |
| `cargo fmt --all --check` | **PASS** | 0 formatting diffs |
| `cargo clippy --locked -p ariad-pdf -p ariad-host -p ariad-cli --all-targets --features ariad-host/test-probe -- -D warnings` | **PASS** | 0 warnings |
| `cargo test --locked -p ariad-pdf` | **PASS** | **176 passed, 0 failed** (25 unit + 151 integration across 27 suites) |
| `ASHIFT_PANDOC=... cargo test --locked -p ariad-host --features test-probe --lib --test tool_publish_atomic` | **PASS** | **183 passed, 0 failed** (166 unit + 17 integration in `tool_publish_atomic`) |
| `ASHIFT_PANDOC=... cargo test --locked -p ariad-cli --features ariad-host/test-probe --test tools_structural` | **PASS** | **7 passed, 0 failed** |
| `env UV_NO_CONFIG=1 just spell` | **PASS** | 0 typos |
| `sh scripts/check-commits.sh origin/dev..HEAD` | **PASS** | All commits in `origin/dev..HEAD` conform to `committed.toml` |

### Round 1 Full Gate (`int`)

| Gate Command | Result | Test / Check Counts |
| :--- | :--- | :--- |
| `cargo fmt --all --check` | **PASS** | 0 formatting diffs |
| `cargo clippy --locked -p ariad-core -p ariad-pdf -p ariad-host -p ariad-cli -p ariad-wasm --all-targets -- -D warnings` | **PASS** | 0 warnings |
| `cargo clippy --locked -p ariad-host -p ariad-cli --features ariad-host/test-probe --all-targets -- -D warnings` | **PASS** | 0 warnings |
| `cargo test --locked -p ariad-core -p ariad-pdf -p ariad-engine-io` | **PASS** | **681 passed, 0 failed, 4 ignored** (`ariad-core`: 507 passed, 4 ignored; `ariad-engine-io`: 5 passed; `ariad-pdf`: 169 passed across 23 unit + 27 integration suites) |
| `ASHIFT_PANDOC=... cargo test --locked -p ariad-host --features test-probe` | **PASS** | **393 passed, 0 failed** (166 unit + 227 integration across 21 binaries) |
| `ASHIFT_PANDOC=... cargo test --locked -p ariad-cli --features ariad-host/test-probe --bins --test tools_structural --test tools_cli --test engine_conformance_v2 --test mcp_stdio --test fixture_manifest --test fuzz_seed_manifest` | **PASS** | **65 passed, 0 failed** (`ashift` bins: 2, `engine_conformance_v2`: 5, `fixture_manifest`: 2, `fuzz_seed_manifest`: 1, `mcp_stdio`: 43, `tools_cli`: 5, `tools_structural`: 7) |
| `cargo test --locked -p ariad-wasm` | **PASS** | **14 passed, 0 failed** |
| `cargo check --locked -p ariad-pdf -p ariad-wasm --target wasm32-unknown-unknown` | **PASS** | 0 errors/warnings |
| `cargo check --locked --manifest-path fuzz/Cargo.toml` | **PASS** | All fuzz targets checked cleanly |
| `env UV_NO_CONFIG=1 just spell` | **PASS** | 0 typos |
| `cargo deny --locked check licenses advisories bans sources` | **PASS** | `advisories ok, bans ok, licenses ok, sources ok` |
| `bash scripts/test-ci-scope.sh` | **PASS** | **82 passed, 0 failed** |
| `cargo publish --dry-run --locked -p ariad-engine-io` | **EXPECTED (publish order)** | Packaged 6 files (70.5 KiB); tarball verify resolves `ariad-core v0.2.0` from crates.io which predates 0.3.0 `ariad_core::protocol::v2` |
| `cargo publish --dry-run --locked -p ariad-pdf` | **EXPECTED (publish order)** | Packaged 94 files (1.0 MiB); tarball verify resolves `ariad-core v0.2.0` from crates.io which predates 0.3.0 `ariad_core::tools` |
| `sh scripts/check-commits.sh origin/dev..HEAD` | **PASS** | All commits in `origin/dev..HEAD` conform to `committed.toml` |

## 4. Red / Failing Items

- **None.** All unit, integration, snapshot, clippy, formatting, spellcheck, license/advisory, CI-scope, WASM, and fuzz checks are green.

## 5. One-Line Notes for Delta Review

- **N1 (destination cycle / recursion removal)**: `page_set::remap_catalog_dest_object` (`9e4ee918`) and `reorder::dest_targets_any` (`6e3031ec`) resolve `/D` one level deep via bounded `resolve_object` (<=16 reference hops) with no recursive calls, preventing stack overflow on self-referential `/D` dictionaries and ping-pong reference cycles across `/Dests`, `/Names /Dests`, and `/OpenAction`.
- **N2 (`layers` shared `/Order` DAG)**: `layers.rs` (`102549da`) filters each shared indirect `/Order` array once via `OrderFilter` with an `in_progress` cycle guard, a depth cap of 64, and `ctx.check()` per node and every 256 items, preserving linear output size on `[x x]` DAGs.
- **N3 (catalog preservation & dangling references)**: `reorder.rs` (`6e3031ec`) mutates the `lopdf::Document` in place when all pages are retained so all `/Catalog` keys (`/OCProperties`, `/StructTreeRoot`, `/MarkInfo`, `/PageMode`, etc.) survive untouched, while subset reorders and `page_set.rs` (`9e4ee918`) preserve `/PageMode` and `/OCProperties`, drop `/StructTreeRoot` and `/MarkInfo /Marked` with `TAGGED_STRUCTURE_WARNING`, and rewrite dangling references to `Object::Null`.
- **N4 (page-range expansion cap before allocation)**: `page_set.rs` (`9e4ee918`) and `mod.rs`/`inprocess.rs` (`67108008`) estimate output page count from typed `PageRange::tokens()` and `PageRef::resolve` (covering `all`, `even`, `odd`, `last`, `last-K`, `N-`, `-N`) and reject selections exceeding `min(10_000, max_pages)` before allocating.
- **N5 (`decompress` `keep` mode)**: `decompress.rs` (`3398a413`) writes real `/Type /ObjStm` object streams and a `/Type /XRef` stream via `lopdf::SaveOptions` when `objects = "keep"` and removes the raw byte-level `ObjStm` marker patch.
- **N6 (`outline` export named-destination lookup)**: `outline.rs` (`102549da`) builds the `/Catalog /Dests` and `/Catalog /Names /Dests` lookup table once before walking the outline tree instead of rescanning the name tree per bookmark.
- **N7 (`split` cumulative output page cap & decode budget)**: `split.rs` (`6e3031ec`, `cbf5a6db`) enforces `min(10_000, max_pages)` on each part's output pages and on cumulative output pages across all parts (never capping input page count) and charges serialized part bytes against the per-document `DecodeBudget`.
- **N8 (extracted attachment `.bin` quarantine)**: `publish.rs` (`463d4bc3`, `9737d57a`) and `run.rs` (`6a9eeef6`) append `.bin` case-insensitively (stripping trailing dots/spaces and catching double extensions) to risky executable, script, shortcut, disk-image, help, and installer extensions without setting executable bits, emitting one `attachment_quarantined` warning per renamed file.
- **N9 (`layers remove` undecodable page handling & truthful warning)**: `layers.rs` (`102549da`, `21151826`) skips pages whose stream filters or content operators cannot be decoded and emits `"page <N>: content could not be decoded and was skipped, so layer content there may remain"`.
- **N10 (`outline` `from_headings` caller limits)**: `outline.rs` (`102549da`) and `tools/mod.rs` (`67108008`) pass `InputDoc.limits` into `ariad_core::reader::pdf::extract_pages_with_limits` via `dispatch_structural_inputs`.
- **Low items (Round 1)**: `xobject::DecodeBudget` made `pub` with `from_limits` (`acd745b0`); `nup.rs` maps outline destinations to `/Fit` on the composite sheet (`bd0f6016`); wall-clock `<200ms` test assertions replaced with structural count assertions (`9e4ee918`, `6e3031ec`); `outline.rs` warns when outline export/import truncates at depth 64 or 100,000 items (`102549da`).
- **D1 (`collect_named_destinations` bounded copy & shared-array deduplication)**: `page_set.rs` (`cb4dee4d`) caps copied destination arrays at 6 elements (ISO 32000 `/FitR` max), tracks visited `/Names` and `/Kids` objects by `ObjectId` across reference chains, caps total named destinations at 100,000 (`PdfError::LimitExceeded(ReadError::TooManyObjects { limit: 100_000 })`), and polls `ctx.check()` in every collection loop.
- **M1 (`split` input > 10,000 pages allowed when output stays within caps)**: `split.rs` (`cbf5a6db`) checks `min(10_000, max_pages)` only against per-part output pages and cumulative output pages across parts in all modes (`ranges`, `every`, `bookmarks`, `blank_separator`, `size`), never against input `page_count`; documented in `docs/tools.md`.
- **M2 (`reorder` widget cloning `HashSet`)**: `reorder.rs` (`cbf5a6db`) indexes top-level `/AcroForm /Fields` references in a `HashSet<ObjectId>` once before registering cloned widgets instead of rescanning `/Fields` per widget.
- **Low items (Round 2)**: `publish.rs` (`9737d57a`) adds `iso`, `img`, `vhd`, `vhdx`, `chm`, `msix`, `msixbundle`, `appx`, and `appxbundle` to `RISKY_EXTRACTED_EXTENSIONS`; `layers.rs` (`21151826`) clarifies the skipped-page warning so it states that undecodable pages were skipped and layer content there may remain.

## 6. Follow-ups

- Web-path `.bin` renaming for extracted attachments (phase 14).
- Deduplicating or renaming duplicated widget `/T` field names when pages with form widgets are duplicated.
- Handling out-of-process engine JSON output exceeding 256 KiB.
- Reducing `pdf.merge` page-tree / object copying from $O(N^2)$ to linear time.
- Adding `#[non_exhaustive]` or builder-only construction for the public `ToolCtx::max_pages` field (allowed in 0.x minor).
