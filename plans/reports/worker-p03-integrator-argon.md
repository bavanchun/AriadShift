# Worker Report: Phase 3 Integrator (`feat/pdf-structural-tools`)

status: complete

## 1. Slice Branch Merges & Cleanup

All five slice branches on top of `6cb9019` were merged into `feat/pdf-structural-tools` via `git merge --no-ff --no-edit`, verified as fully merged with `git cherry feat/pdf-structural-tools <branch>`, and deleted with `git branch -d`:

| Slice | Branch | Slice Commit | Merge Commit | Scope | `git cherry` | `git branch -d` |
|---|---|---|---|---|---|---|
| S1 | `agent-rex/agy-f58f` | `4165b37` | `8bf81bb` | `merge`, `split`, `extract_pages`, `delete_pages` + `organize_pages` specs | `- 4165b37...` | Deleted |
| S2 | `agent-rex/agy-9ab1` | `1ca26f2` | `479910d` | `reorder`, `alternate_mix`, `insert_blank` + `organize_sequence` specs | `- 1ca26f2...` | Deleted |
| S3 | `agent-rex/agy-a114` | `daf1033` | `df9c2ec` | `rotate`, `crop`, `scale`, `single_page`, `rotate_content`, `decompress` + `organize_geometry` specs | `- daf1033...` | Deleted |
| S4 | `agent-rex/agy-13fd` | `68000ba` | `1c17c1e` | `nup`, `booklet`, `overlay`, `divide_pages`, `posterize` + `organize_layout` specs | `- 68000ba...` | Deleted |
| S5 | `agent-rex/agy-6178` | `47ada72` | `c6c086c` | `metadata`, `outline`, `attachments`, `page_labels`, `viewer_prefs`, `portfolio`, `layers` + `organize_document` specs | `- 47ada72...` | Deleted |

Zero merge conflicts occurred across the five merges.

---

## 2. Shared File Wiring

- **`crates/ariad-core/src/tools/specs/mod.rs`**: Added `pub mod organize_sequence;` and `list.extend(organize_sequence::specs());` to `all_specs()`.
- **`crates/ariad-core/src/tools/registry.rs`**: Updated `REGISTRY` `LazyLock<Vec<ToolSpec>>` to populate from `specs::all_specs()` (26 tools total: `pdf.info` + 25 Phase 3 tools).
- **`crates/ariad-pdf/src/tools/mod.rs`**: Replaced `run_tool_rejects_unknown_tool_id_and_returns_not_implemented_for_all_25_stubs` with `run_tool_rejects_unknown_tool_id_and_dispatches_all_25_structural_tools` (verifying unknown tool error and successful execution of all 25 structural tools), and updated `central_dispatcher_records_repaired_xref_warning_and_rewrites_broken_xref_validly` to verify `pdf.rotate` succeeds on `broken-xref.pdf` with `REPAIRED_XREF_WARNING`.
- **`crates/ariad-host/src/tools/inprocess.rs`**: Updated unit test `inprocess_execute_routes_structural_tools_through_ariad_pdf_run_tool` so `pdf.rotate` succeeds end-to-end and `map_pdf_error(PdfError::NotImplemented(...))` is verified directly.
- **`crates/ariad-host/src/tools/publish.rs`**: Extended `default_tool_suffix` with all 26 `ariad-pdf` tools matching `ariad_pdf::naming::tool_suffix` and `docs/tools.md` §6 (`extracted`, `trimmed`, `reordered`, `cropped`, `scaled`, `single-page`, `rotated-content`, `decompressed`, `divided`, `posterized`, `inserted`, `mixed`, `labeled`, `prefs`, etc.).
- **`crates/ariad-wasm/src/api.rs`**: Updated `run_tool_with_spec` to use each `OutputArtifact.format` when non-empty (so JSON-emitting actions on `pdf.outline`, `pdf.attachments`, `pdf.portfolio`, and `pdf.layers` report `"json"` instead of `"pdf"`), and updated unit test `run_tool_validates_tool_id_inputs_formats_params_and_secrets` so `pdf.rotate` succeeds in-memory.
- **`crates/ariad-cli/tests/tools_cli.rs` & `crates/ariad-cli/tests/mcp_stdio.rs`**: Updated tool registry count assertions from 1 to 26 (`25` in `organize` + `1` in `optimize`).
- **`crates/ariad-pdf/src/tools/{extract_pages.rs,merge.rs}` & `typos.toml`**: Expanded an abbreviated rotation-inheritance closure variable to `is_inherited` and registered standard PDF 1.7 stream dictionary keys `DecodeParms` and `FDecodeParms` in `typos.toml`.

---

