# Phase 3 structural PDF tools: code and security review

- Branch: `feat/pdf-structural-tools` (worktree `01-AriadShift-wt-p03-structural`), diff `origin/dev...0b9a6ee` (6 commits, 98 files changed, +23,551 / -179 LOC).
- Scope: the five focus areas in the brief. Read-only review of committed state at `0b9a6ee`.
- Skills: `/ak-code-review` (correctness, surface parity, duplication, and test quality pass) and `/ak-security` (untrusted PDF parsing, DoS/OOM amplification, `Limits`/`DecodeBudget`, and path/confinement pass). Both passes completed.

## Findings, ranked

### Critical

**C1. Host and WASM dispatchers hardcode `output_format: "pdf"`, breaking JSON-emitting actions (`outline export`, `attachments list`, `portfolio list`, `layers list`) and corrupting multi-format attachment/portfolio `add`, `create`, and `extract`.**
`crates/ariad-core/src/tools/specs/organize_document.rs:84` (`pdf.outline`), `:99-100` (`pdf.attachments`), `:147-148` (`pdf.portfolio`), and `:165` (`pdf.layers`); `crates/ariad-host/src/tools/inprocess.rs:162-235`; `crates/ariad-host/src/tools/run.rs:864-866, 1004, 1039-1116`; `crates/ariad-pdf/src/tools/mod.rs:116-123`; `crates/ariad-pdf/src/tools/attachments.rs:212-220`; `crates/ariad-wasm/src/api.rs:701-710`.
- **Failure A — JSON-emitting actions write JSON into `.pdf` files and reject `-o <file>.json`:**
  When `action` is `"export"` (`pdf.outline`) or `"list"` (`pdf.attachments`, `pdf.portfolio`, `pdf.layers`), `ariad_pdf::tools::run_tool` returns `tool_output.json = Some(...)` and a single artifact with `format: "json"` (e.g. `foo.outline.json`). `inprocess.rs:193` ignores `artifact.format` and passes `spec.output_format` (`"pdf"`) to `sanitize_staged_artifact_name`, staging the JSON payload as `foo.outline.json.pdf` with reported format `"pdf"`. Then `run.rs:1040` only exempts `spec.id == "pdf.info"` from default file publication, so `ashift tool pdf.outline in.pdf --param action=export` writes JSON text into `in.outline.pdf` on disk. Passing `-o outline.json` fails at `preflight_output_target` (`run.rs:1004`) with `Confinement("output path 'outline.json' extension does not match target format 'pdf'")`.
- **Failure B — `pdf.attachments` (`add`/`extract`) and `pdf.portfolio` (`create`/`extract`) reject non-PDF attachments, re-serialize attached PDFs, and rename extracted files to `.pdf`:**
  1. `PDF_ATTACHMENTS_SPEC` (`organize_document.rs:99`) and `PDF_PORTFOLIO_SPEC` (`:147`) declare `input_formats: &["pdf"]`, and `dispatch_structural` (`crates/ariad-pdf/src/tools/mod.rs:116-123`) calls `open_for_mutation` (`lopdf` parser) on every input. Passing a `.csv`, `.txt`, `.png`, or `.json` file to `pdf.attachments --param action=add` or `pdf.portfolio --param action=create` fails with `wrong_input` in the host/WASM format gate or `Malformed("the file is not a readable PDF")` in `open_for_mutation`—even though `attachments.rs:595-607` implements MIME detection for CSV, TXT, Markdown, JSON, HTML, XML, PNG, and JPEG. Worse, for PDF attachments, `collect_attachment_payloads` (`attachments.rs:214`) runs `writer::save(&mut cloned, ctx)` instead of embedding the raw input bytes, altering the attached file's bytes and SHA-256 digest.
  2. On `action = "extract"`, `extract_embedded_files_outputs` (`attachments.rs:278-281`) returns `OutputDoc`s with `format` set to the attachment's extension (`"csv"`, `"txt"`, `"bin"`). `inprocess.rs:223-233` forces the staged extension and reported format to `spec.output_format` (`"pdf"`), and `run.rs:1104-1106` overwrites the extracted attachment filename with `default_single_artifact_name` / `default_multi_artifact_name` (`<stem>.attachments-001.pdf`), destroying both the filename and extension of every extracted file. In `ariad-wasm` (`crates/ariad-wasm/src/api.rs:701-710`), `tool_output_format_meta` falls back to `("pdf", "application/pdf", "pdf")` for any format outside `json`/`pdf`/`zip`/`md`/`html`, serving extracted `.csv`/`.txt`/`.png` attachments as `application/pdf`.
