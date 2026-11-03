# Worker report: p03 g4 (decompress keep, attachment quarantine, xobject API, nup destinations)

Branch `p03/g4`, cut from `feat/pdf-structural-tools` at `d1fb28f7`. Result: all four items are fixed. One shared-file change is needed from the integrator (`run.rs`, below) for the per-file warnings to reach the tool result.

## Item 1: decompress `object_streams: "keep"` (review N5)

- **Cause (confirmed in source).** The old code cloned every non-stream object into an extra `/ObjStm` while the originals stayed as normal xref entries, so each object was written twice and the classic xref never pointed at the stream. It then rewrote every `/ObjStr` byte sequence in the whole output, which corrupts names and content such as `/ObjStrX`.
- **lopdf 0.46 check.** `Document::save_with_options` with `SaveOptions { use_object_streams, use_xref_streams }` writes real `/ObjStm` containers and a cross-reference stream, and registers compressed objects as `XrefEntry::Compressed`. So `keep` is implemented, not removed. No schema, `docs/tools.md` or parity-row change is needed for `keep`.
- **Fix** (`crates/ariad-pdf/src/tools/decompress.rs`). `save_with_object_streams` first calls `writer::save` for its side effects on the document (gc, dedup, renumbering, key sorting, deterministic `/ID`), then writes through the lopdf object-stream writer with `compression_level(0)`, so containers stay uncompressed like the rest of the output. The placeholder stream, the byte patch and the hand-written object serialiser are deleted. `keep` still applies only when the input contains an object stream.
- **Tests** (`crates/ariad-pdf/tests/decompress.rs`):
  - `decompress_object_streams_keep_emits_referenced_object_streams_and_an_xref_stream` parses the output. It checks the xref is a stream with compressed entries, each container is a direct, unfiltered `/ObjStm` whose `/N` equals its xref members, no object number is written twice, and no compressed object also has a direct copy.
  - `decompress_keep_and_expand_leave_objstr_lookalike_bytes_untouched` builds a document with `/ObjStrX` and the string `ObjStr marker`, saved with object streams and compressed content. For both modes it asserts `/ObjStrX` appears exactly twice, `/ObjStmX` never, the content string is intact, and the resource key still parses.

## Item 2: attachment extraction policy (review N8)

- **Cause.** `validate_extracted_artifact_extensions` and `publish_atomic` returned an error for any extracted file with a denied extension, so one `.exe` failed the whole extract.
- **Fix** (`crates/ariad-host/src/tools/publish.rs`):
  - A broad `RISKY_EXTRACTED_EXTENSIONS` list: the previous denylist, everything the reviewer listed, and a few more (`ade adp apk deb gadget ipa jnlp msc msp mst ps1xml psd1 rpm run scf vb ws wsc wsh`). `svg`, `html` and `htm` stay out.
  - `is_risky_extracted_name` matches the final extension case-insensitively after trimming trailing dots and spaces, so `x.pdf.exe`, `RUN.EXE` and `run.exe. ` are caught while `x.exe.pdf` is not.
  - `publish_atomic` now publishes a risky extracted file under `<name>.bin` with reported format `bin`, instead of failing. The file name stays sanitised, and the file is created through `promote_into`, which never sets an executable bit.
  - `extracted_quarantine_warnings(&staged)` returns one warning per renamed file.
- **Tests:**
  - Unit tests in `publish.rs`: `risky_extracted_names_match_case_insensitively_with_double_extensions_and_trailing_dots` and `quarantine_appends_bin_only_to_risky_extracted_names_and_warns_per_file`.
  - `tool_publish_atomic.rs`: `run_tool_attachments_extract_quarantines_risky_types_without_failing` adds and extracts `a.svg`, `normal.pdf`, `run.exe`, `script.py`, `shortcut.LNK` and `x.pdf.exe`. It checks the extract succeeds, exactly the four risky ones get `.bin` (`.LNK` arrives as `.lnk`; the staging step already lowercases extensions), bytes are unchanged, there is no executable bit on Unix, and no stray file is left.
  - `publish_atomic_quarantines_risky_extracted_files_and_reports_one_warning_each` covers the warnings at the publish API.

