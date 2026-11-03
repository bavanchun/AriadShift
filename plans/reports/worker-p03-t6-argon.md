# Worker Report: CLI Golden Tests, Structural Fuzz Target, CLI Docs & WASM Size

## Skills Applied

- `/ak-scenario`: Decomposed the 25 structural PDF tools across happy-path golden runs, malformed xref recovery (`broken-xref.pdf`), encrypted input refusal (both user-locked AES-256 and owner-password-only PDFs), multi-artifact output (`pdf.split`), and deterministic two-run byte identity (`SOURCE_DATE_EPOCH=1700000000`).
- `/ak-test`: Structured integration tests in `crates/ariad-cli/tests/tools_structural.rs` with isolated `TempDir` workspaces, deterministic environment configuration, and `insta` JSON snapshot verification.
- `/ak-docs`: Updated `docs/cli.md` with one runnable `ashift tool <id>` example per structural tool (all 25 tools) and verified every command line against the built `ashift` binary.

---

## Summary of Deliverables

1. **CLI Golden Suite (`crates/ariad-cli/tests/tools_structural.rs` + snapshot `crates/ariad-cli/tests/snapshots/tools_structural__structural_tools_golden_summary.snap`)**
   - `all_twenty_five_structural_tools_golden_facts_and_deterministic_bytes`: Executes all 25 structural tools through `ashift tool <id>` on `fixtures/pdf-tools` inputs twice into separate output directories with `SOURCE_DATE_EPOCH=1700000000`, asserting:
     - Exit code `0`.
     - Exact default output filenames (`<stem>.<suffix>.pdf` and `<stem>.part-NNN.pdf`) matching `docs/tools.md` §6.
     - Structural facts via `ashift tool pdf.info <output> --json` (`page_count`, `repaired_xref == false`, `encrypted == false`, page dimensions/paper sizes, rotations, blank-page insertion flags, `outline_count`, `has_attachments`, `metadata`, `/PageLabels`, `/ViewerPreferences`, and layer removal).
     - Byte-for-byte identical output between run 1 and run 2 (`bytes1 == bytes2`) and a consolidated `insta` snapshot of artifact sizes, SHA-256 digests, and `pdf.info` summaries.
   - `structural_tool_on_broken_xref_emits_repair_warning_and_writes_valid_xref`: Runs `ashift tool pdf.rotate` on `fixtures/pdf-tools/broken-xref.pdf`, asserting exit code `0`, `warning[repaired_xref]: cross-reference table was malformed and reconstructed from object offsets` on stderr and in `--json` `warnings`, and `repaired_xref == false` with zero warnings when `pdf.info` inspects the rewritten `broken-xref.rotated.pdf`.
   - `structural_tools_reject_encrypted_input_with_exit_code_twelve`: Materializes both user-locked (`encrypted-aes256.pdf`) and owner-password-only (`encrypted-owner-only.pdf`) inputs from `fixtures/pdf-tools/*.plaintext.pdf`, invokes `ashift tool pdf.rotate`, and asserts exit code `12` (`error[encrypted_input]` referencing `pdf.decrypt`) with zero partial output files left behind.
   - `multi_output_pdf_split_lists_every_artifact_in_human_and_json_modes`: Splits `fixtures/pdf-tools/multipage-20.pdf` into 3 range parts (`1-3`, `4-8`, `9-20`), verifying all 3 artifact paths (`multipage-20.part-001.pdf` .. `multipage-20.part-003.pdf`) in human stdout and `--json` `artifacts`, plus exact per-part page counts (`3`, `5`, `12`).

