# Phase 3 — S5 Worker Report (`argon` / `agy-6178`)

## Summary

Implemented Phase 3 slice S5 (`OrganizeDocument` structural PDF tools) in `ariad-core` and `ariad-pdf`:

1. **`pdf.metadata`**:
   - Registered `PDF_METADATA_SPEC` in `crates/ariad-core/src/tools/specs/organize_document.rs`.
   - Implemented `set` (standard `/Title`, `/Author`, `/Subject`, `/Keywords`, `/Creator`, `/Producer`, `/CreationDate`, `/ModDate`, plus custom `/Info` keys), `remove` (case-insensitive canonical names + custom keys), and `strip_all` (removes `/Info` from trailer and `/Metadata` from `/Catalog`, with `SaveOptions { preserve_info_dates: true, omit_info_if_absent: true }`).
   - Synchronizes XMP `/Metadata` packet (`dc:title`, `dc:creator`, `dc:description`, `pdf:Keywords`, `xmp:CreatorTool`, `pdf:Producer`, `xmp:CreateDate`, `xmp:ModifyDate`) when present or when standard fields are updated.
2. **`pdf.outline`**:
   - Registered `PDF_OUTLINE_SPEC` in `crates/ariad-core/src/tools/specs/organize_document.rs`.
   - Implemented `export` (walks `/Outlines` hierarchy resolving explicit page destination arrays and named destinations via `/Dests` / `/Names/Dests` to 1-based page numbers, emitting `<stem>.outline.json`), `import` (validates 1-based page bounds `1..=page_count`, builds `/Outlines` root and doubly-linked `/First`, `/Last`, `/Parent`, `/Prev`, `/Next`, `/Count` items with `[page_ref /Fit]` destinations), `clear` (removes `/Outlines` from `/Catalog`), and `from_headings` (extracts headings per page via `ariad_core::reader::pdf::read` and builds a nested bookmark tree).
3. **`pdf.attachments`**:
   - Registered `PDF_ATTACHMENTS_SPEC` in `crates/ariad-core/src/tools/specs/organize_document.rs`.
   - Implemented `list` (emits `<stem>.attachments.json` with `[{ name, media_type, size, sha256 }]` matching `truth.json["attachments"]`), `add` (embeds `inputs[1..]` into `inputs[0]` under `/Names/EmbeddedFiles` and `/AF`, enforcing per-attachment size cap `Limits::cloud().max_asset_bytes` and charging `DecodeBudget`), `extract` (emits `<stem>.attachment-NNN.<safe_name>` for all or filtered `names`), and `remove` (removes named or all embedded files and cleans up `/Names/EmbeddedFiles` and `/AF` when empty).
4. **`pdf.page_labels`**:
   - Registered `PDF_PAGE_LABELS_SPEC` in `crates/ariad-core/src/tools/specs/organize_document.rs`.
   - Implemented `/PageLabels` number tree construction on `/Catalog` supporting all 6 styles (`Decimal` -> `/D`, `RomanUpper` -> `/R`, `RomanLower` -> `/r`, `AlphaUpper` -> `/A`, `AlphaLower` -> `/a`, `None` -> omitted `/S`), optional `/P` prefix, `/St` start number, and 1-based `from` validation (`1..=page_count`, no duplicate `from` pages).
5. **`pdf.viewer_prefs`**:
   - Registered `PDF_VIEWER_PREFS_SPEC` in `crates/ariad-core/src/tools/specs/organize_document.rs`.
   - Implemented `/PageLayout` (`SinglePage`, `OneColumn`, `TwoColumnLeft`, `TwoColumnRight`, `TwoPageLeft`, `TwoPageRight`), `/PageMode` (`UseNone`, `UseOutlines`, `UseThumbs`, `FullScreen`, `UseOC`, `UseAttachments`), `/ViewerPreferences` (`/HideToolbar`, `/FitWindow`, `/DisplayDocTitle`), and `/OpenAction` (`[page_ref /Fit]` validated against `1..=page_count`).
6. **`pdf.portfolio`**:
   - Registered `PDF_PORTFOLIO_SPEC` in `crates/ariad-core/src/tools/specs/organize_document.rs`.
   - Implemented `create` (builds a single-page cover PDF with `/Collection << /Type /Collection /View /D|/T >>` on `/Catalog` and embeds all input documents into `/Names/EmbeddedFiles` + `/AF`), `list` (emits `<stem>.portfolio.json`), and `extract` (extracts all embedded files).
7. **`pdf.layers`**:
   - Registered `PDF_LAYERS_SPEC` in `crates/ariad-core/src/tools/specs/organize_document.rs`.
   - Implemented `list` (reads `/OCProperties/OCGs` and `/D` `/BaseState`, `/ON`, `/OFF` to emit `[{ name, visible }]`), `set_default_visibility` (updates `/OCProperties/D` `/ON` and `/OFF` arrays for named layers), and `remove` (strips `/OC /<prop_name> BDC ... EMC` content blocks across all pages, cleans up `/Resources/Properties` even when shared across pages, and updates or removes `/OCProperties`).

## Files Changed

- `crates/ariad-core/src/tools/specs/organize_document.rs`
- `crates/ariad-pdf/src/tools/metadata.rs`
- `crates/ariad-pdf/src/tools/outline.rs`
- `crates/ariad-pdf/src/tools/attachments.rs`
- `crates/ariad-pdf/src/tools/page_labels.rs`
- `crates/ariad-pdf/src/tools/viewer_prefs.rs`
- `crates/ariad-pdf/src/tools/portfolio.rs`
- `crates/ariad-pdf/src/tools/layers.rs`
- `crates/ariad-pdf/tests/metadata.rs`
- `crates/ariad-pdf/tests/outline.rs`
- `crates/ariad-pdf/tests/attachments.rs`
- `crates/ariad-pdf/tests/page_labels.rs`
- `crates/ariad-pdf/tests/viewer_prefs.rs`
- `crates/ariad-pdf/tests/portfolio.rs`
- `crates/ariad-pdf/tests/layers.rs`
- `crates/ariad-pdf/tests/snapshots/metadata__metadata_multipage_20_info.snap`
- `crates/ariad-pdf/tests/snapshots/outline__outline_rotated_pages_imported_info.snap`
- `crates/ariad-pdf/tests/snapshots/attachments__attachments_list_summary.snap`
- `crates/ariad-pdf/tests/snapshots/page_labels__page_labels_six_styles.snap`
- `crates/ariad-pdf/tests/snapshots/viewer_prefs__viewer_prefs_catalog_summary.snap`
- `crates/ariad-pdf/tests/snapshots/portfolio__portfolio_list_entries.snap`
- `crates/ariad-pdf/tests/snapshots/layers__layers_list_watermark_layer.snap`
- `plans/reports/worker-p03-s5-argon.md`

## Verification

- `cargo test --locked -p ariad-pdf --test metadata --test outline --test attachments --test page_labels --test viewer_prefs --test portfolio --test layers`: 25 passed, 0 failed.
- `cargo test --locked -p ariad-core --lib`: 230 passed, 0 failed (including `organize_document_specs_are_valid`).
- `cargo clippy --locked -p ariad-core -p ariad-pdf --all-targets -- -D warnings`: 0 warnings, 0 errors.
- `cargo fmt --all --check`: clean.