## Item 3: public `DecodeBudget` (xobject Low)

- `DecodeBudget` is defined in `crates/ariad-pdf/src/document.rs`, which is not on my file list, so the edit there is a deviation: the struct is now `pub` and `from_limits` is `pub` with documentation. `charge` and `decode` stay `pub(crate)`. Both `#[allow(private_interfaces)]` attributes in `xobject.rs` are removed.
- Test: `crates/ariad-pdf/tests/xobject_api.rs` (new) calls `page_to_form_xobject` and `page_to_form_xobject_in_place` from outside the crate, and checks the budget rejects content larger than its limit.

## Item 4: nup destinations (nup Low)

- `crates/ariad-pdf/src/tools/nup.rs` no longer keeps the destination tail. Every preserved outline destination becomes `[<sheet page> /Fit]`. This code is shared by booklet, divide_pages and posterize through `finalize_layout_document`, so those tools get the same behaviour; they all re-lay out pages.
- Test: the existing nup test is renamed to `nup_outline_destinations_fit_the_sheet_ignore_non_goto_and_handle_inline_or_corrupt_streams`. It now asserts the destination is exactly `[page /Fit]`, the page is a real page of the output, and no `/XYZ` survives.

## Commands

All run through `build-lock.sh`.

| Command | Result |
|---|---|
| `cargo fmt --all --check` | PASS |
| `cargo clippy --locked -p ariad-pdf -p ariad-core -p ariad-host --all-targets -- -D warnings` | PASS |
| `cargo test --locked -p ariad-pdf --test decompress` | PASS, 5 passed |
| `cargo test --locked -p ariad-pdf --test nup` | PASS, 3 passed |
| `cargo test --locked -p ariad-pdf --test xobject_api` | PASS, 2 passed |
| `cargo test --locked -p ariad-pdf --lib` | PASS, 14 passed |
| `cargo test --locked -p ariad-host --features test-probe --lib` | PASS, 166 passed |
| `cargo test --locked -p ariad-host --features test-probe --test tool_publish_atomic` | PASS, 17 passed (re-run after final edits) |

## Deviations and honest notes

- I wrote each regression test together with its fix. I did not run them against the old code first, so I have not observed them failing before the fix.
- `crates/ariad-pdf/src/document.rs` was edited (visibility and docs only).
- I ran one bare `cargo fmt --all` outside the lock; it only reformatted files in this change.
- The warnings reach the tool result only after the integrator applies the `run.rs` change. I verified it by applying it temporarily, running the extract test with a warning-count assertion (4 renamed files, 4 warnings, passed), and then reverting both. The committed host test does not assert warnings, so it passes with or without the `run.rs` change.
- An explicit single-file `-o name.exe` extract target is still rejected by the preflight in `run.rs` (the caller chose that path). Only extractions into a directory or default names are quarantined.
- Names that sanitisation turned into `run.exe_` (a trailing space becomes `_`) are not treated as risky, because no platform runs them as `.exe`.

## Shared-file changes for the integrator

1. `crates/ariad-host/src/tools/run.rs`: apply the diff below. It makes `publish_tool_outputs` return the quarantine warnings and adds them to `ToolOutcome.warnings` with code `attachment_quarantined`.
2. `docs/tools.md` line 347 says the attachment's "own name and extension are kept". Replace the sentence after the path with: "The attachment's own name and extension are kept and its bytes are written unchanged, except that executable, script, shortcut and installer types (for example `.exe`, `.py`, `.lnk`, `.sh`, `.pkg`) get `.bin` appended to the file name, with one warning per renamed file. Files never get an executable bit."
3. No schema, parity-row or registry change: `object_streams` keeps `keep` and `expand`.