- **Failure C — Host input staging erases original input filenames before calling `ariad-pdf`:**
  `run.rs:864-866` stages inputs as `input-0.pdf`, `input-1.pdf`, ..., and `inprocess.rs:162-167` sets `InputDoc.name` from `staged_path.file_name()`. Consequently, across CLI, MCP, and Desktop:
  - `pdf.merge` (`outline = "per-file"`, `merge.rs:196`) titles every top-level file bookmark `"input-0"`, `"input-1"` instead of the input file stems;
  - `pdf.attachments` (`action = "add"` without `names`, `attachments.rs:204`) embeds files as `"input-1.pdf"`;
  - `pdf.portfolio` (`action = "create"`, `portfolio.rs:86`) names portfolio entries `"input-0.pdf"`, `"input-1.pdf"`.
- **Fix:**
  1. Preserve `artifact.format` and `artifact.name` in `inprocess.rs`. When `tool_output.json.is_some()` and no `-o` target was given, suppress default file publication in `run.rs:1040` (not just for `pdf.info`), and allow `.json` in `preflight_output_target` when the tool action emits JSON.
  2. Pass raw `InputDoc` bytes into `attachments::run` and `portfolio::run` without parsing attachment inputs through `open_for_mutation` or re-saving them via `writer::save`, and widen input format checks for attachment/portfolio payloads. In `run.rs:1103-1115`, preserve the sanitized artifact filename and format returned by `extract` instead of renaming to `<stem>.attachments-001.pdf`; in `ariad-wasm/src/api.rs:701-710`, map extracted extensions to their actual MIME types (defaulting to `application/octet-stream`).
  3. Pass the original confined input filename alongside `staged_path` in `StagedInput` so `InputDoc.name` retains the user's filename stem.

---

### High

**H1. Unbounded recursion and exponential DAG traversal on indirect object references cause stack overflows (`SIGSEGV`) or CPU hangs in-process and in WASM.**
`crates/ariad-pdf/src/xobject.rs:551-572, 733-749`; `crates/ariad-pdf/src/tools/attachments.rs:435-459`; `crates/ariad-pdf/src/tools/outline.rs:104-135, 164-199, 220-295, 338-393`; `crates/ariad-pdf/src/tools/extract_pages.rs:411-428, 988-1047`; `crates/ariad-pdf/src/tools/merge.rs:612-648`; `crates/ariad-pdf/src/tools/reorder.rs:596-648`.
`load_bounded` caps inline token nesting (`max_nesting = 256`), not indirect reference cycles (`10 0 obj 10 0 R endobj`) or shared DAG nodes (`10 0 obj << /Kids [11 0 R 11 0 R ...] >>`).
- **Failure A — Immediate stack overflow (`SIGSEGV`) on cyclic or deep indirect reference chains:**
  - `xobject::resolve_number` (`xobject.rs:733-740`) and `xobject::resolve_integer` (`:742-749`) follow `Object::Reference(id)` with no depth limit or visited set. A 1-page PDF with `/MediaBox [10 0 R 0 612 792]` or `/Rotate 10 0 R` where `10 0 obj` is `10 0 R` crashes `rotate`, `crop`, `scale`, `single_page`, `rotate_content`, `nup`, `booklet`, `overlay`, `divide_pages`, and `posterize`.
  - `collect_name_tree_filespecs` (`attachments.rs:435-459`) recurses on `/Kids` with no depth limit, no visited set, and no `ctx.check()`. A 2-node `/Kids` cycle in `/Names /EmbeddedFiles` crashes `pdf.attachments` and `pdf.portfolio`.
  - `lookup_name_tree` (`outline.rs:281-295`) and `resolve_dest_page` / `resolve_named_dest_page` (`outline.rs:220-264`) recurse on `/Kids`, named destinations (`/Dests` or `/Names /Dests` mapping `/A -> (/A)`), and action dicts with no depth limit or visited set.
  - `read_outline_siblings` (`outline.rs:164-199`), `nodes_to_json` (`:104-117`), `validate_outline_nodes` (`:119-135`), and `build_outline_siblings` (`:338-393`) recurse on `/First` / `children` with no hierarchy depth cap.
  - `SelectiveCopier::copy_id` (`extract_pages.rs:411-428`) and `ObjectCopier::copy_id` (`xobject.rs:551-572`) recurse on the call stack per indirect reference without a depth limit or `ctx.check()` (unlike `gc::collect_document`, which uses an explicit `VecDeque`). A chain of 20,000 indirect objects (well below `MAX_OBJECTS = 2,000,000`) overflows the thread stack.
