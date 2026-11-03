status: complete

# Worker Report: Risky-Extension List and Layer-Removal Warning Wording (Slice r3)

Branch `p03/r3` in `/home/vchun/Codes/01-AriadShift-wt-p03-r3` (cut from `feat/pdf-structural-tools` `1c643e99`). Both items assigned to slice `r3` in `.run/coord/p03/r3.md` have been verified against the source, proven with failing regression tests first, fixed, and verified with the narrow check suite.

## Item 1: Disk-image, help and Windows app-package extensions in the risky extracted-extension list (`publish.rs`)

- **Proven cause:** `RISKY_EXTRACTED_EXTENSIONS` in `crates/ariad-host/src/tools/publish.rs` omitted container, help and installer package extensions (`iso`, `img`, `vhd`, `vhdx`, `chm`, `msix`, `msixbundle`, `appx`, `appxbundle`), allowing extracted PDF attachments with those extensions to be written under their active extensions instead of being quarantined with a `.bin` suffix.
- **Fix:** Added `"appx"`, `"appxbundle"`, `"chm"`, `"img"`, `"iso"`, `"msix"`, `"msixbundle"`, `"vhd"`, and `"vhdx"` to `RISKY_EXTRACTED_EXTENSIONS` in `crates/ariad-host/src/tools/publish.rs`. Because `is_risky_extracted_extension` and `is_risky_extracted_name` lower-case the trimmed final extension and strip trailing dots and spaces, all nine extensions are matched case-insensitively and across double extensions and trailing dots/spaces.
- **Regression tests:**
  - `tools::publish::tests::risky_extracted_names_match_case_insensitively_with_double_extensions_and_trailing_dots` (`crates/ariad-host/src/tools/publish.rs`)
  - `publish_atomic_quarantines_risky_extracted_files_and_reports_one_warning_each` (`crates/ariad-host/tests/tool_publish_atomic.rs`)

## Item 2: Truthful per-page warning when `pdf.layers remove` skips undecodable page content (`layers.rs`)

- **Proven cause:** In `crates/ariad-pdf/src/tools/layers.rs`, when `strip_page_layer_content` skipped a page because its stream filter or content operations could not be decoded, `remove_layers` still removed the page's `/Properties` entry (`remove_matching_resource_properties`) and deleted the OCG from `/OCProperties`. However, the emitted warning claimed `"layer content on this page was left unchanged"`, which was inaccurate because removing the OCG causes previously hidden (`OFF`) layer content remaining in the skipped content stream to become visible, and because the layer content itself remains on that page.
- **Fix:** Updated `strip_page_layer_content` to return `Result<bool, PdfError>` (`Ok(true)` when a page's content stream could not be decoded and was skipped) and changed the warning in `remove_layers` to `"page {page_no}: content could not be decoded and was skipped, so layer content there may remain"`.
- **Regression test:**
  - `layers_remove_skips_pages_whose_content_cannot_be_decoded_and_warns` (`crates/ariad-pdf/tests/layers.rs`)

## Verification Commands

All commands were executed through `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh`:

| Command | Result | Counts |
|---|---|---|
| `cargo fmt --all --check` | PASS | 0 diffs |
| `cargo clippy --locked -p ariad-pdf -p ariad-host --all-targets -- -D warnings` | PASS | 0 warnings |
| `cargo test --locked -p ariad-pdf --lib` | PASS | 23 passed, 0 failed |
| `cargo test --locked -p ariad-pdf --test layers` | PASS | 9 passed, 0 failed |
| `ASHIFT_PANDOC=/home/vchun/Codes/01-AriadShift/.tools/pandoc/bin/pandoc cargo test --locked -p ariad-host --features test-probe --lib` | PASS | 166 passed, 0 failed |
| `ASHIFT_PANDOC=/home/vchun/Codes/01-AriadShift/.tools/pandoc/bin/pandoc cargo test --locked -p ariad-host --features test-probe --test tool_publish_atomic` | PASS | 17 passed, 0 failed |

## Assumptions and Deviations

- **Assumptions:** Unified both stream-filter decode failure (`DecodeBudget::decode` returning `Ok(None)`) and content-operator decode failure (`lopdf::content::Content::decode` returning `Err(_)`) under the warning message `"page <N>: content could not be decoded and was skipped, so layer content there may remain"`, matching the wording specified in `.run/coord/p03/r3.md`.
- **Deviations:** None. Report named `plans/reports/worker-p03-r3-argon.md` as specified in `.run/coord/p03/r3.md`.

## Exact Shared-File Changes for the Integrator

None. All changes are self-contained within slice `r3`'s owned files.

## Skills Used

- `/ak-debug`: verified root causes in `crates/ariad-host/src/tools/publish.rs` and `crates/ariad-pdf/src/tools/layers.rs` and confirmed failing regression tests prior to applying code changes.
- `/ak-fix`: applied minimal, cause-aligned fixes to `RISKY_EXTRACTED_EXTENSIONS` and the skipped-page warning in `remove_layers`.
- `/ak-security`: verified case-insensitive, double-extension, and trailing-dot/space quarantine matching for all nine added container/package extensions (`iso`, `img`, `vhd`, `vhdx`, `chm`, `msix`, `msixbundle`, `appx`, `appxbundle`).
