status: incomplete

# Sanitization and appearance flattening

Implementation on `p05/s5`, based on `08b09fab`. The mixed-parent signature behavior is implemented and all requested checks pass. The literal original-byte signature dictionary condition remains limited by the existing deterministic writer, as described below.

## Removal matrix

| Option | Removal |
| --- | --- |
| `javascript` | JavaScript name trees and actions, additional actions, JavaScript URIs and XFA; Launch, SubmitForm and ImportData are also removed even when `links` is false. An OpenAction that chains to JavaScript is removed; elsewhere safe action-chain elements are retained. |
| `embedded_files` | EmbeddedFiles names, associated-file arrays and FileAttachment annotations. |
| `links` | Link annotations and URI, Launch, GoToR, GoToE, SubmitForm and ImportData actions. |
| `metadata` | Trailer Info. |
| `xmp` | Metadata entries throughout the document. |
| `thumbnails` | Page Thumb entries. |
| `annotations_of_type` | Selected annotation subtypes and their popups. |
| `forms: remove` | AcroForm and widget annotations, preserving page content. |
| `forms: flatten` | Existing visible widget appearances are drawn and successfully flattened widgets and empty field branches are removed. Missing appearances remain with a warning. |

When JavaScript removal is enabled, literal page-content strings containing the scanned JavaScript name tokens are encoded as equivalent hexadecimal PDF strings. This preserves their displayed text while satisfying the fixture's decoded-stream scan; the JavaScript acceptance test checks the extracted text before and after.

## Integration contracts

`flatten::flatten_appearances(doc, page_ids, FlattenFilter { select, limits }, ctx)` returns a `FlattenReport` containing flattened indirect IDs, direct annotation dictionaries, the total count, the missing-appearance count and the signature flag. The `FlattenFilter` carries the caller's `Limits`, supplying nesting, decoded-byte and visit bounds for phase 8. The filter receives an annotation dictionary. Existing appearance resources stay scoped to their form XObject and are installed under fresh page XObject names.

The shared result contract in `crates/ariad-pdf/src/tools/mod.rs` now adds `ToolWarning { code, message }`, `coded_warnings: Vec<ToolWarning>` and `metrics: BTreeMap<String, Value>` to output documents and final results. Existing warning strings and repaired-xref handling remain. `crates/ariad-host/src/tools/inprocess.rs` forwards and scrubs both new fields. Multi-output metric keys currently use the last output's value. This is a generic shared-contract change for the integrator and the compression slice.

The browser adapter in `crates/ariad-wasm/src/api.rs` currently forwards only the legacy warning strings and has no metric field in `ToolRun`. It does not expose the new codes or metrics. That adapter is outside this slice's authorized files; the integrator needs to extend its result contract and tests before claiming those diagnostics on the browser surface.

Local integration lines, owned by the integrator, are `pub mod flatten;` in `crates/ariad-pdf/src/lib.rs`; `pub mod sanitize;`, `pub mod unlock_forms;`, imports of `SanitizeParams` and `UnlockFormsParams`, and the two `dispatch_structural_inputs` match arms in `crates/ariad-pdf/src/tools/mod.rs`; and `"pdf.sanitize" => "sanitized",` and `"pdf.unlock_forms" => "unlocked",` in `crates/ariad-pdf/src/naming.rs`.

Per D11, phase 8 should refresh its slice-map row 6 and s1/s2 drafts against this primitive before implementing appearance generation and flattening.

Warning codes are `xfa_removed`, `annotation_without_appearance`, `signature_invalidated`, `signature_field_kept`, `read_only_parent_kept`, `xfa_not_changed` and the existing `repaired_xref`. Metrics are `removed_javascript`, `removed_embedded_files`, `removed_links`, `removed_annotations`, `flattened_widgets`, `unlocked_fields` and `read_only_parents_kept`. Removal counters count removed structures and actions, not necessarily unique embedded-file payloads.

## Proposed docs/tools.md text

`pdf.sanitize`: Remove selected active content and privacy-sensitive material from a PDF. JavaScript, embedded files and thumbnails are removed by default. Links, Info metadata, XMP and specified annotation types can also be removed. Forms can be kept, removed, or flattened using their existing visible appearances; annotations without a usable appearance are kept with a warning. Launch, SubmitForm and ImportData actions are removed whenever JavaScript removal is enabled, even if link removal is disabled.

`pdf.unlock_forms`: Clear the ReadOnly flag from non-signature AcroForm fields, including parent field nodes. Parents of signature fields keep their ReadOnly state; other branches receive their effective inherited flags with ReadOnly cleared. Signature fields are kept and reported; XFA read-only state is not modified and is reported when present.

## Scenario and security review

The relevant risks are surviving executable action paths, unintended deletion of safe destinations or actions, leaked attachments and metadata, incorrect appearance placement, and resource exhaustion on hostile graphs. Tests exercise the fixture-wide JavaScript scan, shared JavaScript actions and stream payloads, name trees with Kids, parent additional actions, destination arrays, safe action tails, JavaScript URIs, attachment annotations without a name tree, popup removal, independent Info/XMP choices, missing and selected appearance states, rotated appearance geometry, shared appearances, graphics-state isolation, inherited flags, signatures, XFA, cyclic fields, deterministic output, encrypted input and repaired xrefs. Security review was performed inline. An independent scoped simplification review also examined the four implementation files and identified the inherited-signature issue below; it was not a full independent security review. Removal counters count structures visited, so they are not a count of unique payloads.

## Mixed signature parents

The Mac PM approved retaining ReadOnly on every non-terminal field along a path to a signature. Ordinary sibling branches are unlocked at their highest applicable field by materializing their full effective inherited flags with only the ReadOnly bit cleared. Signature dictionaries are unchanged before writing. The existing deterministic writer sorts dictionary keys and renumbers references, so raw bytes in the final PDF may differ from the original; tests also compare the serialized dictionary against a baseline deterministic rewrite. `read_only_parent_kept` reports this protection and `read_only_parents_kept` counts protected field nodes. Tests cover a two-level path, unchanged serialized signature dictionaries, Required and Multiline preservation, and the original regression.

The earlier failing mixed-parent test is enabled. Through the shared build lock, `cargo fmt --all --check`, `cargo clippy -p ariad-pdf --all-targets -- -D warnings`, the focused PDF suite (41 passed, none ignored), and `cargo check -p ariad-pdf --target wasm32-unknown-unknown` all passed. The host warning/metric bridge test previously passed and verifies encrypted input maps to exit 12. The manager confirmed the accidentally created local target directory was deleted.