2. **Structural Fuzz Target (`fuzz/fuzz_targets/pdf_tools_structural.rs`, `fuzz/Cargo.toml`, `fuzz/corpus/pdf_tools_structural/`)**
   - Added `[[bin]] name = "pdf_tools_structural"` (`required-features = ["pdf-reader"]`) to `fuzz/Cargo.toml`.
   - Implemented `fuzz/fuzz_targets/pdf_tools_structural.rs` with bounded `Limits` (`max_input_bytes: 64 KiB`, `max_pages: 8`, `max_blocks: 128`, `max_nesting_depth: 16`, `max_decompressed_bytes: 512 KiB`) and a cooperative `ToolCtx::with_deadline(Instant::now() + Duration::from_millis(500))` deadline.
   - Exercises both raw byte inputs (`InputDoc::with_limits`) with arbitrary JSON parameters and pre-opened `PdfDoc` inputs (`InputDoc::from_doc`) across all 25 structural tools with both schema-shaped and arbitrary recursive JSON values.
   - Seeded `fuzz/corpus/pdf_tools_structural/` with 8 representative fixtures (`annotations.pdf`, `broken-xref.pdf`, `incremental-update.pdf`, `mixed-sizes.pdf`, `object-streams.pdf`, `rotated-pages.pdf`, `watermark-layer.pdf`, `with-attachments.pdf`).
   - Verified compilation with `cargo check --locked --manifest-path fuzz/Cargo.toml --features pdf-reader --bin pdf_tools_structural` (**PASS**, `fuzz/Cargo.lock` unchanged). Local `cargo-fuzz` sanitizer execution is Linux-only (`just fuzz` exits on macOS with `fuzzing runs on Linux only`) and runs in Linux CI.

3. **CLI Documentation (`docs/cli.md`)**
   - Added 25 runnable `ashift tool <id>` examples (one per structural tool) under `### ashift tool` -> `#### Examples`.
   - Executed every single documented command line against `target/debug/ashift` via the build lock:

