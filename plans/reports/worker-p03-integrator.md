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

## 3. Phase-3 Gate Results

All gate commands were executed through `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh`:

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

- **N1 (AcroForm field hierarchy & page-set parity)**: `page_set.rs` now copies ancestor `/FT`/`/DA`/`/Q`/`/Opt`/`/V`/`/DV`/`/Ff`/`/MaxLen` down to cloned widget leaves and prunes dangling `/OpenAction`, `/Dests`, and `/Names /Dests` entries across `extract_pages`, `delete_pages`, and `split`.
- **N2 (`reorder` catalog preservation)**: `reorder.rs` mutates the `lopdf::Document` in place so full-page reorders (`reverse`, `duplex_interleave`, `odd_then_even`) retain all catalog keys (`/OCProperties`, `/StructTreeRoot`, `/MarkInfo`, `/PageMode`, etc.), while subset reorders prune dropped-page references and emit `TAGGED_STRUCTURE_WARNING`.
- **N3 (`layers` shared `/Order` DAG & undecodable streams)**: `layers.rs` deduplicates visited container IDs per root while cloning shared `/Order` sub-arrays before in-place filtering, and skips pages with undecodable content streams with a warning instead of failing `layers remove`.
- **N4 (`outline` cyclic `/Names /Dests` & host limits)**: `outline.rs` traverses `/Names /Dests` iteratively with a 10,000-entry budget, caps import trees at depth 64 and 10,000 nodes, and receives caller `InputDoc.limits` via `dispatch_structural_inputs`.
- **N5 (`decompress` `keep` mode)**: `decompress.rs` builds real `/Type /ObjStm` streams and a `/Type /XRef` stream via `lopdf::SaveOptions` when `objects = "keep"`, and expands existing object streams into classic xref tables when `objects = "expand"`.
- **N6 (`xobject` filter chains, `/Resources`, and `/BBox`)**: `xobject.rs` decodes multi-stage content stream filter chains (Flate, LZW, ASCII85, ASCIIHex, RunLength) under `DecodeBudget`, strips inherited `/Rotate`/`/CropBox` after baking them into the Form XObject, and converts arbitrary `[llx, lly, urx, ury]` `/BBox` origins.
- **Q1–Q3 & F1–F14 (Caps, quarantine warnings, golden snapshots)**: `Limits.max_pages` propagates through `ToolCtx` to cap all page-amplifying tools at `min(10_000, max_pages)`, `ariad-host` emits `attachment_quarantined` warnings when `.bin`-quarantining risky extracted attachments, and `tools_structural` golden hashes reflect F1 raw attachment/filename preservation and F3 rotated-base overlay normalization.