```diff
diff --git a/crates/ariad-host/src/tools/run.rs b/crates/ariad-host/src/tools/run.rs
index d08eb9a3..d7ec7505 100644
--- a/crates/ariad-host/src/tools/run.rs
+++ b/crates/ariad-host/src/tools/run.rs
@@ -40,10 +40,11 @@ use crate::{
         inprocess::{self, INPROCESS_PDF_ENGINE},
         publish::{
             PublishError, PublishedArtifact, StagedArtifact, default_multi_artifact_name,
-            default_single_artifact_name, ensure_no_parent_traversal, is_safe_extracted_extension,
-            open_confined_output_dir, publish_atomic, render_naming_template,
-            validate_artifact_extensions, validate_extracted_artifact_extensions,
-            validate_output_path_extension, validate_target_filename,
+            default_single_artifact_name, ensure_no_parent_traversal,
+            extracted_quarantine_warnings, is_safe_extracted_extension, open_confined_output_dir,
+            publish_atomic, render_naming_template, validate_artifact_extensions,
+            validate_extracted_artifact_extensions, validate_output_path_extension,
+            validate_target_filename,
         },
         secrets::{SecretBag, SecretScrubber},
     },
@@ -705,7 +706,7 @@ fn run_tool_inner(
     };
 
     // 11. Plan and execute atomic multi-artifact publication.
-    let published = publish_tool_outputs(
+    let (published, publish_warnings) = publish_tool_outputs(
         &workspace,
         &spec,
         effective_output_format,
@@ -724,13 +725,18 @@ fn run_tool_inner(
     let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
     let mut metrics = raw_exec.metrics;
     metrics.insert("elapsed_ms".to_owned(), json!(elapsed_ms));
+    let mut warnings = raw_exec.warnings;
+    warnings.extend(publish_warnings.into_iter().map(|message| ConvertWarning {
+        code: "attachment_quarantined".to_owned(),
+        message,
+    }));
 
     Ok(ToolOutcome {
         tool: spec.id.to_owned(),
         version: spec.version.to_owned(),
         engine: spec.engine.to_owned(),
         artifacts: published,
-        warnings: raw_exec.warnings,
+        warnings,
         metrics,
         inline_json: raw_exec.inline_json,
     })
@@ -1094,9 +1100,9 @@ fn publish_tool_outputs(
     allowed_dirs: Option<&AllowedDirs>,
     overwrite: bool,
     fail_at_index: Option<usize>,
-) -> Result<Vec<PublishedArtifact>, ToolError> {
+) -> Result<(Vec<PublishedArtifact>, Vec<String>), ToolError> {
     if raw_artifacts.is_empty() {
-        return Ok(Vec::new());
+        return Ok((Vec::new(), Vec::new()));
     }
 
     let first_input = inputs
@@ -1109,12 +1115,12 @@ fn publish_tool_outputs(
         .unwrap_or("document");
 
     let (dest_dir, explicit_single_filename): (PathBuf, Option<String>) = match output {
-        OutputTarget::None => return Ok(Vec::new()),
+        OutputTarget::None => return Ok((Vec::new(), Vec::new())),
         OutputTarget::DefaultBesideInput => {
             // Inline-JSON actions (`pdf.info`, `outline export`, `attachments list`, `portfolio list`, `layers list`)
             // without `-o` emit structured JSON inline without writing a file next to input.
             if (spec.id == "pdf.info" || emits_json) && naming_template.is_none() {
-                return Ok(Vec::new());
+                return Ok((Vec::new(), Vec::new()));
             }
             let parent = first_input
                 .parent()
@@ -1200,7 +1206,12 @@ fn publish_tool_outputs(
         validate_artifact_extensions(effective_output_format, &staged)?;
     }
 
-    Ok(publish_atomic(
+    let quarantine_warnings = if extracts_attachments {
+        extracted_quarantine_warnings(&staged)
+    } else {
+        Vec::new()
+    };
+    let published = publish_atomic(
         workspace,
         &staged,
         &dest_dir,
@@ -1208,7 +1219,8 @@ fn publish_tool_outputs(
         allowed_dirs,
         overwrite,
         fail_at_index,
-    )?)
+    )?;
+    Ok((published, quarantine_warnings))
 }
 
 static COMPILED_PARAM_SCHEMAS: LazyLock<RwLock<HashMap<String, Arc<jsonschema::Validator>>>> =
```

## Skills used

I did not invoke the `ak:debug`, `ak:fix` or `ak:security` skills. I applied their approach by hand: read the cause in source, then fix, then add a hostile-input test.

## Unresolved questions

- Should an explicit `-o file.exe` extract target also be quarantined to `.bin`, or keep failing in preflight?
