# Code Review: DOCX Text-Box Import Repair

## Scope

- `crates/ariad-core/src/pandoc/docx_document_styles.rs`
- `crates/ariad-core/src/pandoc/to_ir.rs`
- `crates/ariad-core/src/pandoc/docx_style_reader.rs`
- `/tmp/p11-int-evidence/stirling-docx-diagnosis/output-2-27f9deef05f4.docx`
- `/tmp/p11-int-evidence/stirling-docx-raw-pandoc.json`
- `/tmp/p11-int-evidence/textbox-unit.log`
- `/tmp/p11-int-evidence/textbox-focused.log`
- Microsoft Open XML `TextBoxContent` documentation supplied with the review task

This was a read-only review. No build, test, package-manager, engine, network mutation, release, or Git mutation was run by the reviewer.

## Verdict

PASS. The repair is cause-aligned, narrowly scoped, and safe for public DOCX input. No blocking or informational source finding remains.

The rejected structure is valid WordprocessingML: `w:txbxContent` is a separate text story whose permitted children include paragraphs and tables. The captured Stirling artifact has SHA-256 `27f9deef05f4bdbf864707b2a7265f74b6237b895a0acac8550770f0a8562094` and contains valid text-box paragraphs in both `mc:Choice` and `mc:Fallback` branches. Rejecting any nested `w:p` or `w:r` without recognizing the text-box boundary therefore rejected a valid document.

The repair assigns ownership correctly. Pandoc remains the semantic DOCX reader and the local OOXML pass recovers formatting only where it can match Pandoc's semantic output. The local pass skips interpretation inside `w:txbxContent`, emits one existing `UnsupportedNode` warning that text-box content and formatting may be omitted, and resumes the enclosing paragraph and run unchanged. It does not fabricate drawing text or introduce a partial text-box reader.

The warning correction is required by observed behavior. The official Pandoc 3.12 `docx+styles` AST captured in `/tmp/p11-int-evidence/stirling-docx-raw-pandoc.json` contains only one paragraph, `Ghi chép cộng đồng`, while the source DOCX contains many additional text-box paragraphs in the modern and fallback drawing branches. The repair removes the fatal local-formatting rejection and retains whatever Pandoc supports; it cannot claim that all text-box content survives. `UnsupportedNode` accurately describes this semantic limitation, whereas `StylesDropped` would understate it as formatting-only loss.

## Correctness Evidence

- `text_box_depth` is set only for a WordprocessingML `txbxContent` element resolved by namespace, not by prefix or an unqualified local-name match.
- The saved depth is the enclosing stack depth. Both normal end events and empty elements clear it at that exact boundary; nested elements cannot clear it early.
- While inside the text box, inner `p`, `r`, text, property, table, and end events cannot start, finish, or mutate the enclosing local-formatting state.
- The parser regression demonstrates that text before and after a text box remains one bold outer run, that inner italic formatting does not contaminate it, that the following ordinary paragraph remains available, and that an empty text box behaves the same way.
- The host regression requires successful real DOCX import, retention of the following ordinary paragraph and the document's established paragraph/character/table styles, and the explicit unsupported-text-box warning. It deliberately makes no assertion that Pandoc preserves or omits the drawing text.
- Outside `txbxContent`, the existing nested-paragraph and nested-run rejection remains active. Style parsing and `basedOn` validation are in the separate unchanged style-definition parser.
- `to_ir_with_media` still maps Pandoc semantic content first, applies local formatting second, resolves media, finalizes identities, and validates the complete IR document.

## Safety Evidence

Skipping local style interpretation does not skip XML parsing or resource accounting:

- The complete `word/document.xml` member remains subject to the existing decompressed-byte and IR-byte cap.
- Every start or empty element, including elements inside text boxes and both markup-compatibility branches, counts against the node cap.
- Every nested element still counts against `max_nesting_depth`.
- Every WordprocessingML paragraph, including skipped text-box paragraphs, counts against `max_blocks`; the new raw paragraph counter prevents skipped stories from bypassing the previous finalized-paragraph check.
- Attributes are still decoded and duplicate Word attributes are still rejected.
- General references inside text-box text are still resolved through the allowlisted XML references; unknown references fail. DTDs and CDATA remain rejected globally.
- The parser still requires a complete, closed document root and stack.

The initial private unit run passed all three intended structural tests: surrounding/empty text-box state, paragraph/depth/entity limits, and unrelated malformed nesting. The separate broader filtered log selected zero tests, but `/tmp/p11-int-evidence/textbox-unit.log` directly ran the three library tests. The controller was rerunning those tests after changing the expected warning code to `UnsupportedNode`.

## Contract and Scope

`UnsupportedNode` is an existing stable warning code and accurately reports the empirically observed possibility that Pandoc omits text-box content as well as formatting. The message is specific and does not promise preservation that the official Pandoc AST disproves. No enum, schema, public API, dependency, engine boundary, or conversion order changes. Normal paragraph/run/table formatting outside text-box stories continues through the existing recovery and matching path.

## Unresolved Questions

- The controller's final host suite and captured-Stirling `__ir` rerun were still pending in `/tmp/p11-int-evidence/textbox-host-final.log` during this source review. Expected evidence is successful import, retention of Pandoc's supported output, and the explicit `UnsupportedNode` warning; complete text-box-content preservation is outside this repair's contract.

Status: DONE
Summary: The repair recognizes valid `w:txbxContent` stories without contaminating outer formatting state, honestly warns that Pandoc may omit their content and formatting, and preserves all byte, node, depth, paragraph, entity, and structural checks. No concrete regression was found.
Concerns/Blockers: None.