- **Failure B — Exponential CPU hang on 32-level diamond DAGs:**
  `collect_name_tree_entries` (`merge.rs:612-648`), `collect_page_label_nums` (`extract_pages.rs:988-1047`), and `retain_acroform_field` (`reorder.rs:596-648`) check `depth < 32` / `depth < 64` but do not track visited `ObjectId`s or call `ctx.check()`. A 32-level DAG where each `/Kids` array repeats the next child 4 times causes `4^32` recursive visits on a few-KB file, hanging the host or MCP server thread.
- **Fix:** Bound `resolve_number` / `resolve_integer` to 8 reference hops. Add a `visited: BTreeSet<ObjectId>`, a `MAX_TREE_DEPTH = 64` cap, and `ctx.check()?` to every `/Kids`, `/Dests`, `/Outlines`, and `/AcroForm` traversal. Convert `SelectiveCopier` and `ObjectCopier` to an iterative worklist or enforce a recursion depth limit with `ctx.check()`.

**H2. Unbounded output page and tile amplification in `pdf.posterize`, `pdf.divide_pages`, `pdf.merge`, `pdf.extract_pages`, and `pdf.split` causes OOM aborts, plus a scale-shrinkage bug in `pdf.posterize`.**
`crates/ariad-pdf/src/tools/posterize.rs:55-68, 137-155`; `crates/ariad-pdf/src/tools/divide_pages.rs:53-55`; `crates/ariad-pdf/src/tools/merge.rs:38-48`; `crates/ariad-pdf/src/tools/extract_pages.rs:64-75, 269-286`; `crates/ariad-pdf/src/tools/split.rs:57-97, 238-317`.
- **Failure A — `pdf.posterize` unbounded `cols * rows` allocation and loop (`posterize.rs:68, 137-150`):**
  When `params.tiles` is passed, `cols` and `rows` are capped at `16`. When `params.scale` is used (including the default `scale = 2.0`), `cols` and `rows` are computed from `(visual_w * scale / sheet_w).ceil() as u32` with **no upper bound**. `CustomPaperSize` (`crates/ariad-core/src/tools/params/common.rs:103`) accepts any `w > 0.0` up to `1_000_000.0` (e.g. `w: 0.01, h: 0.01`), and a source page can have `/MediaBox [0 0 100000 100000]` with `scale: 64.0`. That produces `cols = 640_000_000`, `rows = 640_000_000`, and `Vec::with_capacity(selected_set.len() * (cols * rows) as usize)` at `posterize.rs:68` immediately aborts the process with an OOM allocation panic.
- **Failure B — `pdf.posterize` silently shrinks the requested `scale` when `crop_marks` or `overlap_mm` is enabled (`posterize.rs:144-154`):**
  At `:145-146`, `cols` and `rows` divide `visual_w * s` by the full `sheet_w` / `sheet_h` instead of the non-overlapping printable advance (`step_x = tile_w - overlap_pt`, where `tile_w = sheet_w - 2 * mark_margin`). Then `:150-154` recomputes `effective_scale` to fit into those `cols × rows` printable tiles:
  `effective_scale = (total_cov_w / visual_w).min(total_cov_h / visual_h)`.
  Input: an A4 page (`595.28 × 841.89 pt`) with default params (`sheet: "A4"`, `scale: 2.0`, `overlap_mm: 10.0`, `crop_marks: true`). `cols = 2, rows = 2`, `tile_w = 559.28`, `overlap_pt = 28.35`, `total_cov_w = 2 * 559.28 - 28.35 = 1090.21 pt`, so `effective_scale = 1090.21 / 595.28 = 1.831` instead of `2.0`. Even worse, a page slightly wider than half a sheet (e.g. `300 pt` wide on A4 with `scale: 1.9`) gets `cols = ceil(570 / 595.28) = 1`, which shrinks `effective_scale` down to `0.93` (smaller than the original page!).