| # | Tool ID | CLI Example Command | Verification |
|---|---|---|---|
| 1 | `pdf.merge` | `ashift tool pdf.merge fixtures/pdf-tools/rotated-pages.pdf fixtures/pdf-tools/annotations.pdf --param outline=per-file --param page_labels=renumber -o out/` | **PASS** (`rotated-pages.merged.pdf`) |
| 2 | `pdf.split` | `ashift tool pdf.split fixtures/pdf-tools/rotated-pages.pdf --param mode=every --param every=2 -o out/` | **PASS** (`rotated-pages.part-001.pdf`, `rotated-pages.part-002.pdf`) |
| 3 | `pdf.extract_pages` | `ashift tool pdf.extract_pages fixtures/pdf-tools/multipage-20.pdf --param pages=1-3 -o out/` | **PASS** (`multipage-20.extracted.pdf`) |
| 4 | `pdf.delete_pages` | `ashift tool pdf.delete_pages fixtures/pdf-tools/rotated-pages.pdf --param pages=2,4 -o out/` | **PASS** (`rotated-pages.trimmed.pdf`) |
| 5 | `pdf.reorder` | `ashift tool pdf.reorder fixtures/pdf-tools/rotated-pages.pdf --param order=reverse -o out/` | **PASS** (`rotated-pages.reordered.pdf`) |
| 6 | `pdf.alternate_mix` | `ashift tool pdf.alternate_mix fixtures/pdf-tools/rotated-pages.pdf fixtures/pdf-tools/annotations.pdf --param reverse_second=true -o out/` | **PASS** (`rotated-pages.mixed.pdf`) |
| 7 | `pdf.insert_blank` | `ashift tool pdf.insert_blank fixtures/pdf-tools/rotated-pages.pdf --param after=1-1 --param count=2 --param size=same -o out/` | **PASS** (`rotated-pages.inserted.pdf`) |
| 8 | `pdf.rotate` | `ashift tool pdf.rotate fixtures/pdf-tools/rotated-pages.pdf --param pages=1-1 --param angle=90 -o out/` | **PASS** (`rotated-pages.rotated.pdf`) |
| 9 | `pdf.crop` | `ashift tool pdf.crop fixtures/pdf-tools/rotated-pages.pdf --param pages=all --param 'box={"left":36,"bottom":36,"right":576,"top":756}' --param target=crop -o out/` | **PASS** (`rotated-pages.cropped.pdf`) |
| 10 | `pdf.scale` | `ashift tool pdf.scale fixtures/pdf-tools/mixed-sizes.pdf --param pages=all --param to=A4 --param fit=contain -o out/` | **PASS** (`mixed-sizes.scaled.pdf`) |
| 11 | `pdf.single_page` | `ashift tool pdf.single_page fixtures/pdf-tools/rotated-pages.pdf -o out/` | **PASS** (`rotated-pages.single-page.pdf`) |
| 12 | `pdf.rotate_content` | `ashift tool pdf.rotate_content fixtures/pdf-tools/rotated-pages.pdf --param pages=1-1 --param angle_deg=45 --param expand=true -o out/` | **PASS** (`rotated-pages.rotated-content.pdf`) |
| 13 | `pdf.decompress` | `ashift tool pdf.decompress fixtures/pdf-tools/object-streams.pdf --param object_streams=expand -o out/` | **PASS** (`object-streams.decompressed.pdf`) |
| 14 | `pdf.nup` | `ashift tool pdf.nup fixtures/pdf-tools/rotated-pages.pdf --param per_sheet=2 --param sheet=A4 --param order=row -o out/` | **PASS** (`rotated-pages.nup.pdf`) |
| 15 | `pdf.booklet` | `ashift tool pdf.booklet fixtures/pdf-tools/rotated-pages.pdf --param sheet=A4 --param binding=left --param creep_pt=0 -o out/` | **PASS** (`rotated-pages.booklet.pdf`) |
| 16 | `pdf.overlay` | `ashift tool pdf.overlay fixtures/pdf-tools/rotated-pages.pdf fixtures/pdf-tools/watermark-layer.pdf --param overlay_pages=first --param position=over --param pages=all -o out/` | **PASS** (`rotated-pages.overlay.pdf`) |
| 17 | `pdf.divide_pages` | `ashift tool pdf.divide_pages fixtures/pdf-tools/rotated-pages.pdf --param pages=all --param cols=2 --param rows=2 --param order=row -o out/` | **PASS** (`rotated-pages.divided.pdf`) |
| 18 | `pdf.posterize` | `ashift tool pdf.posterize fixtures/pdf-tools/annotations.pdf --param 'tiles={"cols":2,"rows":2}' --param sheet=A4 --param overlap_mm=10 --param crop_marks=true -o out/` | **PASS** (`annotations.posterized.pdf`) |
| 19 | `pdf.metadata` | `ashift tool pdf.metadata fixtures/pdf-tools/rotated-pages.pdf --param 'set={"title":"Golden Title","author":"AriadShift"}' -o out/` | **PASS** (`rotated-pages.metadata.pdf`) |
| 20 | `pdf.outline` | `ashift tool pdf.outline fixtures/pdf-tools/multipage-20.pdf --param action=clear -o out/` | **PASS** (`multipage-20.outline.pdf`) |
| 21 | `pdf.attachments` | `ashift tool pdf.attachments fixtures/pdf-tools/rotated-pages.pdf fixtures/pdf-tools/annotations.pdf --param action=add -o out/` | **PASS** (`rotated-pages.attachments.pdf`) |
| 22 | `pdf.page_labels` | `ashift tool pdf.page_labels fixtures/pdf-tools/multipage-20.pdf --param 'ranges=[{"from":1,"style":"roman-lower"},{"from":5,"style":"decimal","prefix":"p-","start":1}]' -o out/` | **PASS** (`multipage-20.labeled.pdf`) |
| 23 | `pdf.viewer_prefs` | `ashift tool pdf.viewer_prefs fixtures/pdf-tools/rotated-pages.pdf --param page_layout=two-column-left --param fit_window=true -o out/` | **PASS** (`rotated-pages.prefs.pdf`) |
| 24 | `pdf.portfolio` | `ashift tool pdf.portfolio fixtures/pdf-tools/rotated-pages.pdf fixtures/pdf-tools/annotations.pdf --param action=create --param view=details -o out/` | **PASS** (`rotated-pages.portfolio.pdf`) |
| 25 | `pdf.layers` | `ashift tool pdf.layers fixtures/pdf-tools/watermark-layer.pdf --param action=remove --param 'names=["AriadShift Watermark"]' -o out/` | **PASS** (`watermark-layer.layers.pdf`) |

---

## WASM Bundle Size (`ariad_wasm_bg.wasm`)

Built with `env UV_NO_CONFIG=1 just wasm-build` (`wasm-release` profile + `wasm-bindgen 0.2.129`):

