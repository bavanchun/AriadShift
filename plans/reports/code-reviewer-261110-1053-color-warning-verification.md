# Code Review: PDF Unknown-Colour Warning Classification

## Scope

- `crates/ariad-core/src/reader/pdf/mod.rs`
- `crates/ariad-core/src/reader/pdf/interpreter.rs`
- `crates/ariad-core/src/warning.rs`
- `crates/ariad-pdf/src/redact/verify_text.rs`
- `crates/ariad-host/src/tools/redact.rs`
- `crates/ariad-host/tests/redact_pipeline.rs`
- `crates/ariad-core/tests/pdf_styles.rs`
- `crates/ariad-core/tests/pdf_reader.rs`
- `/tmp/p11-int-evidence/redaction-diagnosis.log`

This was a read-only source review. Per the delegation constraint, no Cargo or other build command was run.

## Verdict

The classification repair is safe and correctly distinguishes cosmetic colour-style loss from incomplete text evidence. Changing only the `unknown_color` warning from `UnsupportedNode` to the existing `StylesDropped` code does not relax the verifier's handling of actual text loss: Unicode mapping loss remains `TextUnmapped`, and operator/glyph-limit truncation remains `UnsupportedNode`. The host's six-check aggregation and fail-closed publication gates are unchanged.

`unknown_color` does not imply lost text. It is set when `Paint::color` is `None` after colour-space resolution or colour-operand validation. Text-showing operators independently decode glyphs through `show`/`record`; the optional paint colour is attached to each recorded glyph after decoding. Unsupported colour therefore leaves the text and geometry available while omitting only the colour value. The new focused test exercises both directions with actual text under an unsupported Lab colour space: an absent target passes and a present target fails.

Reusing `StylesDropped` is consistent with the existing warning contract. `WarningCode` already serializes that stable code as `styles_dropped`, its documented meaning is that style information cannot be retained, and the repair does not add an enum variant or change a schema. Retaining the existing message also tells callers precisely which style value remained unknown.

## Finding

### Informational: stale fixture assertion must follow the classification

`crates/ariad-core/tests/pdf_reader.rs` around line 225 still expects the real `vi-garden-route.pdf` ICCBased-colour warning to use `WarningCode::UnsupportedNode`. The repaired reader now emits `WarningCode::StylesDropped`, and `crates/ariad-core/tests/pdf_styles.rs` has already been updated accordingly. Update the fixture assertion to `StylesDropped`; otherwise the core PDF reader suite will fail despite the intended behavior.

## Evidence

- `reader/pdf/interpreter.rs`: `unknown_color` is derived only from `Paint::color.is_none()` in colour operators and default colour-space setup.
- `reader/pdf/interpreter.rs`: `show` decodes font codes and `record` increments `unmapped`; glyph text is normalized and stored independently of `Glyph.color: Option<[u8; 3]>`.
- `reader/pdf/mod.rs`: `TextUnmapped` is emitted from the independent `unmapped` count, and truncation still emits `UnsupportedNode`.
- `redact/verify_text.rs`: the core check remains unavailable for `TextUnmapped | UnsupportedNode`, traverses `Inline::Styled`, and now ignores only the cosmetic `StylesDropped` warning.
- `redact_internal.rs`: any failed mandatory check yields `Failed`, and any unavailable check under complete scope yields `Unverifiable`.
- The supplied real-run log shows the sole blocker was the colour warning: PDFium text, render, image, graph, and rewrite checks passed, while core text became unavailable solely because the cosmetic warning shared `UnsupportedNode`.
- No authorization, external input, database, query, concurrency, or data-exposure path is changed.

## Unresolved Questions

- None after the stale fixture assertion is updated and the controller's requested focused and full redaction test runs pass.

Status: DONE_WITH_CONCERNS
Summary: The warning reclassification is causally and contractually sound and keeps all real text-loss and fail-closed paths intact. One existing PDF-reader fixture assertion still needs to expect `StylesDropped`.
Concerns/Blockers: Update `crates/ariad-core/tests/pdf_reader.rs` around line 225 before landing.