## 3. `ToolSpec` vs. `docs/parity-matrix.md` & `docs/tools.md` Reconciliations

Every one of the 26 registered `ToolSpec`s was reconciled against `docs/parity-matrix.md` and `docs/tools.md`:

1. **`pdf.decompress` in `docs/parity-matrix.md` (line 104)**:
   - Updated **Engine** from `qpdf` to `ariad-pdf`, **Runtimes** from `local, cloud` to `wasm, local, cloud`, and **Surfaces** from `CLI, MCP, web(h), mac` to `CLI, MCP, web, mac` to match the Phase 3 addendum and `organize_geometry::decompress_spec` (`group: ToolGroup::Optimize`, `engine: "ariad-pdf"`, `runtimes: &[ToolRuntime::Wasm, ToolRuntime::Local]`, `tier: Tier::Free`).
2. **Tool Groups table in `docs/tools.md` (§2)**:
   - Updated the `organize` row to list all 25 `organize` tools (including `pdf.overlay`, which was previously listed as an example under `overlay` in `docs/tools.md` despite being in `organize` in `docs/parity-matrix.md` line 57 and `organize_layout.rs`).
   - Added `pdf.decompress` to the `optimize` row examples.
3. **Input Arity & Formats in `docs/tools.md` (§3)**:
   - Documented `pdf.attachments` (`many(1, 100)`, `input_formats: ["pdf", "md", "html", "docx", "epub", "ir", "docling-json", "odt", "rtf", "xlsx", "ods", "pptx", "odp", "doc", "xls", "ppt", "png", "jpeg", "webp", "tiff"]`) and `pdf.portfolio` (`many(1, 500)`, `input_formats: ["pdf"]`) so action-polymorphic tools that accept sidecar attachments/member files on `"add"` / `"create"` and a single PDF on `"list"` / `"extract"` / `"remove"` are explicitly documented.
4. **Default Artifact Naming table in `docs/tools.md` (§6)**:
   - Expanded the suffix table to cover all 25 Phase 3 tools and multi-artifact naming patterns (`<stem>.part-<NNN>.pdf`, `<stem>.tile-<NNN>.pdf`, `<stem>.outline.json`, `<stem>.attachments.json`, `<stem>.portfolio.json`, `<stem>.layers.json`, and sanitized extracted filenames).
5. **All 25 Phase 3 `ToolSpec`s**:
   - Verified `tier: Tier::Free`, `runtimes: &[ToolRuntime::Wasm, ToolRuntime::Local]`, `engine: "ariad-pdf"`, `group: ToolGroup::Organize` (24 tools) and `ToolGroup::Optimize` (`pdf.decompress`).

---

## 4. Duplication Review: S1 (`extract_pages.rs`) vs. S2 (`reorder.rs`)

- **Analysis**:
  - S1 (`crates/ariad-pdf/src/tools/extract_pages.rs`) implements an out-of-place `SelectiveCopier` (`build_extracted_doc`) that copies only selected pages and their reachable object closure into a fresh `Document::with_version`, preserving `/Pages /Rotate` when inherited rotation is uniform (`inherited_rotation: true`) and rebuilding `/Outlines`, `/PageLabels`, and `/AcroForm` `/Fields`. `delete_pages.rs`, `merge.rs`, and `split.rs` call `extract_pages.rs` helpers directly.
  - S2 (`crates/ariad-pdf/src/tools/reorder.rs`) clones the full `Document` and mutates `/Pages` in place, materializing `/Rotate` onto individual `/Page` dictionaries (`inherited_rotation: false`), deep-cloning duplicated page subtrees, and stripping `/PageLabels` when reordered out of prefix order. `alternate_mix.rs` and `insert_blank.rs` call `reorder.rs` helpers (`collect_page_ids`, `pages_root_id`, `rebuild_pages_root`, `materialize_page_attributes`) directly.
- **Decision**:
  - Left both implementations intact for Phase 3. Unifying them now would alter the `inherited_rotation` (`true` vs `false`) and `/PageLabels` retention invariants locked into each slice's committed `.snap` files (`extract_pages`, `delete_pages`, `merge`, `split` vs `reorder`, `alternate_mix`, `insert_blank`).
- **Suggested Follow-up**:
  - Extract a shared `crates/ariad-pdf/src/tools/page_set.rs` helper parameterized by a rotation-inheritance policy (`PreserveParentRotateIfUniform` vs `MaterializeOnPage`) and page-label policy (`RemapContiguousRuns` vs `DropWhenNonIdentityPrefix`) when Phase 4 (`pdf.remove_blank`) touches page-subsetting.

---

## 5. Regenerated & Refreshed Files