| Baseline / Stage | Size (bytes) | Delta vs Pre-Phase (`6,750,968`) | Delta vs Phase-2 (`6,944,059`) |
|---|---:|---:|---:|
| Pre-phase (`v0.2.0`) | `6,750,968` | `0` (`0.00%`) | `-193,091` (`-2.78%`) |
| Phase-2 (`pdf.info`) | `6,944,059` | `+193,091` (`+2.86%`) | `0` (`0.00%`) |
| **Current (`packages/ariad-wasm/pkg/ariad_wasm_bg.wasm`)** | **`8,352,779`** | **`+1,601,811` (`+23.73%`)** | **`+1,408,720` (`+20.29%`)** |

- **FLAG (`> 15%` growth over Phase-2)**: `ariad_wasm_bg.wasm` grew by `+1,408,720` bytes (`+20.29%`) over the Phase-2 baseline (`6,944,059` bytes), exceeding the 15% alert threshold (`7,985,667` bytes) by `367,112` bytes.

---

## Defect Uncovered During Scenario Testing (Not Modified per File-Ownership Rule)

1. **`ariad-host` hardcodes `spec.output_format` (`"pdf"`) for polymorphic non-PDF tool actions (`crates/ariad-host/src/tools/inprocess.rs` & `crates/ariad-host/src/tools/run.rs`)**
   - **Affected tools and inputs**:
     - `ashift tool pdf.outline fixtures/pdf-tools/multipage-20.pdf --param action=export -o <dir>`
     - `ashift tool pdf.attachments fixtures/pdf-tools/with-attachments.pdf --param action=list -o <dir>`
     - `ashift tool pdf.attachments fixtures/pdf-tools/with-attachments.pdf --param action=extract -o <dir>`
     - `ashift tool pdf.portfolio fixtures/pdf-tools/with-attachments.pdf --param action=list -o <dir>`
     - `ashift tool pdf.layers fixtures/pdf-tools/watermark-layer.pdf --param action=list -o <dir>`
   - **Symptom**: While `ariad-pdf` returns `OutputArtifact { format: "json", name: "<stem>.outline.json", ... }` (or extracted attachment filenames/formats) and `ariad-wasm` preserves `artifact.format` and `artifact.name`, `crates/ariad-host/src/tools/inprocess.rs` (lines 204–230) ignores `artifact.format`/`artifact.name` and constructs `StagedArtifact { format: spec.output_format.to_owned(), ... }` (`"pdf"`), and `crates/ariad-host/src/tools/run.rs` (line 1115) validates single-file `-o` extensions against `spec.output_format`. Consequently, `action=export` / `action=list` via CLI/host publishes JSON payload bytes under a `.pdf` filename (`multipage-20.outline.pdf`) or rejects `-o out.json` with exit code `9`.

2. **CLI `--param KEY=VALUE` integer coercion with string-typed `PageRange` (`crates/ariad-cli/src/main.rs`)**
   - **Symptom**: Because `--param KEY=VALUE` attempts `serde_json::from_str(VALUE)` before falling back to `Value::String`, passing a single-page `PageRange` as `--param pages=1` parses `1` as `Value::Number(1)`, which `PageRange` (expecting a JSON string) rejects with exit code `9` (`error[invalid_params]`). Range expressions that are not JSON numbers (`--param pages=1-1`, `--param pages=1-3`, `--param pages=2,4`, `--param pages=all`) or quoted JSON strings (`--param 'pages="1"'`) parse as `Value::String` and succeed.

---

## Verification Gates Run (via `build-lock.sh`)

- `cargo fmt --all --check` -> **PASS**
- `cargo clippy --locked -p ariad-cli --all-targets -- -D warnings` -> **PASS**
- `ASHIFT_PANDOC=$PWD/.tools/pandoc/bin/pandoc cargo test --locked -p ariad-cli --features ariad-host/test-probe --test tools_structural` -> **PASS** (`4 passed; 0 failed`)
- `cargo check --locked --manifest-path fuzz/Cargo.toml --features pdf-reader --bin pdf_tools_structural` -> **PASS**
- `env UV_NO_CONFIG=1 just wasm-build` -> **PASS** (`8,352,779` bytes)
- `env UV_NO_CONFIG=1 just spell` -> **PASS**
- `sh scripts/check-commits.sh origin/dev..HEAD` -> **PASS**
