# Worker Report — Sequence & Interleave Structural PDF Tools (`pdf.reorder`, `pdf.alternate_mix`, `pdf.insert_blank`)

## 1. Summary of Changes

Implemented the three page-sequence and interleaving structural PDF tools in `crates/ariad-pdf` and their standalone `ToolSpec` module in `crates/ariad-core`:

1. **`crates/ariad-core/src/tools/specs/organize_sequence.rs`** (new):
   - Exposes `pub fn specs() -> Vec<ToolSpec>` declaring `pdf.reorder` (`InputArity::One`, params `ReorderParams`), `pdf.alternate_mix` (`InputArity::many(2, 2)`, params `AlternateMixParams`), and `pdf.insert_blank` (`InputArity::One`, params `InsertBlankParams`), all in `ToolGroup::Organize`, `Tier::Free`, `runtimes: &[ToolRuntime::Wasm, ToolRuntime::Local]`, `engine: "ariad-pdf"`.
   - Parameter definitions and unit tests in `crates/ariad-core/src/tools/params/{reorder,alternate_mix,insert_blank}.rs` were verified complete against `docs/tools.md` §3.1.

2. **`crates/ariad-pdf/src/tools/reorder.rs`**:
   - Implemented `pub fn run(inputs: &[PdfDoc], params: &ReorderParams, ctx: &ToolCtx) -> Result<Vec<OutputDoc>, PdfError>`.
   - Supports presets (`Reverse`, `DuplexInterleave`, `OddThenEven`) and custom `PageRange` sequences (`DuplicatePolicy::KeepInOrder`).
   - Materializes inherited `/Resources`, `/MediaBox`, `/CropBox`, `/BleedBox`, `/TrimBox`, `/ArtBox`, and effective `/Rotate` (including parent/sibling inference matching `info.rs`) directly onto each `/Page` dictionary before rebuilding a flat `/Pages` root.
   - Clones duplicate page selections into distinct `/Page` object IDs (cloning non-widget annotations with updated `/P` back-pointers) so `lopdf` and `pdf.info` preserve every repeated page in `Kids`.
   - Updates `/Annots` destinations and `/GoTo` actions when pages are dropped, prunes `/AcroForm` fields whose widget annotations belong only to dropped pages, filters and remaps `/Outlines` (promoting surviving child bookmarks when a parent's target page was dropped), updates `/PageLabels` when prefix order is preserved (or removes `/PageLabels` when reordered), and runs `gc::collect_document`.

3. **`crates/ariad-pdf/src/tools/alternate_mix.rs`**:
   - Implemented `pub fn run(inputs: &[PdfDoc], params: &AlternateMixParams, ctx: &ToolCtx) -> Result<Vec<OutputDoc>, PdfError>`.
   - Validates `inputs.len() == 2` and rejects encrypted documents in either input slot with `PdfError::Encrypted`.
   - Deep-copies all pages, resources, streams, annotations, `/AcroForm` fields, and `/Outlines` from the second document into a clone of the first document via `xobject::ObjectCopier`, materializing each source page's inherited boxes, rotation, and resources before interleaving.
   - Reverses the second document's page list when `params.reverse_second` is `true` (`[A1, B_N, A2, B_{N-1}, ...]`) and appends any trailing pages when input lengths differ.
   - Combines `/Outlines` and `/AcroForm` fields from both inputs, strips `/PageLabels`, propagates `REPAIRED_XREF_WARNING` if either input had a repaired xref table, and runs `gc::collect_document`.

4. **`crates/ariad-pdf/src/tools/insert_blank.rs`**:
   - Implemented `pub fn run(inputs: &[PdfDoc], params: &InsertBlankParams, ctx: &ToolCtx) -> Result<Vec<OutputDoc>, PdfError>`.
   - Validates `inputs.len() == 1` and `params.count >= 1`, expanding `params.after` with `DuplicatePolicy::Reject`.
   - Resolves blank page geometry per insertion point: `BlankPageSize::Same` matches the preceding page's effective visible `/CropBox` width/height and effective `/Rotate`; `BlankPageSize::Paper` uses `paper::resolve_paper_size` with `/Rotate 0`.
   - Allocates a shared empty content stream (`Stream::new(dictionary! {}, Vec::new())`) and inserts `params.count` distinct `/Page` dictionaries with empty `/Resources` after each selected page so `pdf.info` classifies every inserted page with `blank: true`.
   - Preserves `/Outlines`, `/AcroForm`, and `/Annots`, shifts `/PageLabels /Nums` start indices to match each original page's new 0-based index, and runs `gc::collect_document`.

5. **Integration Tests & Snapshots**:
   - `crates/ariad-pdf/tests/reorder.rs` (7 tests) + `crates/ariad-pdf/tests/snapshots/reorder__rotated_pages_reverse.snap`
   - `crates/ariad-pdf/tests/alternate_mix.rs` (5 tests) + `crates/ariad-pdf/tests/snapshots/alternate_mix__rotated_and_mixed_reverse_second.snap`
   - `crates/ariad-pdf/tests/insert_blank.rs` (5 tests) + `crates/ariad-pdf/tests/snapshots/insert_blank__mixed_sizes_same.snap`

## 2. Integrator Wiring Notes

Because `crates/ariad-core/src/tools/specs/mod.rs`, `crates/ariad-core/src/tools/registry.rs`, and `crates/ariad-pdf/src/tools/mod.rs` are read-only for this worker slice:

1. **ToolSpec Registry Wiring (`ariad-core`)**:
   - Add `pub mod organize_sequence;` to `crates/ariad-core/src/tools/specs/mod.rs`.
   - In `crates/ariad-core/src/tools/registry.rs::build_registry()`, add `out.extend(specs::organize_sequence::specs());` and remove the inline `pdf.reorder`, `pdf.alternate_mix`, and `pdf.insert_blank` entries from `crates/ariad-core/src/tools/specs/organize_pages.rs`.
2. **Stub Test Update (`ariad-pdf`)**:
   - In `crates/ariad-pdf/src/tools/mod.rs::tests::run_tool_rejects_unknown_tool_id_and_returns_not_implemented_for_all_25_stubs`, remove `("pdf.reorder", ...)`, `("pdf.alternate_mix", ...)`, and `("pdf.insert_blank", ...)` from `stub_cases`.

## 3. Verification Executed

All commands were executed under `/Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift/.run/coord/build-lock.sh`:

- `cargo fmt --all --check` — passed (exit 0)
- `cargo clippy --locked -p ariad-core -p ariad-pdf --all-targets -- -D warnings` — passed (0 warnings)
- `cargo test --locked -p ariad-pdf --test reorder --test alternate_mix --test insert_blank` — passed (17/17 tests: 7 in `reorder`, 5 in `alternate_mix`, 5 in `insert_blank`)
- `cargo test --locked -p ariad-core --lib` — passed (229/229 unit tests)