- **`schemas/tools.v1.json`**: Regenerated via `ARIAD_SCHEMA_WRITE=1 cargo test --locked -p ariad-core --test schema_drift -- --ignored write_committed_schemas` and verified with `cargo test --locked -p ariad-core --test tools_registry --test schema_drift`.
- **`crates/ariad-cli/tests/snapshots/tools_cli__tools_json.snap`**: Updated intentional snapshot because `ashift tools --json` now outputs all 26 registered tools (`pdf.info` + 25 Phase 3 tools) instead of only `pdf.info`. No other existing snapshot was modified.
- **`Cargo.lock` / `fuzz/Cargo.lock` / `THIRD_PARTY_LICENSES`**: Unchanged (no dependencies added or modified).

---

## 6. Phase Gate Verification Commands & Results

All commands were executed inside `/Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift/.run/coord/build-lock.sh`:

| Command | Result | Details |
|---|---|---|
| `cargo fmt --all --check` | **PASS** | 0 formatting diffs |
| `cargo clippy --locked --workspace --all-targets -- -D warnings` | **PASS** | 0 warnings across workspace |
| `cargo test --locked -p ariad-core -p ariad-pdf -p ariad-engine-io` | **PASS** | **611 passed**, 0 failed, 4 ignored (`ariad-core`: 233 unit + 266 integration = 499; `ariad-engine-io`: 5 unit; `ariad-pdf`: 11 unit + 96 integration across 26 tool test binaries = 107) |
| `ASHIFT_PANDOC=.../.tools/pandoc/bin/pandoc cargo test --locked -p ariad-host --features test-probe -- --test-threads=1` | **PASS** | **390 passed**, 0 failed, 0 ignored (166 unit + 224 integration) |
| `ASHIFT_PANDOC=.../.tools/pandoc/bin/pandoc cargo test --locked -p ariad-cli --features ariad-host/test-probe -- --test-threads=2` | **PASS** | **197 passed**, 0 failed, 9 ignored (2 unit + 195 integration, including 43 `mcp_stdio` and 5 `tools_cli`) |
| `cargo test --locked -p ariad-wasm` | **PASS** | **13 passed**, 0 failed |
| `cargo check --locked -p ariad-pdf -p ariad-wasm --target wasm32-unknown-unknown` | **PASS** | Clean `wasm32-unknown-unknown` target check |
| `env UV_NO_CONFIG=1 just spell` | **PASS** | 0 typos |
| `env UV_NO_CONFIG=1 just licenses-check` | **PASS** | `THIRD_PARTY_LICENSES`, desktop JS licences, and web notice graph verified |
| `bash scripts/test-ci-scope.sh` | **PASS** | **82 passed**, 0 failed |
| `cargo publish --dry-run --locked -p ariad-pdf -p ariad-engine-io --config 'patch.crates-io.ariad-core.path="crates/ariad-core"'` | **PASS** | Both tarballs packaged and compiled cleanly (note: unpatched `cargo publish --dry-run` pulls `ariad-core 0.2.0` from crates.io which predates Phase 2 `protocol::v2`; patching `ariad-core` as `just package-check` does passes cleanly) |
| `sh scripts/check-commits.sh origin/dev..HEAD` | **PASS** | All commits in `origin/dev..HEAD` conform to conventional commit and line-length rules |

- **Anything left red**: **None.** All tests and checks pass.
- **Deferred WASM build command (not executed per instructions)**:
  ```bash
  source /Volumes/AriadWork/02-Ariadnev-Eco/.cache/env.sh && /Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift/.run/coord/build-lock.sh env UV_NO_CONFIG=1 just wasm-build
  ```

---

## 7. Notes for the Phase Gate

- `ariad-core::tools::all()` now returns 26 `ToolSpec`s (`25` in `organize` + `1` in `optimize` for `pdf.decompress`), all with `engine = "ariad-pdf"`, `tier = Free`, and `runtimes = [Wasm, Local]`.
- `pdf.outline`, `pdf.attachments`, `pdf.portfolio`, and `pdf.layers` emit a `.json` artifact on `"list"` actions (`OutputArtifact.format = "json"`) and `.pdf` (or extracted attachment bytes) on mutating/extracting actions; both `ariad-host` and `ariad-wasm` preserve each artifact's declared `format`.
- Running `cargo test -p ariad-host --lib` (without `nextest`) should pass `-- --test-threads=1` on external SSDs so concurrent Pandoc subprocess probes in `ariad-host` unit tests do not contend on the 5-second version-check timeout.
- `cargo publish --dry-run -p ariad-pdf -p ariad-engine-io` requires `--config 'patch.crates-io.ariad-core.path="crates/ariad-core"'` (matching `just package-check`) until the next `ariad-core` release is published to crates.io.
