# Worker Report: Phase 3 T1 — Shared Infrastructure & Parameter Schemas

## Summary of Changes

Implemented the shared infrastructure, parameter schemas, empty spec modules, and central tool dispatcher for the 25 structural PDF tools across `ariad-core`, `ariad-pdf`, `ariad-host`, and `ariad-wasm`:

1. **`crates/ariad-core/src/tools/params/**` (25 tool parameter schemas + shared types)**
   - Added `crates/ariad-core/src/tools/params/common.rs` defining `StandardPaper`, `CustomPaperSize`, `PaperSize`, `GridOrder`, `MarginSides`, and `Margins`.
   - Added one parameter module per tool under `crates/ariad-core/src/tools/params/`:
     - Page assembly (8): `merge`, `split`, `extract_pages`, `delete_pages`, `reorder`, `alternate_mix`, `insert_blank`, `rotate`.
     - Geometry & inspection (5): `crop`, `scale`, `single_page`, `rotate_content`, `decompress`.
     - Layout & imposition (5): `nup`, `booklet`, `overlay`, `divide_pages`, `posterize`.
     - Document structure & metadata (7): `metadata`, `outline`, `attachments`, `page_labels`, `viewer_prefs`, `portfolio`, `layers`.
   - Every parameter struct derives `Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema` with `#[serde(deny_unknown_fields)]`, defaults matching `docs/tools.md` §14, and serde deserialization validators for domain constraints (`angle` quarter turns, `n` in `{2, 4, 6, 8, 9, 16}`, bounded numeric ranges, non-empty required lists/fields, and `PageRange` syntax).
   - Wired and re-exported all 25 parameter types in `crates/ariad-core/src/tools/params/mod.rs`.

2. **`crates/ariad-core/src/tools/specs/**` (Empty spec modules)**
   - Added `organize_pages.rs`, `organize_geometry.rs`, `organize_layout.rs`, and `organize_document.rs`, each exposing `pub fn specs() -> Vec<ToolSpec> { Vec::new() }` so no stubbed tool is registered in `tools::all()` during T1.
   - Wired all four modules in `crates/ariad-core/src/tools/specs/mod.rs` and added `pub fn all_specs() -> Vec<ToolSpec>`.

3. **`crates/ariad-pdf/src/{paper.rs,naming.rs,gc.rs,writer.rs,xobject.rs}`**
   - `paper.rs`: Exact PDF point dimensions (`72 pt = 1 inch`) for `A0`..`A6`, `Letter`, `Legal`, `Tabloid`, `Ledger`, case-insensitive lookup, and `resolve_paper_size` for `ariad_core::tools::params::PaperSize`.
   - `naming.rs`: Deterministic default single-output (`<stem>.<suffix>.pdf`) and multi-output (`<stem>.<label>-<NNN>.<ext>`) naming helpers plus internal trailer stem propagation (`/_AriadInputStem`) so single-arg tool signatures `run(doc: &PdfDoc, ...)` can derive `<stem>` without signature changes.
   - `gc.rs`: `gc::collect(doc: &mut PdfDoc, ctx: &ToolCtx) -> Result<usize, PdfError>` and `gc::collect_document` reachability walk from trailer references (`/Root`, `/Info`, etc.), pruning unreachable objects and obsolete `/ObjStm` / `/XRef` streams.
   - `writer.rs`: `writer::save(doc: &mut PdfDoc, ctx: &ToolCtx) -> Result<Vec<u8>, PdfError>` and `writer::save_with_options`, performing GC, SHA-256 stream deduplication, breadth-first object renumbering starting at `(1, 0)`, lexicographical dictionary key sorting, deterministic `/CreationDate`, `/ModDate`, and `/ID` derivation from `SOURCE_DATE_EPOCH` (or SHA-256 content digest when unset), and full xref table/stream rewrite from scratch.
   - `xobject.rs`: `page_to_form_xobject`, `page_to_form_xobject_in_place`, `ObjectCopier`, `placement_matrix`, `content_rotation_matrix`, and `render_place_xobject`, handling inherited `/Resources`, `/Rotate` (`0`, `90`, `180`, `270`), `/CropBox` / `/MediaBox` offsets, and multi-stream `/Contents` arrays.

4. **`crates/ariad-pdf/src/tools/{mod.rs,*.rs}` & Host / WASM Dispatchers**
   - Added `InputDoc`, `OutputDoc`, `OutputArtifact`, `ToolOutput`, `REPAIRED_XREF_WARNING`, and `ariad_pdf::tools::run_tool` in `crates/ariad-pdf/src/tools/mod.rs`, plus 25 stub modules returning `Err(PdfError::NotImplemented("pdf.<name>".to_owned()))`.
   - Generalized `crates/ariad-host/src/tools/inprocess.rs::execute` and `crates/ariad-wasm/src/api.rs::run_tool_with_spec` to route any `ariad-pdf` tool through `ariad_pdf::tools::run_tool`, staging returned artifacts, forwarding warnings, and preserving `pdf.info` inline JSON and metrics.
   - Exposed `pub mod ctx { pub use crate::document::ToolCtx; }` in `crates/ariad-pdf/src/lib.rs` so both `ariad_pdf::ToolCtx` and `ariad_pdf::ctx::ToolCtx` resolve cleanly.

## Verification Commands & Results

All commands were executed under `/Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift/.run/coord/build-lock.sh`:

- `cargo test --locked -p ariad-pdf -p ariad-core --lib` -> **PASS** (`229 passed` in `ariad-core`, `11 passed` in `ariad-pdf`)
- `cargo test --locked -p ariad-pdf --test info` -> **PASS** (`7 passed`)
- `cargo test --locked -p ariad-core --test tools_registry --test schema_drift` -> **PASS**
- `cargo test --locked -p ariad-host --lib` -> **PASS**
- `cargo test --locked -p ariad-host --features test-probe --test tool_publish_atomic --test tool_secrets` -> **PASS**
- `cargo test --locked -p ariad-wasm --lib` -> **PASS**
- `cargo test --locked -p ariad-cli --features ariad-host/test-probe --test tools_cli` -> **PASS**
- `cargo check --locked -p ariad-pdf -p ariad-wasm --target wasm32-unknown-unknown` -> **PASS**
- `cargo clippy --locked -p ariad-core -p ariad-pdf -p ariad-host -p ariad-wasm --all-targets -- -D warnings` -> **PASS**
- `cargo fmt --all --check` -> **PASS**
- `env UV_NO_CONFIG=1 just spell` -> **PASS**

## New Dependencies & Licences

- **None.** SHA-256 and UTC PDF timestamp formatting in `crates/ariad-pdf/src/writer.rs` are implemented in self-contained pure Rust (validated against NIST FIPS 180-4 vectors), keeping `Cargo.lock` and `THIRD_PARTY_LICENSES` untouched.

## Open Questions / Notes for Coordinator

- `crates/ariad-core/src/tools/registry.rs` is outside T1's owned file list and currently defines `const ALL_TOOLS: &[ToolSpec] = &[specs::organize_info::SPEC];`. `crates/ariad-core/src/tools/specs/mod.rs` now provides `pub fn all_specs() -> Vec<ToolSpec>` concatenating `organize_info`, `organize_pages`, `organize_geometry`, `organize_layout`, and `organize_document` so T5 can wire `registry.rs` to `specs::all_specs()` (via `LazyLock<Vec<ToolSpec>>` or static slice) when registering the 25 `ToolSpec`s.