- **Failure C — Missing `MAX_OUTPUT_PAGES` / `limits.max_pages` checks across page-expanding tools:**
  Only `reorder.rs:53`, `alternate_mix.rs:64`, and `insert_blank.rs:78` check `MAX_OUTPUT_PAGES` (`100_000`). `extract_pages.rs:64-75` (where `DuplicatePolicy::KeepInOrder` allows `"all,all,..."` to multiply page count arbitrarily), `merge.rs:38-48`, `divide_pages.rs:53-55`, `posterize.rs:68`, and `booklet.rs:68` never check `MAX_OUTPUT_PAGES` or `limits.max_pages`.
- **Failure D — `pdf.split` clones catalog `/Names` (`/EmbeddedFiles` attachments) into every output part (`extract_pages.rs:269-286`, `split.rs:57-97`):**
  `build_extracted_doc` deep-copies the catalog `/Names` and `/AF` dictionaries into every extracted document. Splitting a 500-page PDF that holds a 10 MiB attachment with `mode: "every", every: 1` duplicates the 10 MiB attachment into all 500 `OutputDoc` buffers in memory (~5 GiB RSS).
- **Fix:**
  1. In `posterize.rs`, compute `cols = ((visual_w * s - overlap_pt) / step_x).ceil()` and `rows = ((visual_h * s - overlap_pt) / step_y).ceil()`, preserve `effective_scale = s` in `scale` mode, enforce a minimum custom paper size (`>= 36.0 pt`, matching `crop.rs:14` `MIN_BOX_PT`), and cap `cols` and `rows` at `MAX_TILES = 16` in both modes.
  2. Enforce `limits.max_pages.unwrap_or(MAX_OUTPUT_PAGES)` in `extract_pages`, `merge`, `divide_pages`, `posterize`, and `booklet` (and cap `PageRange::expand` output length).
  3. Strip `/EmbeddedFiles` and `/AF` when `build_extracted_doc` is called from `pdf.split` (or copy only page-referenced attachments).

**H3. Unbounded stream decompression, double decompression, and `Limits` bypasses in XObject conversion, layer removal, attachments, portfolio, and outline heading detection.**
`crates/ariad-pdf/src/xobject.rs:536-594, 661-711`; `crates/ariad-pdf/src/tools/layers.rs:416`; `crates/ariad-pdf/src/tools/nup.rs:239-280`; `crates/ariad-pdf/src/tools/scale.rs:72-118`; `crates/ariad-pdf/src/tools/attachments.rs:171-176`; `crates/ariad-pdf/src/tools/portfolio.rs:49`; `crates/ariad-pdf/src/tools/outline.rs:399`.
- **Failure A — Unbounded decompression in `layers.rs` and `xobject.rs`:**
  - `layers::strip_page_layer_content` (`layers.rs:416`) calls `doc.get_page_content(page_id)`, which in `lopdf` calls `decode_filters(None)` (no size limit) and never charges `DecodeBudget`. A single Flate bomb in a page content stream OOMs `pdf.layers` (`action = "remove"`).
  - `xobject::collect_page_content_bytes` (`xobject.rs:661-711`) calls `stream.decompressed_content()` (unbounded) and concatenates all streams into `combined: Vec<u8>`. Callers (`scale`, `single_page`, `rotate_content`, `nup`, `booklet`, `overlay`, `divide_pages`, `posterize`) first call `charge_page_contents` (`scale.rs:72-118`) or `charge_page_streams` (`nup.rs:239-280`), which decompresses every stream once into a temporary `Vec<u8>` via `budget.decode(stream)` and drops it, and then `page_to_form_xobject` decompresses every stream a **second** time with no limit. Furthermore:
    1. `nup::charge_page_streams` (`nup.rs:270`) ignores inline `Object::Stream` inside `/Contents` arrays, so inline streams in `nup`, `booklet`, `overlay`, `divide_pages`, and `posterize` bypass `DecodeBudget` completely and are decompressed unboundedly at `xobject.rs:703`.
    2. When a stream has a filter that fails `decompressed_content_with_limit` with a non-memory error, `budget.decode` returns `Ok(None)` without charging (`document.rs:128`), and `collect_page_content_bytes` (`xobject.rs:688, 703`) falls back to `stream.content.clone()`, splicing raw compressed bytes directly into an uncompressed Form XObject stream and corrupting the output PDF.
    3. `ObjectCopier` (`xobject.rs:536-594`) copies `/Resources` (including nested Form/Image `/XObject` streams) without charging `DecodeBudget`.
