# Tool Contract Specification

This document specifies the normative contract for AriadShift document tools (`ariad-core::tools`, `ariad-pdf`, `ariad-host::tools`, `ariad-engine/2`, `ariad-recipe/1`, CLI, MCP, web, and macOS surfaces).

All tool registration, parameter validation, access control, page-range parsing, and recipe definitions live in `ariad-core::tools`. Structural PDF operations run in-process inside `ariad-pdf` (including on `wasm32`), while rendering, encryption, OCR, signing, validation, and AI tools run out of process over `ariad-engine/2` orchestrated by `ariad-host`. Product surfaces (`ashift` CLI, MCP servers, `apps/web`, `apps/macos`) carry no document manipulation logic.

## Table of Contents

- [1. Architecture Boundary: Tools vs. Conversions](#1-architecture-boundary-tools-vs-conversions)
- [2. Tool Identification, Versioning, and Groups](#2-tool-identification-versioning-and-groups)
- [3. Inputs: Arity, Formats, Ordering, and Encryption Preflight](#3-inputs-arity-formats-ordering-and-encryption-preflight)
- [4. Parameters (`params`) and Large Parameter Delivery (`params_file`)](#4-parameters-params-and-large-parameter-delivery-params_file)
- [5. Page-Range Grammar](#5-page-range-grammar)
- [6. Outputs, Naming, Confinement, and Atomic Multi-Artifact Publishing](#6-outputs-naming-confinement-and-atomic-multi-artifact-publishing)
- [7. Resource Limits, Sandbox, Network Isolation, and Progress](#7-resource-limits-sandbox-network-isolation-and-progress)
- [8. Determinism](#8-determinism)
- [9. Recipes (`ariad-recipe/1`) and Untrusted Recipe Rules](#9-recipes-ariad-recipe1-and-untrusted-recipe-rules)
- [10. Tool Tiers and Access Control (`ToolAccess`)](#10-tool-tiers-and-access-control-toolaccess)
- [11. Secrets Contract (`x-ariad-secret`)](#11-secrets-contract-x-ariad-secret)
- [12. CLI Commands and Tool Exit Codes](#12-cli-commands-and-tool-exit-codes)
- [13. MCP Surface (`list_doc_tools` and `run_doc_tool`)](#13-mcp-surface-list_doc_tools-and-run_doc_tool)

---

## 1. Architecture Boundary: Tools vs. Conversions

A **conversion** transforms a single document from one format into a different target format along a planned path in the capability graph (`ashift convert`, where same-format routes and self-loops are rejected by design).

A **tool** is a typed operation:

```text
tool_id(inputs[], params) -> artifacts[]
```

- **Separate registry (AD-1)**: Tools are registered in `ariad-core::tools`, separate from the conversion capability graph (`capabilities.json`). Conversions stay routes; tools never enter the route planner.
- **In-process structural PDF crate (AD-2)**: `crates/ariad-pdf` (pure Rust built on `lopdf`, `#![forbid(unsafe_code)]`, bounded by `Limits`, fuzzed) executes structural PDF tools (`pdf.info`, `pdf.merge`, `pdf.split`, `pdf.rotate`, `pdf.reorder`, `pdf.crop`, `pdf.metadata`, and invisible OCR text-layer writing) in-process on every runtime, including `wasm32-unknown-unknown` in the browser and offline on macOS.
- **Out-of-process engine protocol v2 (AD-3)**: `ariad-engine/2` extends `ariad-engine/1` with `op = "run_tool"` and `capabilities.tools[]` while keeping `/1` frozen and unchanged. Each engine declares the protocol version it speaks statically in `EngineSpec.protocol` (`V1` or `V2`) with no runtime handshake. `run_tool` is strictly one-shot and never uses warm sessions (`session_key() == None`).
- **Stateless edit operations (AD-17)**: Page-mode edits (`pdf.edit_apply` / `edit.apply {ops: [...]}`) are stateless tool invocations taking document bytes plus an ordered operation list and returning new PDF bytes. Document-mode editing happens in the surface editor (Tiptap on web, TextKit on macOS) and exports a full `ariad-ir/2` document. No engine session holds edit state.

---

## 2. Tool Identification, Versioning, and Groups

### Tool ID Grammar

Every tool identifier (`ToolId`) is a validated UTF-8 string matching the regular expression:

```regex
^[a-z]+(\.[a-z0-9_]+)+$
```

- **Flat two-segment namespace**: Tool IDs use a flat `<domain>.<action>` convention (`pdf.merge`, `pdf.rotate`, `pdf.extract_pages`, `pdf.watermark_remove_raster`, `ai.summarize`). Nested three-segment styles such as `pdf.page.rotate` are **not** used.
- **Valid examples**: `pdf.info`, `pdf.merge`, `pdf.split`, `pdf.rotate`, `pdf.decrypt`, `pdf.ocr`, `ai.ask`, `ai.recipe_from_text`.
- **Invalid examples**: `PDF.rotate` (uppercase), `pdf.page.rotate` (nested namespace disallowed by convention), `pdf-rotate` (missing dot separator), `pdf.` (empty segment), `.merge` (leading dot).

### Version per Tool

Every `ToolSpec` carries a string `version` (initially `"1"`). When an engine advertises tool support in `ariad-engine/2` `capabilities`, it reports both `id` and `version`:

```json
{
  "type": "capabilities",
  "protocol": "ariad-engine/2",
  "engine": "pdfium",
  "version": "0.3.0",
  "tool": { "name": "pdfium", "version": "136.0.7069", "status": "found" },
  "license": "Apache-2.0 AND BSD-3-Clause",
  "routes": [],
  "tools": [
    { "id": "pdf.render", "version": "1" },
    { "id": "pdf.redact", "version": "1" }
  ],
  "enforces_memory_limit": false,
  "supports_session": false
}
```

### Tool Groups

Every tool belongs to exactly one group from the closed `ToolGroup` enum:

| Group | Purpose | Example Tools |
|---|---|---|
| `organize` | Page and document structure manipulation | `pdf.info`, `pdf.merge`, `pdf.split`, `pdf.extract_pages`, `pdf.delete_pages`, `pdf.reorder`, `pdf.rotate`, `pdf.rotate_content`, `pdf.crop`, `pdf.crop_whitespace`, `pdf.scale`, `pdf.nup`, `pdf.booklet`, `pdf.single_page`, `pdf.insert_blank`, `pdf.alternate_mix`, `pdf.divide_pages`, `pdf.posterize`, `pdf.overlay`, `pdf.metadata`, `pdf.outline`, `pdf.attachments`, `pdf.portfolio`, `pdf.page_labels`, `pdf.viewer_prefs`, `pdf.layers`, `pdf.remove_blank`, `pdf.detect_scanned`, `pdf.text_layout` |
| `convert` | Multi-artifact or specialized document conversions that do not fit 1-to-1 routes | `pdf.render`, `pdf.to_images`, `pdf.thumbnails`, `pdf.extract_images`, `image.to_pdf`, `pdf.to_pdfa`, `pdf.validate_pdfa`, `pdf.from_url` |
| `optimize` | File size reduction, stream recompression, decompression, linearization, repair | `pdf.decompress`, `pdf.compress`, `pdf.linearize`, `pdf.repair` |
| `security` | Encryption, decryption, permission flags, sanitization | `pdf.encrypt`, `pdf.decrypt`, `pdf.permissions`, `pdf.sanitize` |
| `overlay` | Stamps, page numbers, headers/footers, watermarks, background overlays | `pdf.page_numbers`, `pdf.stamp`, `pdf.watermark_add`, `pdf.watermark_remove`, `pdf.watermark_remove_raster` |
| `ocr` | Optical character recognition and deskewing | `pdf.ocr`, `pdf.deskew` |
| `annotate` | Highlights, notes, freehand ink, geometric markups, comment flattening | `pdf.annotate`, `pdf.annotations_flatten`, `pdf.annotations_remove` |
| `forms` | AcroForm field inspection, filling, creation, and flattening | `pdf.forms_inspect`, `pdf.forms_fill`, `pdf.forms_flatten` |
| `sign` | PAdES digital signing, timestamping, and signature verification | `pdf.sign`, `pdf.sign_verify` |
| `redact` | True object-stream and raster redaction with verification report | `pdf.redact`, `pdf.redact_verify` |
| `compare` | Visual overlay comparison and semantic content comparison | `pdf.compare_visual`, `pdf.compare_semantic` |
| `edit` | Stateless page-mode in-place edits and document-mode export helpers | `pdf.edit_apply` |
| `ai` | Offline-first summarization, Q&A with citations, translation, table extraction | `ai.summarize`, `ai.ask`, `ai.translate`, `ai.extract_tables`, `ai.recipe_from_text` |
| `automation` | Batch recipe execution and folder watch automation | `automation.recipe_validate` |

### `ToolSpec` Descriptor Example

All parameter structs live in `ariad-core::tools::params::*` (pure `serde` + `schemars` types with no engine or `lopdf` dependencies) so `ariad-core` owns the complete registry without crate cycles:

```json
{
  "id": "pdf.rotate",
  "version": "1",
  "group": "organize",
  "summary": "Rotate selected PDF pages by a multiple of 90 degrees without re-rendering.",
  "inputs": { "arity": "one" },
  "input_formats": ["pdf"],
  "output_format": "pdf",
  "tier": "free",
  "runtimes": ["wasm", "local"],
  "engine": "ariad-pdf",
  "secret_params": [],
  "params_schema": {
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "title": "RotateParams",
    "type": "object",
    "additionalProperties": false,
    "properties": {
      "pages": {
        "type": "string",
        "default": "all",
        "description": "1-based page range selector."
      },
      "angle": {
        "type": "integer",
        "enum": [-90, 90, 180, 270],
        "description": "Clockwise rotation angle in degrees."
      }
    },
    "required": ["angle"]
  }
}
```

> **Note on `runtimes`**: Tools register `["wasm", "local"]` (or `["local"]` for out-of-process engines such as `pdfium`) until the Phase 13 server tool-job infrastructure (`job_inputs`, worker queues, and plan-to-tool-access mapping) and Phase 14 browser WASM PDFium integration land.

### Multi-Format Tools (`output_format` and `output_formats`)

Every `ToolSpec` carries a primary `output_format` string (such as `"pdf"`, `"png"`, or `"json"`). Tools that can emit artifacts in more than one format additionally declare a non-empty `output_formats` allowlist (serialized in `schemas/tools.v1.json` only when non-empty so single-format descriptors remain unchanged):

- `pdf.render` and `pdf.to_images`: `output_format: "png"`, `output_formats: ["png", "jpeg", "webp", "bmp", "tiff"]`
- `pdf.thumbnails`: `output_format: "png"`, `output_formats: ["png", "json"]`
- `pdf.extract_images`: `output_format: "png"`, `output_formats: ["png", "jpeg", "zip"]`

Before opening each selected page, `pdf.extract_images` checks image dimensions and data against the job's limits. Inline images are accepted when their data is unfiltered or uses a single ASCIIHex, ASCII85, or Flate filter and their color components can be determined from their dictionary. If the engine cannot safely inspect an inline image or a referenced image's color space, it fails with `limit_exceeded` instead of opening the page. This includes inline images with other filters, filter chains, or color-space aliases. Each distinct form XObject is charged once per page, including when it is invoked repeatedly or reached through multiple resource paths. Traversal and image budgets still apply.

Both `ariad-host` and `ariad-wasm` enforce `ToolSpec::allowed_output_formats()` (`output_formats` when non-empty, otherwise `[output_format]`):
1. When the caller selects a target format via `params.format` or an explicit `-o <FILE>` extension, the requested format must be in `allowed_output_formats()`.
2. Before publishing or returning any produced artifact, every emitted artifact's format (and file extension) is checked against `allowed_output_formats()` and, when `params.format` was specified on a single-format-per-run tool (`pdf.render` / `pdf.to_images`), against that requested format.

### Mode-Dependent Engine Availability (`pdf.split` `detect = "pixel"`)

`pdf.split` supports `mode: "blank-separator"` with two detection modes (`detect`):
- `"content"` (default): Inspects PDF page content streams and resources in-process via `ariad-pdf`. Available on both `wasm` and `local` runtimes without any external pack.
- `"pixel"`: Rasterizes each page at `dpi` (default `72`, range `36..=600`) and measures the fraction of near-white pixels against `threshold` (default `0.995`, range `0.90..=1.0`) using the out-of-process `pdfium` engine (`local` runtime).

Engine availability for `pdf.split` is decided **before execution** via `ToolSpec::engine_for_params(params)`:
- When `mode = "blank-separator"` and `detect = "pixel"`, the effective engine is `"pdfium"`.
- On `ariad-wasm` (where out-of-process engines do not exist) or on `ariad-host` when the `pdfium` engine pack is not installed, the call fails upfront at validation/plan time (before staging inputs or running any structural fallback) with exit code `5` (`engine_unavailable` on WASM; `tool_missing` on the host) naming the missing `"pdfium"` engine and its install hint (`ashift engines install pdfium`). It **never** falls back silently to `"content"` detection.

---

## 3. Inputs: Arity, Formats, Ordering, and Encryption Preflight

### Input Arity (`InputArity`)

Each `ToolSpec` declares its input arity:

- `one`: Exactly 1 input file is required (for example, `pdf.info`, `pdf.rotate`, `pdf.split`, `pdf.compress`).
- `many(min, max)`: Between `min` and `max` input files (inclusive) are required (`1 <= min <= max`).
  - `pdf.merge`: `many(2, 500)`
  - `pdf.alternate_mix`: `many(2, 2)`
  - `pdf.overlay`: `many(2, 2)`
  - `pdf.compare_visual` / `pdf.compare_semantic`: `many(2, 2)`
  - `pdf.attachments`: `many(1, 100)` (`list` / `extract` / `remove` take 1 PDF; `add` takes the base PDF followed by attachment files)
  - `pdf.portfolio`: `many(1, 100)` (`create` takes 1..=100 input files; `list` / `extract` take 1 portfolio PDF)

Supplying fewer or more inputs than `inputs` allows fails before engine dispatch with exit code `10` (`wrong_input`).

### Accepted Formats

Each `ToolSpec` declares `input_formats` using canonical format identifiers (for example, `["pdf"]` or `["png", "jpeg", "tiff", "webp"]`). The host sniffs every input file's actual content (magic bytes and container inspection, identical to `ashift inspect`) rather than trusting the filename extension. If any input's detected format is not in `input_formats`, execution fails before dispatch with exit code `10` (`wrong_input`). Regular files are required; directories, FIFOs, sockets, and device nodes are rejected upfront.

### Ordering Semantics

Input order is preserved end-to-end across CLI arguments, MCP `inputs` arrays, WASM `inputs` arrays, and `ariad-engine/2` `inputs`:

- **`pdf.merge`**: Pages are concatenated in the exact order the input files are provided (`inputs[0]`, then `inputs[1]`, ..., `inputs[N - 1]`). When `outline: "per-file"` is active, top-level bookmarks are generated in the same order using each input's file stem.
- **`pdf.overlay`**: `inputs[0]` is the base document; `inputs[1]` is the overlay document applied over or under `inputs[0]`.
- **`pdf.alternate_mix`**: Pages interleave starting with page 1 of `inputs[0]`, followed by page 1 of `inputs[1]` (or the last page of `inputs[1]` when `reverse_second: true`).
- **`pdf.compare_*`**: `inputs[0]` is the baseline ("before") document and `inputs[1]` is the revised ("after") document.

### Encrypted Input Preflight

`lopdf` automatically decrypts PDFs that have an empty user password (such as owner-password-only PDFs that restrict printing or copying) and strips their `/Encrypt` dictionary on save. To prevent silent permission stripping or unintended decryption:

1. Before or during bounded PDF loading (`ariad-pdf::document::open`), the loader inspects the raw trailer for `/Encrypt`.
2. If `/Encrypt` is present, every tool except `pdf.decrypt` (or tools explicitly accepting a decryption password secret) immediately returns `PdfError::Encrypted` and exits with code `12` (`encrypted_input`), instructing the user to run `pdf.decrypt` first.
3. Structural tools never save an unencrypted copy of an encrypted input.

---

## 4. Parameters (`params`) and Large Parameter Delivery (`params_file`)

### Schema Validation and Explicit Defaults

1. **Per-tool JSON Schema**: Every tool's parameters are defined by a Rust struct in `ariad-core::tools::params::<tool>` deriving `Serialize`, `Deserialize`, and `schemars::JsonSchema`. All tool schemas are exported in `schemas/tools.v1.json` and checked for drift in CI.
2. **Unknown fields rejected**: Every parameter struct carries `#[serde(deny_unknown_fields)]` (`"additionalProperties": false` in JSON Schema). Passing an unknown key, wrong type, or out-of-range value fails prior to tool execution with exit code `9` (`invalid_params`).
3. **Explicit defaults**: Every optional field defines a deterministic default via `#[serde(default = "...")]` and documents that default in the JSON Schema `"default"` property. Calling a tool where all parameters are optional with `{}` is always valid.

#### Parameter Examples

```json
// pdf.info (takes no parameters; unknown fields are rejected)
{}
```

```json
// pdf.merge (explicit defaults shown)
{
  "outline": "per-file",
  "page_labels": "renumber"
}
```

```json
// pdf.split (split by page ranges)
{
  "mode": "ranges",
  "ranges": ["1-5", "6-10", "11-last"]
}
```

### Large Parameters (`params_file`, AD-3)

The `ariad-engine/2` protocol enforces a strict **1 MiB** (1,048,576 bytes) cap on each stdin request line.

- When serialized `params` are **<= 256 KiB** (262,144 bytes) and contain **no** secret parameters, `ariad-host` sends `params` inline in the `run_tool` request JSON and omits `params_file`:
  ```json
  {"protocol":"ariad-engine/2","job":"job_01K…","op":"run_tool","tool":"pdf.rotate",
   "inputs":[{"path":"/work/in/0.pdf","format":"pdf"}],
   "output":{"dir":"/work/out"},"params":{"pages":"1-3,5","angle":90},
   "work_dir":"/work/tmp","limits":{"max_pages":null,"timeout_s":null,"max_memory_mb":null,"max_nesting_depth":64}}
  ```
- When serialized `params` exceed **256 KiB** (for example, `pdf.outline` importing a large bookmark tree, `pdf.forms_fill` with many fields, or `pdf.edit_apply` with a batch of page-mode operations) — or when **any** secret parameter is present (AD-16) — `ariad-host`:
  1. Writes the full UTF-8 JSON parameter object to `<work_dir>/params.json` with file mode `0600`.
  2. Sets `"params": {}` and `"params_file": "/work/tmp/params.json"` in the `run_tool` request:
     ```json
     {"protocol":"ariad-engine/2","job":"job_01K…","op":"run_tool","tool":"pdf.outline",
      "inputs":[{"path":"/work/in/0.pdf","format":"pdf"}],
      "output":{"dir":"/work/out"},"params":{},"params_file":"/work/tmp/params.json",
      "work_dir":"/work/tmp","limits":{"max_pages":null,"timeout_s":null,"max_memory_mb":null,"max_nesting_depth":64}}
     ```
  3. Deletes `<work_dir>/params.json` immediately when the job finishes, fails, or is cancelled.
- Engine helpers in `ariad-engine-io` transparently load `params_file` when present and reject inline `params` larger than 256 KiB with `invalid_request`.

---

## 5. Page-Range Grammar

Every tool that selects or orders pages (`pdf.rotate`, `pdf.extract_pages`, `pdf.delete_pages`, `pdf.reorder`, `pdf.split`, `pdf.crop`, `pdf.scale`, `pdf.insert_blank`, `pdf.overlay`, `pdf.ocr`, `pdf.redact`, etc.) shares a single normative parser in `ariad-core::tools::page_range::PageRange`.

### Grammar (EBNF)

```ebnf
page_range  = token , { "," , token } ;
token       = "all"
            | "even"
            | "odd"
            | range_expr
            | page_ref ;
range_expr  = page_ref , "-" , page_ref
            | page_ref , "-"
            | "-" , page_ref ;
page_ref    = pos_int
            | "last"
            | "last-" , pos_int ;
pos_int     = nonzero_digit , { digit } ;
nonzero_digit = "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" ;
digit       = "0" | nonzero_digit ;
```

ASCII whitespace around commas and tokens is ignored (`"1 - 3"` is a syntax error; `"1-3, 5, last-1"` is valid). Empty strings (`""`) and leading, trailing, or consecutive commas (`",1"`, `"1,"`, `"1,,2"`) are rejected.

### Token Semantics (1-Based Indexing)

Let `P` (`P >= 1`) be the total page count of the target PDF document:

| Token | Meaning | Expansion on a 10-page PDF (`P = 10`) |
|---|---|---|
| `all` | Every page in ascending order `1..=P` | `[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]` |
| `even` | Even-numbered pages in ascending order (`2, 4, ...`) | `[2, 4, 6, 8, 10]` (errors if `P == 1` and selection becomes empty) |
| `odd` | Odd-numbered pages in ascending order (`1, 3, ...`) | `[1, 3, 5, 7, 9]` |
| `N` | Single 1-based page `N` (`1 <= N <= P`) | `5` → `[5]` |
| `N-M` | Inclusive range from `N` to `M`. If `N <= M`, expands ascending (`N..=M`); if `N > M`, expands descending (`N, N-1, ..., M`). | `2-5` → `[2, 3, 4, 5]`; `5-2` → `[5, 4, 3, 2]` |
| `N-` | From page `N` through `P` (`N..=P`, `1 <= N <= P`) | `8-` → `[8, 9, 10]` |
| `-M` | From page `1` through `M` (`1..=M`, `1 <= M <= P`) | `-3` → `[1, 2, 3]` |
| `last` | Final page `P` | `last` → `[10]` |
| `last-K` | Page `P - K` (`1 <= K < P`) | `last-1` → `[9]`; `last-2-last` → `[8, 9, 10]` |

### Duplicate Page Handling

Whether duplicate page indices are permitted depends on the tool's operation mode:

1. **Reorder and extraction tools (`DuplicatePolicy::KeepInOrder`)** — `pdf.extract_pages`, `pdf.reorder`:
   - Duplicates are preserved in the exact order specified by the user.
   - Example: `"1,1,5-3,last"` on a 10-page document expands to `[1, 1, 5, 4, 3, 10]`.
2. **In-place modification and deletion tools (`DuplicatePolicy::Reject`)** — `pdf.rotate`, `pdf.delete_pages`, `pdf.crop`, `pdf.scale`, `pdf.overlay`, `pdf.ocr`, `pdf.redact`, `pdf.watermark_add`, etc.:
   - Any repeated page index (whether explicit like `"2,2"` or overlapping like `"1-4,3-5"` or `"all,1"`) is **rejected** with exit code `9` (`invalid_params`).

### Normative Error Reporting

Every parse or bounds error **must** name the exact offending comma-separated token in its message and exit with code `9` (`invalid_params`):

- Token syntax error: `invalid page range token "1..3": expected page number, range, "all", "even", "odd", or "last"`
- Zero index: `invalid page range token "0": pages are 1-based`
- Out-of-bounds page: `page range token "12-15" exceeds document page count (10)`
- Out-of-bounds `last-K`: `page range token "last-10" underflows document page count (10)`
- Disallowed duplicate: `duplicate page 3 in page range token "3-5" (already selected)`

---

## 6. Outputs, Naming, Confinement, and Atomic Multi-Artifact Publishing

### Default Artifact Naming (`<stem>.<tool-suffix>.<ext>`)

Let `<stem>` be the filename stem of `inputs[0]` (with its format extension stripped):

- **Single-artifact tools**: Default filename is `<stem>.<tool-suffix>.<ext>`:
  - `pdf.rotate` → `report.rotated.pdf`
  - `pdf.rotate_content` → `report.rotated-content.pdf`
  - `pdf.merge` → `first.merged.pdf`
  - `pdf.extract_pages` → `report.extracted.pdf`
  - `pdf.delete_pages` → `report.trimmed.pdf`
  - `pdf.reorder` → `report.reordered.pdf`
  - `pdf.crop` → `report.cropped.pdf`
  - `pdf.scale` → `report.scaled.pdf`
  - `pdf.nup` → `report.nup.pdf`
  - `pdf.booklet` → `report.booklet.pdf`
  - `pdf.single_page` → `report.single-page.pdf`
  - `pdf.insert_blank` → `report.inserted.pdf`
  - `pdf.alternate_mix` → `first.mixed.pdf`
  - `pdf.divide_pages` → `report.divided.pdf`
  - `pdf.posterize` → `report.posterized.pdf`
  - `pdf.overlay` → `base.overlay.pdf`
  - `pdf.metadata` → `report.metadata.pdf`
  - `pdf.outline` (`import` / `clear` / `from_headings`) → `report.outline.pdf`; (`export`) → `report.outline.json`
  - `pdf.attachments` (`add` / `remove`) → `report.attachments.pdf`; (`list`) → `report.attachments.json`
  - `pdf.portfolio` (`create`) → `first.portfolio.pdf`; (`list`) → `report.portfolio.json`
  - `pdf.page_labels` → `report.labeled.pdf`
  - `pdf.viewer_prefs` → `report.prefs.pdf`
  - `pdf.layers` (`set_default_visibility` / `remove`) → `report.layers.pdf`; (`list`) → `report.layers.json`
  - `pdf.remove_blank` → `report.remove-blank.pdf`
  - `pdf.crop_whitespace` → `report.cropped.pdf`
  - `pdf.detect_scanned` → `report.detect-scanned.json`
  - `pdf.text_layout` → `report.text-layout.json`
  - `image.to_pdf` → `first.pdf`
  - `pdf.decompress` → `report.decompressed.pdf`
  - `pdf.compress` → `report.compressed.pdf`
  - `pdf.linearize` → `report.linearized.pdf`
  - `pdf.repair` → `report.repaired.pdf`
  - `pdf.encrypt` → `report.encrypted.pdf`
  - `pdf.decrypt` → `report.decrypted.pdf`
  - `pdf.sanitize` → `report.sanitized.pdf`
  - `pdf.ocr` → `scan.ocr.pdf`
  - `pdf.sign` → `contract.signed.pdf`
  - `pdf.redact` → `contract.redacted.pdf` (plus `contract.redaction-report.json` when verification report is requested)
  - `pdf.info` → prints structured JSON to stdout on CLI when `--json` is passed (or human summary without `-o`), and writes `report.info.json` when `-o` specifies a directory or file.
  - JSON-emitting actions (`pdf.outline` `export`, and `list` on `pdf.attachments`, `pdf.portfolio` and `pdf.layers`) never write a PDF. Without `-o` the CLI prints the JSON to stdout and creates no file beside the input; `-o <dir>` writes the `.json` name listed above and `-o <FILE>.json` writes that exact file.
- **Multi-artifact tools**:
  - `pdf.split` → `<stem>.part-001.pdf`, `<stem>.part-002.pdf`, ... (1-based index, zero-padded to at least 3 digits: `001`..`999`, widening to 4+ digits when parts >= 1,000).
  - `pdf.to_images` / `pdf.render` → `<stem>.page-001.<ext>`, `<stem>.page-002.<ext>`, ... (`combine = "none"`), or `<stem>.pages.<ext>` (`combine = "vertical" | "horizontal"`).
  - `pdf.thumbnails` → `<stem>.page-001.png`, `<stem>.page-002.png`, ... plus `<stem>.thumbnails.json`.
  - `pdf.extract_images` → `<stem>.page-001.image-001.<ext>`, ... (`archive = false`), or `<stem>.images.zip` (`archive = true`).
  - `pdf.attachments` and `pdf.portfolio` (`action = "extract"`) → `<stem>.attachment-001.<sanitized-name>` confined inside `<output_dir>`. The attachment's own name and extension are kept and its bytes are written unchanged, except that executable, script, shortcut and installer types (for example `.exe`, `.py`, `.lnk`, `.sh`, `.pkg`) get `.bin` appended to the file name, with one warning per renamed file. Files never get an executable bit. Entries are numbered in name order.

When `-o <FILE>` is passed to a single-artifact tool, that exact file path is used. Passing a non-directory `-o <FILE>` to a multi-artifact tool (such as `pdf.split`) is rejected upfront with exit code `2` (`usage`).

### Input-Overwrite Prevention and Confinement

1. **Never overwrite inputs**: Every output path is canonicalized and checked against the canonical paths of all job inputs (`confine_input` / `confine_output`). If any destination resolves to the same physical file as any input, the call is unconditionally rejected (exit code `2`), even when `--overwrite` or `--allow-overwrite` is passed.
2. **Existing destination files**: If any target output file already exists (or is a dangling symlink) and overwrite was not explicitly enabled (`--overwrite` on CLI, or `--allow-overwrite` + `overwrite: true` on MCP), execution fails with exit code `6` (`destination_exists`).
3. **Directory confinement**: On MCP and confined host entrypoints, all inputs and outputs must reside within allowed roots (`--allow-dir` or client `roots/list`), with no symlink escapes, `..` traversal, or hidden (`.*`) path components.

### Staging and All-or-Nothing Atomic Multi-Artifact Publication

Tools never write directly to user destination paths:

1. **Workspace staging**: Both in-process tools (`ariad-pdf`) and out-of-process engines (`ariad-engine/2`) write all produced artifacts into the isolated job workspace directory `<workspace>/out/`.
2. **Validation**: Before publishing, `ariad-host` verifies that every reported `artifact` path stays strictly inside `<workspace>/out/`, is a regular file (no symlinks), and has a valid filename and extension.
3. **Atomic promotion (`Workspace::promote_into`)**: Artifacts are published into the confined destination directory through capability handles (`cap_std::fs::Dir`). Each file is written/staged to a sibling temporary file in the destination directory, flushed with `sync_all()`, and atomically linked/renamed into place (using hard-link noclobber when `overwrite == false`).
4. **All-or-nothing set publication**: When a tool produces multiple artifacts (`N >= 1`), they are published as an indivisible set. If promoting artifact `k` of `N` fails for any reason (for example, `EEXIST` on artifact 3 of 5 or `ENOSPC`), `ariad-host` immediately unlinks artifacts `1..k-1` that were just created during that batch and returns the error. On any tool failure, limit violation, or signal cancellation (exit `130`), **nothing is published** to the output directory.

---

## 7. Resource Limits, Sandbox, Network Isolation, and Progress

### Limits Honoured per Tool

Every tool invocation receives an `ariad_core::limits::Limits` struct (`Limits::local()` on CLI/macOS, `Limits::cloud()` on cloud workers, plus browser `budgetBytes` in `ariad-wasm`). The runner generalises over `JobRequest::{V1, V2}` (`output_dirs()`, `limits()`, `work_dir()`, `session_key()`) so that every safeguard enforced for `convert` applies identically to `run_tool`:

| Limit | Enforcement |
|---|---|
| `max_input_bytes` | Checked per input file before staging; in `ariad-wasm`, the sum of all input `Uint8Array` byte lengths is checked against `budgetBytes` before opening any document. |
| `max_pages` | Checked against input page counts at load (when set in `Limits`), and combined with the fixed 10,000-page structural output ceiling as `min(10_000, max_pages)` for output-producing tools (`pdf.merge`, `pdf.insert_blank`, `pdf.extract_pages`, `pdf.reorder`, `pdf.nup`, `pdf.booklet`, `pdf.divide_pages`, `pdf.posterize`, and `pdf.split`). In `pdf.split`, both the output pages of each part and the cumulative output pages across all parts are capped at `min(10_000, max_pages)` (never the input page count), and serialized part bytes are charged against the per-document decode budget (`max_decompressed_bytes`). |
| `timeout_s` | Wall-clock timer enforced by the host runner (`JobSpec::for_request`) and cancellation token; kills the engine process tree on expiry. |
| `max_memory_mb` | Host watchdog samples aggregate resident set size (RSS) of the engine process tree before dispatch and every 100 ms on macOS or 250 ms elsewhere during execution, killing the tree on violation. Linux PDFium tools additionally receive a hard `RLIMIT_AS` via the launcher before execution, including with sandboxing off. |
| Decoded stream & object caps | `ariad-pdf` opens every PDF exclusively through `ariad_core::reader::pdf::load_bounded(bytes, &limits)`, enforcing `max_decompressed_bytes` across Flate/LZW/filter streams, `max_nesting_depth` (< 100), and `MAX_OBJECTS` (object count ceiling). |
| `max_asset_bytes` | Bounds individual embedded images, fonts, or file attachments added or extracted by a tool. |

PDFium image extraction bounds the combined declared inline-image pixels, decoded bytes, and scanned content for each page, including forms; image extraction and scan detection also bound object traversal per page and document. Extraction rejects a form whose estimated nested object expansion exceeds 100,000 visits. Scan detection accepts at most 2,000 image rectangles per page and bounds the work used to calculate their exact union. Complex inputs may therefore reach a limit even when their file size is small.

For PDFium tools, an unspecified `timeout_s` or `max_memory_mb` uses the cloud defaults (600 seconds and 4096 MiB respectively), including local runs. Explicit limits remain unchanged. `pdf.render` and `pdf.to_images` also cap the cumulative encoded page bytes and the final ZIP size at `max_decompressed_bytes`; a limit failure publishes no artifacts.

Exceeding any limit terminates the job immediately and returns `limit_exceeded` (exit code `4`).

### Sandbox and Network Isolation (AD-15)

- **Mandatory confinement**: Out-of-process engines (`pdfium`, `qpdf`, `tesseract`, `sign`, `verapdf`, `ai`, `inpaint`) declare `sandbox_required: true` in their `EngineSpec` and refuse to start if OS sandbox isolation (Linux Landlock + seccomp, macOS Seatbelt) is unavailable unless the user explicitly sets `ASHIFT_SANDBOX=off`. Writable filesystem roots are restricted to `<workspace>/out` and the job's private `work_dir` (`TMPDIR`).
- **Zero engine network access (AD-15)**: No engine ever receives network access. Whenever a tool requires remote data — RFC 3161 timestamp authority (TSA) responses, OCSP responses, CRLs for `pdf.sign`, or web page HTML/assets for `pdf.from_url` — the trusted host (`ariad-host`) fetches the resource beforehand using its SSRF-safe HTTP client:
  - HTTPS only (plain HTTP is allowed only for explicit loopback development).
  - DNS is resolved once per hop and rejected if **any** resolved IP address is loopback, private (RFC 1918 / RFC 4193), link-local, CGNAT, multicast, broadcast, or cloud metadata (`169.254.169.254`, `fd00:ec2::254`).
  - Every redirect target is re-validated against the same URL and DNS rules (bounded redirect count).
  - Strict response byte size and wall-clock time caps are enforced.
  - The host writes the verified bytes into `<work_dir>` and passes the local file path to the offline engine. For `pdf.from_url`, the host renders the fetched static HTML offline (`HTML -> IR -> Typst`); JavaScript-driven pages are never executed and emit a `js_not_executed` warning.

### Progress Unit

Progress events use **pages** as their canonical unit (`done: u64`, `total: u64`), with `stage` set to the `tool_id` (or `"run_tool"`):

```json
{"type":"progress","stage":"pdf.rotate","done":12,"total":20}
```

---

## 8. Determinism

For every in-process tool in `ariad-pdf` (and deterministic native writers), running the same tool with **the same input bytes, the same `params`, and the same `SOURCE_DATE_EPOCH`** produces **byte-identical output**:

1. **Timestamps**: When `SOURCE_DATE_EPOCH` (Unix seconds in UTC) is set in the environment (or supplied via `ToolCtx`), PDF `/CreationDate` and `/ModDate` in the document `Info` dictionary and `xmp:ModifyDate` / `xmp:CreateDate` / `xmp:MetadataDate` in the XMP metadata stream are formatted deterministically from that timestamp (`D:YYYYMMDDHHmmSSZ`).
2. **Document IDs (`/ID`)**: Trailer `/ID` arrays (`[<id0> <id1>]`) are computed as a deterministic cryptographic digest (first 16 bytes of SHA-256 over the input hashes, canonical tool ID, canonical `params` JSON, and timestamp) rather than from OS random number generators.
3. **Object numbering and serialization**: Orphan objects are garbage-collected before save, surviving indirect objects are renumbered in deterministic breadth-first traversal order from the `Catalog` root, and dictionary entries are written in lexicographical key order.

Golden tests and CI determinism checks run every in-process tool twice under a fixed `SOURCE_DATE_EPOCH` and assert `sha256(run_1) == sha256(run_2)`.

---

## 9. Recipes (`ariad-recipe/1`) and Untrusted Recipe Rules

A **recipe** chains multiple tool invocations and format conversions into a reproducible pipeline validated against `schemas/recipe.v1.json`.

### Canonical Format

Minimal canonical recipe:

```json
{"version":"ariad-recipe/1","steps":[{"tool":"pdf.merge","params":{}},{"convert":{"to":"docx"}}]}
```

Expanded multi-step example:

```json
{
  "version": "ariad-recipe/1",
  "steps": [
    {
      "tool": "pdf.merge",
      "params": {
        "outline": "per-file"
      }
    },
    {
      "tool": "pdf.rotate",
      "params": {
        "pages": "all",
        "angle": 90
      }
    },
    {
      "convert": {
        "to": "docx",
        "profile": "editable"
      }
    }
  ]
}
```

### Sequential Execution Semantics

1. **Step chaining**: Steps execute sequentially (`steps[0]`, `steps[1]`, ..., `steps[M - 1]`) inside a shared isolated job workspace.
   - `steps[0]` receives the initial input file(s) passed to `ashift recipe run`.
   - The output artifact(s) produced by step `N` become the input file(s) of step `N + 1` in artifact emission order.
2. **Step kinds**:
   - **Tool step**: `{"tool": "<tool_id>", "params": { ... }, "if"?: "<condition>"}` invokes a registered tool from `ariad-core::tools`.
   - **Convert step**: `{"convert": {"to": "<format>", "profile"?: "editable" | "faithful" | "fast" | "private"}}` runs the standard conversion pipeline (`ariad-host::convert`) on each current artifact.
3. **Fan-out and conditionals (Phase 13)**:
   - When a step produces multiple artifacts (for example, `pdf.split` emitting 5 parts) and the next step has `InputArity::One` (for example, `pdf.rotate` or `convert`), the runner fans out and applies step `N + 1` to each artifact in order; if step `N + 1` has `InputArity::Many`, it receives the full artifact list as a single batch.
   - Optional step predicate `"if"` (such as `"page_count > 10"` or `"needs_ocr == true"`) skips the step and passes its inputs through unchanged when false.
   - Optional error policy `"on_error": "stop" | "skip" | "continue"` defaults to `"stop"` (aborting the entire recipe and publishing nothing).
4. **Atomic publication**: Intermediate step outputs remain inside the temporary workspace and are cleaned up when the recipe finishes. Only the final step's artifacts are atomically published to the output directory.

### Output Naming Templates

A recipe may specify an optional `"naming"` template for final published artifacts (default: `"{stem}"` plus the final step's suffix and extension):

- **Allowed character set**: Only ASCII characters matching `^[A-Za-z0-9._ -]+$` outside placeholder tokens, plus the closed token set:
  - `{stem}`: Sanitized stem of the originating input file.
  - `{n}`: 1-based zero-padded output index (`001`, `002`, ...).
  - `{date}`: UTC date in `YYYY-MM-DD` (honouring `SOURCE_DATE_EPOCH`).
- **Disallowed patterns**: Any path separator (`/`, `\`), `..` segment, leading dot (`.`), control character, shell metacharacter, or unknown `{...}` token is rejected during recipe validation with exit code `9` (`invalid_params`).
- **Confinement**: Rendered output filenames are always joined as a single normal path component beneath the confined output directory capability (`cap_std::fs::Dir`).

### Untrusted Recipes and Deep Links (AD-18)

Any recipe loaded from an external `.json` file, shared URL, clipboard import, macOS Quick Look preview, or macOS Shortcut is classified as `origin: Imported` (untrusted):

1. **Explicit confirmation required**: Surfaces must display every step (`tool` / `convert`, parameters, and target output directory) and require explicit user confirmation before executing an imported recipe:
   - CLI (`ashift recipe run`): Prints the step summary and prompts interactively on `/dev/tty`, or requires the explicit `--yes` flag in non-interactive scripts.
   - Web and macOS: Shows a confirmation sheet listing all steps and target folder before any step runs.
   - Natural-language recipes generated by `ai.recipe_from_text` are also treated as untrusted and never auto-execute without user confirmation.
2. **Forbidden steps in imported recipes**: Imported recipes are strictly forbidden from configuring watch-folder automation (`watch`) or network-fetch steps (such as `pdf.from_url`). Presence of either in an imported recipe fails validation with exit code `9`.
3. **No literal secrets**: Imported and trusted recipes alike reject any literal value on an `x-ariad-secret` parameter; a recipe step may only contain `"password": {"$secret": "prompt"}` (see [§11](#11-secrets-contract-x-ariad-secret)).
4. **Deep links are route-only**: `ariadshift://` deep links may only navigate to a named view, conversion pair, or tool ID (for example, `ariadshift://tool/pdf.merge`). Deep links **never** accept filesystem paths, parameter blobs, or inline recipes.

---

## 10. Tool Tiers and Access Control (`ToolAccess`)

Every `ToolSpec` carries a `tier` field (AD-9):

```rust
pub enum Tier {
    Free,
    Pro,
}
```

### Pure Access Decision (`ariad-core::tools::access`)

Access control is computed by a single pure function in `ariad-core::tools::access`:

```rust
pub fn allowed(tier: Tier, plan: Plan) -> bool
```

backed by the `ToolAccess` type in `ariad-core`:

- Named **`ToolAccess`** so it does not collide with `ariad-server`'s cloud quota/rate-limit `Entitlement` struct.
- Called identically by every surface and runtime: `ashift` CLI, local MCP, `ariad-server` (REST `/v1` and remote `/mcp`), `ariad-ffi` (macOS), and `ariad-wasm` (browser).
- `Tier::Free` tools return `true` for every `Plan` (`Plan::Free`, `Plan::Pro`, `Plan::Team`, `Plan::Enterprise`).
- `Tier::Pro` tools return `true` only when `plan` grants Pro tool access.
- When `allowed(spec.tier, plan)` returns `false`, execution stops before opening inputs or launching engines and returns exit code `11` (`tier_not_allowed`).

### Enforcement Authority per Surface

| Surface | Enforcement Level | Notes |
|---|---|---|
| `ariad-host` (CLI, local MCP) | **Authoritative** | Checked in `ariad_host::tools::run` before input staging or engine launch; returns exit code `11`. |
| `ariad-server` (Cloud REST & remote MCP) | **Authoritative** | Server maps workspace plan entitlements to `ToolAccess` in Phase 13; returns HTTP `403 Forbidden` / MCP error with `exit_code: 11`. |
| `apps/macos` (`ariad-ffi`) | **Authoritative** | Checked in Rust via `ariad-ffi` before execution. |
| `apps/web` (`ariad-wasm`) | **Advisory** | `ariad-wasm` calls the same `access::allowed` check for UI/UX consistency, but because WebAssembly running in the user's browser executes on user-controlled hardware and can be modified in DevTools, browser tier gating is advisory rather than a security boundary. |

### Batch cap (`within_run_quota`)

`allowed(tier, plan)` stays pure and tier-only. A second pure function in the same module, `ariad-core::tools::access`, decides the batch cap: `within_run_quota(plan, RunSize { files, bytes }) -> bool`. The recipe runner calls it before a run starts and before each watch trigger (each trigger counts as one run). The cap is 25 files or 250 MB combined input per run; a run above either limit needs a plan that grants Pro. Host and server enforcement is authoritative and browser enforcement is advisory, the same split as AD-9 in the table above.

Billing, payment providers, and subscription management are out of scope of the tool contract; there are no credit meters or per-page token deductions.

---

## 11. Secrets Contract (`x-ariad-secret`)

Tools such as `pdf.encrypt`, `pdf.decrypt`, `pdf.permissions`, and `pdf.sign` require sensitive credentials (user/owner passwords, PKCS#12 / `.p12` / `.pfx` passphrases, private key passphrases). Passing secrets inline leaks them through process listings (`ps`), shell history, shared recipe files, URLs, crash reports, and database rows.

### Schema Marker

Every secret field on a tool parameter struct is listed in `ToolSpec.secret_params` and annotated in the generated JSON Schema (`schemas/tools.v1.json`) with `"x-ariad-secret": true`:

```json
{
  "title": "DecryptParams",
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "password": {
      "type": "string",
      "x-ariad-secret": true,
      "description": "User or owner password required to decrypt the PDF."
    }
  },
  "required": ["password"]
}
```

### Accepted Secret Channels per Surface (Normative)

Literal values for any parameter marked `x-ariad-secret: true` are **rejected** in inline `--param`, `--params-json`, recipe files, URLs, MCP `params`, and database `jobs` rows:

| Surface | Inline in `params` / argv / URL | Allowed Secret Channel |
|---|---|---|
| **CLI (`ashift tool`)** | **Rejected** (exit code `9` with hint to use `--secret`) | `--secret <name>=@<path>` (reads secret bytes from file), `--secret <name>=-` (reads secret from stdin), or interactive terminal prompt when `{"$secret": "prompt"}` is used. |
| **Local MCP (`ashift mcp`)** | **Rejected** (MCP error, `exit_code: 9`) | Short-lived in-memory secret handle `{"$secret": "sec_<id>"}` created via `secret_put` (never logged; rejected if handles are not enabled). |
| **Remote MCP / Cloud HTTP (`ariad-server`)** | **Rejected** (HTTP 400 / MCP error, `exit_code: 9`; never stored in `jobs` DB rows) | Short-lived ephemeral secret handle `{"$secret": "sec_<id>"}` held in memory/ephemeral store for a single job dispatch and wiped upon job completion. |
| **Web (`apps/web` / `ariad-wasm`)** | **Rejected** in URLs, `localStorage`, `sessionStorage`, IndexedDB, and OPFS history | Ephemeral in-memory `SecretBag` passed directly from the password/file dialog to the Web Worker for a single `runTool` call and zeroed/dropped after completion. |
| **macOS (`apps/macos` / `ariad-ffi`)** | **Rejected** in `UserDefaults`, recent files, and Spotlight/Quick Look metadata | macOS Keychain (`SecItem` / `SecKey` callback for signing identities) or ephemeral secure prompt (`NSSecureTextField`) passed in memory over `ariad-ffi`. |
| **Recipe (`ariad-recipe/1`)** | **Rejected** (both literal strings and `@file` paths fail validation with exit code `9`) | Only the prompt reference object `{"$secret": "prompt"}` is allowed; the executing surface prompts the user through its allowed channel at runtime. |

#### CLI Secret Examples

```bash
# Allowed: read password from a protected file or stdin
ashift tool pdf.decrypt locked.pdf --secret password=@/run/secrets/pdf_pass -o unlocked.pdf
printf '%s' "$PDF_PASS" | ashift tool pdf.decrypt locked.pdf --secret password=- -o unlocked.pdf

# Rejected (exit code 9): secret passed inline via --param
ashift tool pdf.decrypt locked.pdf --param password=hunter2
# -> error[invalid_params]: parameter 'password' is marked x-ariad-secret and cannot be passed via --param; use --secret password=@<file> or --secret password=-
```

#### Recipe Secret Reference Example

```json
{
  "version": "ariad-recipe/1",
  "steps": [
    {
      "tool": "pdf.decrypt",
      "params": {
        "password": { "$secret": "prompt" }
      }
    },
    {
      "convert": {
        "to": "markdown"
      }
    }
  ]
}
```

### `params_file` Delivery to Engines

When `ariad-host` dispatches a tool to an out-of-process `ariad-engine/2` engine (such as `qpdf` for `pdf.encrypt`/`pdf.decrypt` or `sign` for `pdf.sign`) with resolved secrets in `SecretBag`:

1. The host merges the resolved secret values into `<work_dir>/params.json`, created atomically with permissions `0600` inside the job's isolated workspace.
2. The `run_tool` request line on stdin carries `"params": {}` and `"params_file": "<work_dir>/params.json"`. Secrets never travel on the engine's command-line `argv` or environment variables.
3. Immediately after the engine process exits (or is killed on timeout/cancellation), `ariad-host` deletes `<work_dir>/params.json` before tearing down the workspace.

### Secret Scrubbing Rules

Before running any tool that has one or more resolved secrets in `SecretBag`:

1. `ariad-host` registers every non-empty secret string (along with its JSON-escaped and URL-encoded forms) in a per-job output scrubber.
2. Every byte of captured engine `stderr`, every protocol `warning.message`, every `error.message`, every tracing/diagnostic log line, and the final CLI/MCP JSON report is passed through the scrubber.
3. Any occurrence of a secret value is replaced with `[REDACTED]` before the message is written to stderr, stdout, MCP responses, or error types. Automated tests assert that forced engine failures with secret inputs never leak the secret string in stdout, stderr, or returned errors.

---

## 12. CLI Commands and Tool Exit Codes

### CLI Commands

```bash
# List all registered tools (optionally filtered by group)
ashift tools [--group <GROUP>] [--json]

# Run a single tool on one or more inputs
ashift tool <TOOL_ID> <INPUT>... [-o <FILE_OR_DIR>] \
  [--param <KEY=VALUE>]... \
  [--params-json <FILE|->] \
  [--secret <NAME=@FILE|->]... \
  [--overwrite] [--json]

# Run a pipeline recipe
ashift recipe run <RECIPE.JSON> <INPUT>... -o <DIR> [--yes] [--watch <DIR>] [--overwrite] [--json]
```

- `--param <KEY=VALUE>` parses `<VALUE>` as JSON when valid (`90`, `true`, `["1-3"]`) or falls back to a UTF-8 string (`all`, `per-file`), merging keys into the `params` object.
- `--params-json <FILE|->` reads a JSON object from `<FILE>` or stdin (`-`); `--param` flags override top-level keys from `--params-json`.

#### CLI JSON Output Shape (`ashift tool ... --json`)

```json
{
  "tool": "pdf.rotate",
  "version": "1",
  "engine": "ariad-pdf",
  "artifacts": [
    {
      "path": "output/report.rotated.pdf",
      "format": "pdf",
      "bytes": 48210
    }
  ],
  "warnings": [],
  "metrics": {
    "pages": 20,
    "elapsed_ms": 14
  }
}
```

### Tool Error Exit Codes

Tool commands preserve the existing `ashift` exit codes (`0`–`6` and `130` in [`docs/cli.md`](cli.md#exit-codes)) and allocate codes **`8`–`12`** for tool-specific failures without reusing `0`–`6`:

| Exit Code | Error Code (`error.code`) | Name | Meaning |
|---|---|---|---|
| `0` | — | Success | Tool or recipe completed and all artifacts were atomically published. |
| `1` | `engine_failure` / `io` | Failure | Corrupt PDF/input file, I/O failure, or general runtime tool failure. |
| `2` | `usage` | Usage | Invalid CLI flags, destination resolving to an input path, or passing a file `-o` to a multi-artifact tool. |
| `3` | `unsupported_route` | Unsupported Route | Used by `convert` steps inside a recipe when a conversion route is unreachable. |
| `4` | `limit_exceeded` | Limit Exceeded | Input size, page count (`max_pages`), decompressed stream bytes (`max_decompressed_bytes`), nesting depth, timeout (`timeout_s`), or process memory (`max_memory_mb`) exceeded `Limits`. |
| `5` | `tool_missing` / `sandbox_unavailable` | Tool Missing | Required engine pack (`pdfium`, `qpdf`, `tesseract`, `sign`, `verapdf`, `ai`, `inpaint`) is not installed, failed signature/hash verification at resolve, or required sandbox isolation is unavailable. |
| `6` | `destination_exists` | Destination Exists | One or more target artifact paths already exist and `--overwrite` was not specified. Nothing is published. |
| `8` | `unknown_tool` | Unknown Tool | The requested `<TOOL_ID>` does not match any tool in the `ariad-core::tools` registry (or violates the tool ID grammar). |
| `9` | `invalid_params` | Invalid Params | `params` failed JSON Schema validation, contained an unknown field, had an invalid or out-of-bounds `PageRange` token, violated untrusted-recipe rules, or passed an `x-ariad-secret` field inline. |
| `10` | `wrong_input` | Wrong Input | Input file count violates the tool's `InputArity` (`one` or `many(min, max)`), or a sniffed input file format is not in the tool's `input_formats`. |
| `11` | `tier_not_allowed` | Not Allowed by Tier | The tool has `tier: "pro"` and `ariad_core::tools::access::allowed(tier, plan)` returned `false`. |
| `12` | `encrypted_input` | Encrypted Input | Input PDF contains `/Encrypt` and requires decryption via `pdf.decrypt` (or a valid password secret was not supplied / failed verification). |
| `130` | `interrupted` | Interrupted | Interrupted by signal (`SIGINT` / `SIGTERM` / `Ctrl-C`). Engine subprocess trees are killed and staged workspace files are removed. |

> **Engine protocol (`ariad-engine/2`) `ErrorCode` mapping**: On the engine wire protocol (`crates/ariad-core/src/protocol`), `ErrorCode` remains closed to the `/1` variants (`invalid_request`, `unsupported_route`, `limit_exceeded`, `engine_failure`, `tool_missing`, `tool_version`, `io`). If an out-of-process engine receives a `run_tool` request for a tool ID it does not implement, it returns `unsupported_route`; host-level registry lookup before engine dispatch produces `unknown_tool` (exit code `8`).

---

## 13. MCP Surface (`list_doc_tools` and `run_doc_tool`)

To avoid any naming collision or confusion with the MCP protocol's built-in JSON-RPC method `tools/list`, AriadShift exposes document tools on local (`ashift mcp`) and remote (`ariad-server` `/mcp`) MCP servers under the tool names **`list_doc_tools`** and **`run_doc_tool`** (plus `run_recipe` on remote MCP in Phase 13).

### 1. `list_doc_tools`

Lists registered document tools from `ariad-core::tools`, their availability, tier, accepted input formats, arity, and JSON Schema for `params`.

- **Parameters**:
  - `group` (string, optional): Filter by `ToolGroup` (`organize`, `convert`, `optimize`, `security`, `overlay`, `ocr`, `annotate`, `forms`, `sign`, `redact`, `compare`, `edit`, `ai`, `automation`).
- **Returns**: Array of tool descriptors matching `ashift tools --json` (`id`, `version`, `group`, `summary`, `inputs`, `input_formats`, `output_format`, `tier`, `runtimes`, `engine`, `status`, `secret_params`, `params_schema`).

### 2. `run_doc_tool`

Executes a document tool inside an isolated workspace, atomically publishes all resulting artifacts into the confined output directory, registers `file://` resources (on local stdio MCP), and returns structured metrics.

- **Parameters**:
  - `tool` (string, required): Tool identifier matching `^[a-z]+(\.[a-z0-9_]+)+$` (for example, `"pdf.rotate"` or `"pdf.merge"`).
  - `inputs` (array of strings, required): Ordered list of input file paths (within allowed directories on local MCP) or `upload_id` strings (on remote MCP).
  - `output_dir` (string, optional): Confined output directory for produced artifacts (or `output` file path for single-artifact tools on local MCP). Defaults to the directory of `inputs[0]`.
  - `params` (object, optional): Tool parameters validated against the tool's `params_schema`. Defaults to `{}`. Secret fields (`x-ariad-secret`) may only reference a short-lived secret handle (`{"$secret": "sec_..."}`); literal secret strings are rejected with `invalid_params` (`exit_code: 9`).
  - `overwrite` (boolean, optional): Overwrite existing destination files when the server was launched with `--allow-overwrite`. Defaults to `false`.
- **Returns (on success)**:
  - `structuredContent` matching the CLI `--json` tool outcome (`tool`, `version`, `engine`, `artifacts`, `warnings`, `metrics`).
  - For `pdf.info` or JSON-emitting tools (`<= 256 KiB`), the structured JSON payload is also included inline.
  - On local MCP, each published artifact is registered as a `file://` resource for `resources/read`.

### MCP Error Mapping

When `run_doc_tool` or `list_doc_tools` fails, the MCP server returns a tool result with `isError: true` and `structuredContent` carrying the exact `code` and `exit_code` from the [Tool Error Exit Codes](#tool-error-exit-codes) table:

```json
{
  "isError": true,
  "structuredContent": {
    "error": {
      "code": "invalid_params",
      "exit_code": 9,
      "message": "page range token \"25\" exceeds document page count (20)"
    }
  },
  "content": [
    {
      "type": "text",
      "text": "error[invalid_params] (exit 9): page range token \"25\" exceeds document page count (20)"
    }
  ]
}
```

| Condition | MCP `error.code` | MCP `error.exit_code` | Remote MCP HTTP-equivalent `status` |
|---|---|---|---|
| Unknown `tool` ID | `unknown_tool` | `8` | `404` |
| Schema validation error, bad `PageRange` token, or inline secret | `invalid_params` | `9` | `400` |
| Input count outside `InputArity` or unsupported sniffed format | `wrong_input` | `10` | `422` |
| Tool requires `tier: "pro"` and `ToolAccess` is not allowed | `tier_not_allowed` | `11` | `403` |
| Encrypted input PDF requires `pdf.decrypt` or password secret | `encrypted_input` | `12` | `422` |
| Limit exceeded (`max_pages`, `max_memory_mb`, `timeout_s`, stream cap) | `limit_exceeded` | `4` | `413` |
| Required engine pack missing, tampered, or sandbox unavailable | `tool_missing` | `5` | `503` |
| Destination exists and overwrite is not active | `destination_exists` | `6` | `409` |
| Path outside allowed roots or self-overwrite | `usage` | `2` | `400` |
| Corrupt input or runtime engine failure | `engine_failure` | `1` | `500` |
