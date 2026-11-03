# Worker Fix Report: `pandoc-mapping` (Issues #81 partial and #94)

status: complete — derived list tightness from Pandoc AST item blocks in `to_ir.rs`, populated Pandoc figure image alt text from caption plain text in `from_ir.rs`, and verified with unit and DOCX golden/round-trip tests.

## Assumptions and Scope Decisions

- **Issue #81 scope**: Only the list tightness loss across DOCX round trips is addressed here. Losing the fenced code-block language across DOCX round trips is **not** in scope; preserving code-block languages through DOCX requires a DOCX custom style mapping and belongs to Phase 11 of the next-stage plan (`plans/261031-1422-next-stage-pdf-suite/plan.md`).
- **List tightness rule (`to_ir.rs`)**: Probing Pandoc 3.12 (`pandoc -f markdown -t json | pandoc -f json -t docx` followed by `pandoc -f docx -t json`) confirmed that Pandoc writes tight list items (`Plain`) using the `Compact` paragraph style in `word/document.xml` and reads them back as `Block::Plain` (plus optional nested `Block::BulletList` or `Block::OrderedList` blocks), whereas loose list items are written without `Compact` and read back as `Block::Para`. We therefore classify a non-empty `BulletList` or `OrderedList` as `tight: true` when every item contains at least one `Block::Plain` and every block in the item is `Block::Plain`, `Block::BulletList`, or `Block::OrderedList`. `DefinitionList` remains `tight: false` because each entry expands into a strong-term paragraph followed by definition paragraph(s).
- **Figure alt text rule (`from_ir.rs`)**: In `ariad-core::reader::html`, a `<figure>` containing an `<img>` and `<figcaption>` is folded into `IrBlock::Figure { asset, caption }` only when the image's `alt` matches the caption text, and both the HTML and Markdown writers derive the `<img>` `alt` attribute from `plain_text(caption)`. Populating the Pandoc `Figure` inner `Inline::Image` alt inlines with `text_inlines(&plain_text(caption))` makes DOCX output emit `wp:docPr descr="<caption plain text>"` and preserves the figure across DOCX-to-HTML and DOCX-to-Markdown round trips.

## Per-Issue Details

### Issue #81 (partial): Tight lists become loose after DOCX round trip

- **Cause with evidence**:
  - `crates/ariad-core/src/pandoc/to_ir.rs` hard-coded `tight: false` in `Block::OrderedList` (line 193) and `Block::BulletList` (line 208).
  - Empirical probe with `/home/vchun/Codes/01-AriadShift/.tools/pandoc/bin/pandoc` (3.12) showed that tight Markdown lists (`- one\n- two\n- three\n` and `1. one\n2. two\n`) round-trip through DOCX back to Pandoc JSON AST with `Block::Plain` items, whereas loose lists round-trip with `Block::Para` items. Ignoring `Plain` vs `Para` in `to_ir.rs` caused every list read from DOCX to become `tight: false`.
  - Code-block language loss in #81 is out of scope for this fix; it requires a DOCX custom style and belongs to Phase 11 of the next-stage plan.
- **Fix**:
  - Added `is_tight_list(items: &[Vec<Block>]) -> bool` in `crates/ariad-core/src/pandoc/to_ir.rs` and wired it into `Block::OrderedList` and `Block::BulletList`, with a code comment documenting the Pandoc 3.12 `Plain` vs `Para` (`Compact` style) rule and explaining why `DefinitionList` stays `tight: false`.
- **Tests added**:
  - `pandoc::to_ir::tests::maps_tight_and_loose_bullet_and_ordered_lists` (`crates/ariad-core/src/pandoc/to_ir.rs`)
  - `pandoc::to_ir::tests::preserves_tightness_for_nested_lists_and_rejects_mixed_items` (`crates/ariad-core/src/pandoc/to_ir.rs`)
  - `tight_lists_stay_tight_after_docx_round_trip` (`crates/ariad-cli/tests/docx_golden.rs`)

### Issue #94: Figure image alt text dropped when mapping `IrBlock::Figure` to Pandoc `Figure`

- **Cause with evidence**:
  - `crates/ariad-core/src/pandoc/from_ir.rs` (lines 178–195) constructed the `Block::Figure` body's `Inline::Image` with `Vec::new()` as its alt inlines, causing Pandoc's DOCX writer to emit `<wp:docPr descr="" .../>` in `word/document.xml` even when an HTML `<figure>` had matching `alt` and `<figcaption>`.
- **Fix**:
  - Added `plain_text(inlines: &[IrInline]) -> String` in `crates/ariad-core/src/pandoc/from_ir.rs` (matching the helper in the HTML and Markdown writers) and passed `text_inlines(&plain_text(caption))` as the alt inlines of `Inline::Image` when mapping `IrBlock::Figure`.
- **Tests added**:
  - `pandoc::from_ir::tests::maps_table_alignment_spans_caption_and_figures` (updated assertion in `crates/ariad-core/src/pandoc/from_ir.rs`)
  - `pandoc::from_ir::tests::populates_figure_image_alt_with_caption_plain_text` (`crates/ariad-core/src/pandoc/from_ir.rs`)
  - `captioned_html_figure_preserves_image_descr_in_docx` (`crates/ariad-cli/tests/docx_golden.rs`)

## Verification Commands

- `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh cargo fmt --all --check` — **PASS**
- `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh cargo clippy -p ariad-core -p ariad-cli --all-targets -- -D warnings` — **PASS**
- `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh cargo test -p ariad-core --lib pandoc` — **PASS** (21 passed)
- `ASHIFT_PANDOC=/home/vchun/Codes/01-AriadShift/.tools/pandoc/bin/pandoc /home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh cargo test -p ariad-cli --test docx_golden` — **PASS** (4 passed)
- `sh scripts/check-commits.sh origin/dev..HEAD` — failed only because `.tools/bin/committed` is not present in this worktree; verified commits with `/home/vchun/Codes/01-AriadShift/.tools/bin/committed --config committed.toml origin/dev..HEAD` — **PASS**

## What Only CI or Another OS Can Verify

- Cross-platform Pandoc 3.12 execution on macOS and Windows (including CRLF normalization in template XML parts on Windows).
- Full workspace test matrix (`just ci`) across crates outside the narrow check scope.

## Suggested PR Text

**Title:** `fix(core): preserve Pandoc list tightness and figure image alt text`

**Body:**
```markdown
## Summary
- Derive `IrBlock::List.tight` in `ariad-core::pandoc::to_ir` from Pandoc 3.12's `Plain` vs `Para` item blocks (`Compact` paragraph style in DOCX), so tight bullet and ordered lists stay tight after a DOCX round trip. (Code-block language preservation in DOCX requires a custom style and is deferred to Phase 11 of the next-stage plan.)
- Populate the `Inline::Image` alt inlines of `Block::Figure` in `ariad-core::pandoc::from_ir` from the plain text of the figure caption so DOCX output carries `wp:docPr descr` and round-trips back to HTML `<figure>`.
- Add unit tests in `ariad-core::pandoc::{to_ir, from_ir}` and DOCX golden/round-trip integration tests in `crates/ariad-cli/tests/docx_golden.rs`.

Refs #81
Closes #94
```