- **Failure B — Hardcoded `Limits` ignore caller/cloud limits:**
  - `attachments::max_attachment_bytes` (`attachments.rs:171-176`) hardcodes `Limits::cloud().max_asset_bytes` (50 MiB) instead of reading `doc.limits().max_asset_bytes`.
  - `portfolio::run` (`portfolio.rs:49`) and `outline::detect_heading_outlines` (`outline.rs:399`) hardcode `&Limits::local()`, bypassing caller-supplied `Limits::cloud()` in WASM/server contexts (and `ariad_core::reader::pdf::read` in `outline.rs:399` does not receive or check `ToolCtx` cancellation/deadline).
- **Fix:** Pass `&mut DecodeBudget` directly into `page_to_form_xobject`, `page_to_form_xobject_in_place`, and `strip_page_layer_content` so each content stream is decoded once via `budget.decode(stream)?` and reused; return `PdfError::Malformed` instead of splicing raw `stream.content` when a filtered stream fails to decode. Store `Limits` on `PdfDoc` (or `ToolCtx`) and propagate it to `attachments`, `portfolio`, and `outline::detect_heading_outlines`.

---

### Medium

**M1. `pdf.overlay` applies wrong rotation on rotated base pages and drops non-XObject page `/Resources`.**
`crates/ariad-pdf/src/tools/overlay.rs:81-157`.
- **Failure:** Unlike `scale.rs:148` and `rotate_content.rs:94`, `overlay.rs` never calls `materialize_inferred_rotations` before converting pages and never resets `/Rotate` to `0` on `base_page_id` after `page_to_form_xobject_in_place` (`overlay.rs:152-156` only writes `/Resources` and `/Contents`). When a base page has `/Rotate 90`:
  1. `base_form` captures `base_form.rotation = 90` and unrotated `crop_box`;
  2. `overlay.rs:100` sets `relative_rotation = (ov_form.rotation - base_form.rotation).rem_euclid(360)` (`270°`) and fits `ov_form` into the unrotated `base_form.crop_box`;
  3. `base_page_id` still retains `/Rotate 90` on the `/Page` dictionary, so PDF viewers rotate the entire composed page by 90° while the overlay XObject was counter-rotated by 270°.
- **Fix:** Materialize inferred rotations on both `base_doc` and `overlay_doc`. Either keep `/Rotate` on `base_page_id` and rotate `ov_form` into `base_form`'s user space by `ov_form.rotation` only, or normalize `base_page_id` to unrotated visual dimensions (`Rotate = 0`, swapping width/height when `base_form.rotation` is 90/270 and applying `placement_matrix` to `FmBase`).

**M2. `pdf.decompress` completely ignores `params.object_streams` (`ObjectStreamsMode::Keep`).**
`crates/ariad-pdf/src/tools/decompress.rs:22-98` vs `crates/ariad-core/src/tools/params/decompress.rs:28`.
- **Failure:** `DecompressParams` exposes `object_streams: ObjectStreamsMode` (`"expand"` default, `"keep"` option), documented in `docs/parity-matrix.md` and `DecompressParams`. `decompress::run` (`decompress.rs:22`) takes `_params: &DecompressParams` (unused!) and unconditionally calls `gc::collect_document(inner, ctx)` (`:57`), which deletes all `/Type /ObjStm` objects (`gc.rs:51-57`). Passing `--param object_streams=keep` has no effect.
- **Fix:** Either honour `ObjectStreamsMode::Keep` (skipping `/ObjStm` stripping when `"keep"` is requested) or remove the dead `"keep"` variant from `DecompressParams` and the parity docs if `lopdf` always expands object streams on load.

**M3. `pdf.metadata` writes raw UTF-8 without a BOM into `/Info` literal strings (corrupting non-ASCII metadata), drops `PDFDocEncoding` strings on read, and writes empty `/Info` + XMP objects when `remove` clears all fields.**
`crates/ariad-pdf/src/tools/metadata.rs:121-136, 203-222, 291-325`.
- **Failure:**
  1. `set_opt_literal` (`metadata.rs:318-325`) and custom field writing (`:301-306`) write `Object::String(val.as_bytes().to_vec(), StringFormat::Literal)` without a UTF-16BE `FE FF` BOM (or PDF 2.0 `EF BB BF` UTF-8 prefix). Per ISO 32000-1 §7.9.2.2, PDF 1.x viewers interpret BOM-less byte strings in `/Info` as `PDFDocEncoding`, turning any non-ASCII title/author (e.g. Vietnamese or accented names) into mojibake in `/Info` while XMP has UTF-8. (`outline.rs:536` and `page_labels.rs:133` already use `encode_pdf_string` with UTF-16BE BOM for non-ASCII strings.)
  2. `object_to_string` (`metadata.rs:203-222`) only checks UTF-16BE BOM and `std::str::from_utf8`, returning `None` for existing `/Info` strings containing `PDFDocEncoding` bytes `0x80..=0xFF` (e.g. `©`, `—`, स्मार्ट quotes, Latin-1 accents), silently dropping those fields when updating another field.
  3. At `metadata.rs:121-136`, when `params.strip_all` is `false` and `params.remove` removes the last remaining metadata fields so `state.is_empty()` is `true`, execution enters the `else` branch and inserts an empty `/Info << >>` dictionary and an empty XMP `/Metadata` stream into the document. That makes `SaveOptions { omit_info_if_absent: state.is_empty(), .. }` at `:138` a no-op because `/Info` was just inserted into `doc.trailer`.
- **Fix:** Use `encode_pdf_string` (UTF-16BE with `FE FF` BOM for non-ASCII) when writing `/Info` strings, use `decode_pdf_bytes` (with `PDFDocEncoding` fallback) in `object_to_string`, and take the `strip_all_metadata(inner)` branch whenever `params.strip_all || state.is_empty()`.

**M4. `pdf.layers` (`action = "remove"`) flattens and deletes nested `/Order` arrays, leaves stale OCG refs in `/RBGroups`/`/Locked`/`/Configs`, and misses inherited `/Pages` `/Properties`.**
`crates/ariad-pdf/src/tools/layers.rs:281-296, 373-390, 463-579`.
- **Failure:**
  1. In `remove_layers` (`layers.rs:379-388`), `/OCProperties /D /Order` is rebuilt via `extract_ref_vec`, which discards any element that is not a top-level `Object::Reference` (`layers.rs:281-296`). Per ISO 32000-1 Table 4.49, `/Order` may contain nested arrays (`[(Layer Group) 10 0 R 11 0 R]`) representing hierarchical UI layer trees; removing one layer silently deletes all nested sub-arrays from `/Order`. Removed OCG references are also left dangling in `/RBGroups`, `/Locked`, and `/Configs`.
  2. `find_matching_resource_properties` (`layers.rs:463-501`) stops at the first ancestor in the `/Parent` chain that has a `/Resources /Properties` dictionary; if a `/Page` defines `/Properties << /MC0 ... >>` and inherits `/MC1` from its parent `/Pages`, `/MC1` is not recognized and its `BDC ... EMC` watermark block is not stripped. Conversely, `remove_matching_resource_properties` (`layers.rs:503-579`) only cleans `page_id`'s `/Resources` and never removes matching entries from ancestor `/Pages` `/Resources`.
- **Fix:** Recursively filter `/Order`, `/RBGroups`, `/Locked`, and `/Configs` arrays in place to preserve nested array structure. Merge `/Resources /Properties` across all `/Parent` ancestors in `find_matching_resource_properties`, and clean ancestor `/Pages` `/Resources /Properties` when all pages sharing that ancestor have had the layer removed.

**M5. Duplicated page-manipulation logic between `extract_pages.rs` and `reorder.rs` (plus `insert_blank.rs` / `nup.rs`) causes four concrete behavioural divergences (Scope Item 5).**
`crates/ariad-pdf/src/tools/extract_pages.rs:321-1104` vs `crates/ariad-pdf/src/tools/reorder.rs:146-987`.
Running `pdf.extract_pages` (`pages: "2,1"`) and `pdf.reorder` (`order: "2,1"`) on the same PDF produces materially different outputs:
1. **`/PageLabels` destroyed by `pdf.reorder` (`reorder.rs:932-987` vs `extract_pages.rs:973-1104`):**
   `extract_pages.rs` walks `/PageLabels` number trees (including `/Kids`) and synthesizes remapped `/PageLabels` ranges for arbitrary page selections and reorderings. `reorder.rs:936-949` checks if the new order is a contiguous prefix `1..=k`; if not (which is true for *every* actual reordering, e.g. `"2,1"`), `reorder.rs:942` deletes `/PageLabels` from `/Catalog` completely! In addition, `reorder.rs:956` and `insert_blank.rs:163` only read flat `/Nums` and silently delete `/PageLabels` if the number tree uses `/Kids`.
2. **Dropped `/Page` objects retained in output by `pdf.reorder` (`reorder.rs:108, 146-188`):**
   When `pdf.reorder` selects a subset of pages (e.g. `order: "1,2"` on a 10-page PDF), it removes the dropped page IDs from `/Pages /Kids` (`:167`) and relies on `gc::collect_document` (`:108`) to purge them. However, if any surviving object—such as `/Catalog /Dests`, `/Catalog /Names /Dests`, `/Catalog /OpenAction`, `/StructTreeRoot`, or an annotation `/Popup`—still holds an `Object::Reference(dropped_page_id)`, `gc::collect_document` traverses from `/Catalog` into `dropped_page_id` and retains the dropped `/Page` dictionary and all of its content streams and fonts in the output PDF. Meanwhile, `extract_pages.rs:456-458` replaces references to uncopied pages with `Object::Null`, leaving broken destination arrays `[null /XYZ 0 792 null]` inside `/Dests` / `/Names /Dests`.
3. **Duplicated pages (`"1,1"`) corrupt widget annotations in both tools in opposite ways (`reorder.rs:349-352` vs `extract_pages.rs:619-638, 710`):**
   When page 1 is duplicated (`"1,1"`), `reorder.rs:349-350` attaches the *same* widget `annot_id` to both page 1 and page 2 (so both pages share one widget object whose `/P` points only to page 1) and shallow-clones non-widget annotations (`:352`). Conversely, `extract_pages.rs:619-638` allocates a fresh widget object on the second copy of the page, but `copy_filtered_acroform` (`:710, 725`) only maps the first widget via `widget_map`, so the second page's widget is never registered in `/AcroForm /Fields`.
4. **Three incompatible `/Outlines` destination implementations (`extract_pages.rs:845-862`, `reorder.rs:782-824`, `nup.rs:535-580`):**
   - `reorder.rs` resolves named destinations via `/Catalog /Dests` (dictionary only, not `/Names /Dests`) and integer page indices, preserves `dest_tail`, and defaults tail to `[/Fit]`.
   - `extract_pages.rs` ignores named destinations and integer page indices, strips indirect `Object::Reference` elements from `dest_tail` (`:858`), and defaults tail to `[/XYZ null null null]`.
   - `nup.rs` discards `dest_tail` completely (rewriting all destinations to `[/Fit]`) and treats any `/A` dictionary containing `/D` as a GoTo action without checking `/S == /GoTo`.
- **Fix:** Consolidate `reorder`, `extract_pages`, and `delete_pages` onto one shared page-selection/remapping engine (`build_extracted_doc` with shared `/PageLabels`, `/Outlines`, `/Dests`, and `/AcroForm` remappers), and strip `/Catalog /OpenAction` and `/Dests` entries that target dropped pages instead of emitting `[null /XYZ ...]`.

---

### Low

**L1. `REPAIRED_XREF_WARNING` is duplicated or omitted depending on whether a tool is called via `run_tool` or `<tool>::run`.**
`crates/ariad-pdf/src/tools/mod.rs:498-500` vs `crates/ariad-pdf/src/tools/reorder.rs:111`, `alternate_mix.rs:171`, `insert_blank.rs:132`.
- **Failure:** `reorder::run`, `alternate_mix::run`, and `insert_blank::run` push `REPAIRED_XREF_WARNING` inside `run()`, whereas the other 22 tools rely on `finalize_tool_output` (`tools/mod.rs:498-500`) to append it when invoked through `run_tool`. Calling `extract_pages::run` or `merge::run` directly via the public Rust API on a repaired PDF returns `warnings: []`.
- **Fix:** Either push `REPAIRED_XREF_WARNING` consistently in one place or include a helper called by every tool's `run()`.

**L2. Snapshot assertions auto-create missing `.snap` files and pass, 5 test files write uncleaned files to `/tmp/ariad-p03-s4-preview`, and Phase 3 T6 CLI golden tests + fuzz target are missing.**
`crates/ariad-pdf/tests/{merge,split,extract_pages,delete_pages,nup,booklet,overlay,divide_pages,posterize,metadata,outline,attachments,page_labels,viewer_prefs,portfolio,layers}.rs`; `plans/261031-1422-next-stage-pdf-suite/phase-03-structural-pdf-tools.md:208-212, 237`.
- **Failure:**
  1. Every custom `assert_json_snapshot` helper in the 16 snapshot-using test files contains `if ... || !snap_path.exists() { fs::write(&snap_path, ...) }`, so a missing or misspelled snapshot file is silently created on disk and the test passes without review.
  2. `nup.rs:158-160`, `booklet.rs:173-175`, `overlay.rs:121-123`, `divide_pages.rs:118-120`, and `posterize.rs:82-84` unconditionally write preview PDFs to `std::env::temp_dir().join("ariad-p03-s4-preview")` on every `cargo test` run without cleaning them up.
  3. Phase 3 plan task T6 (`phase-03-structural-pdf-tools.md:208-212`) required `crates/ariad-cli/tests/tools_structural.rs` (CLI golden tests for the 25 tools) and `fuzz/fuzz_targets/pdf_tools_structural.rs`. Neither exists yet; all 25 integration tests call `ariad_pdf::tools::run_tool` directly, which is why the host-level bugs in **C1** went undetected.
  4. Phase 3 rule 5 (`phase-03-structural-pdf-tools.md:237`: per-page progress events) is not wired: `inprocess.rs:148-152, 237-241` only emits `"reading"` and `"complete"` per input file.
- **Fix:** Fail snapshot tests when `!snap_path.exists()` unless `UPDATE_SNAPSHOTS` / `INSTA_UPDATE=always` is set; gate `/tmp/ariad-p03-s4-preview` writes behind an env var or `tempfile::tempdir()`; and add `crates/ariad-cli/tests/tools_structural.rs` covering CLI execution (including JSON-output and extract actions) plus `fuzz/fuzz_targets/pdf_tools_structural.rs`.

---

## Checked and found sound

- **All 25 tool specs and parameter schemas match `docs/parity-matrix.md`:** All 25 tool IDs across Organize (8), Pages & Geometry (6), Layout & Imposition (5), and Document Structure & Metadata (6) are registered in `crates/ariad-core/src/tools/specs/` with `ToolAccess::Free`, `execution: ToolExecution::InProcess`, `min_inputs`/`max_inputs` matching the matrix, and `#[serde(deny_unknown_fields)]` on every parameter struct.
- **Deterministic serialization (`writer.rs`, `gc.rs`):** `writer::save` and `gc::collect_document` use `BTreeMap`/`BTreeSet` exclusively, normalize `/ID` deterministically from a SHA-256 of the body bytes, strip `/Prev` and `/XRefStm`, and produce byte-identical outputs across repeated runs.
- **Encrypted input handling:** `open_for_mutation` (`crates/ariad-pdf/src/tools/mod.rs:347-370`) rejects encrypted PDFs (`PdfError::Encrypted`) before any tool mutates or saves the document, so decrypted content is never written back out.
- **Artifact filename sanitization (`naming.rs`, `split.rs:348-364`, `attachments.rs:564-593`):** Bookmark slugs (`slugify_bookmark`) and extracted attachment names (`sanitize_attachment_filename`) strip `/`, `\`, and `..`, restrict characters to `[A-Za-z0-9._-]`, and prefix every generated artifact name with `{input_stem}.part-{idx:03}-` or `{input_stem}.attachment-{idx:03}.`, preventing path traversal, leading-dot hidden files, Windows reserved device names, and filename collisions across multiple extracted items.
- **No secrets or raw document payloads echoed in errors:** None of the 25 Phase 3 tools take secret parameters, and error messages report only bounded parameter values, page numbers, or structural descriptions.

Status: DONE_WITH_CONCERNS
