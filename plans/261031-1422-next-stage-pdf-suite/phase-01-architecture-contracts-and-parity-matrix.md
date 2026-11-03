---
phase: 1
title: "Architecture contracts, parity matrix and test corpus"
status: completed
priority: P1
effort: "5 tasks, ~3-4 worker-days"
dependencies: []
branch: docs/next-stage-contracts
milestone: A (foundations)
---

# Phase 1: Architecture contracts, parity matrix and test corpus

## Goal

Lock every contract the next 19 phases build on, before any product code is written: the architecture decisions below land in `ARCHITECTURE.md`, the tool contract is specified, the feature-parity matrix is approved, the i18n policy is fixed, and a permissive PDF test corpus exists. No Rust/TS product code changes in this phase.

## Why this phase exists (evidence)

- PDF tools do not fit the current engine contract: `Request::Convert` takes one input and one output format (`crates/ariad-core/src/protocol.rs:17-38`); the planner rejects same-format routes (`crates/ariad-core/src/planner/graph.rs:222`) and self-loops (`planner/mod.rs` `validate`, ~line 348). `ariad-engine/1` is final and breaking changes need a new major version (`ARCHITECTURE.md:465`).
- `ARCHITECTURE.md:745` and `:869` restrict PDFium to the Docling pack and bench. `pdfium-render = "0.9"` sits unused in root `[workspace.dependencies]`.
- AGENTS.md: a change that contradicts ARCHITECTURE.md must update it, including the Decision Log (`ARCHITECTURE.md:926-1065`, columns `| Topic | Chosen | Rejected | Rationale |`), in the same change.
- Accepted direction: `plans/reports/brainstorm-261031-2108-next-stage-product-direction.md` (decisions D1-D14).

## Architecture decisions to record (authored by the Mac PM; workers transcribe, do not redesign)

| ID | Topic | Chosen | Rejected | Rationale |
|---|---|---|---|---|
| AD-1 | Tool layer | A **tool** is a typed operation `tool_id(inputs[], params) -> artifacts[]` (e.g. `pdf.merge`, `pdf.rotate`). Tools live in a registry in `ariad-core::tools`, separate from the conversion capability graph. Conversions stay routes; tools never enter the planner. | Forcing PDF→PDF ops into capability edges; one engine op per tool in `/1` | The planner forbids same-format edges by design; tools need N inputs and typed params. Keeps `/1` frozen. |
| AD-2 | In-process structural PDF crate | New crate `crates/ariad-pdf` (pure Rust on `lopdf`, `#![forbid(unsafe_code)]`, bounded by `Limits`, fuzzed) runs structural tools in-process on every runtime including `wasm32`. Joins the §1 exemption list alongside the native readers. | Running page-tree ops through an external C/C++ binary | Structural ops (merge/split/rotate/reorder/crop/metadata) are parser-light and must run client-side in the browser and offline on Mac. |
| AD-3 | Engine protocol v2 | `ariad-engine/2` = `/1` plus `op = "run_tool"` with `{tool, inputs: [{path, format}], output: {dir}, params, params_file?, work_dir, limits}` and `capabilities.tools[]`. The protocol an engine speaks is declared **statically** in `EngineSpec.protocol` (no runtime handshake). The runner is generalised over a request enum that exposes `output_dirs()`, `limits()`, `work_dir()`, `session_key()`, so sandbox writable roots, timeouts, the memory watchdog, TMPDIR, limit validation and artifact roots apply to `run_tool` exactly as to `convert`. Params larger than 256 KiB, and every secret, travel as files in `work_dir` (`params_file`), never inline (the request line stays ≤ 1 MiB). `/1` stays final and unchanged. `run_tool` never uses sessions. | Editing `/1` in place; a describe-time handshake per call; inline large params | Follows the major-version rule at `ARCHITECTURE.md:465`; runner `match` sites (`runner.rs:346-351, 398-424, 524-535, 879-885`) would otherwise silently drop limits and sandbox roots for new requests. |
| AD-4 | PDFium | PDFium is allowed as an **out-of-process engine** `pdfium` (render, text with positions, page objects, annotations, forms, redaction, in-place edit) shipped as a signed pack from pinned upstream binaries, and in the browser as a separately loaded, unmodified WASM file. Remove the unused `pdfium-render` workspace entry unless the engine uses it. | Linking PDFium into the host or Swift app process; MuPDF (AGPL) | PDFium is BSD-3/Apache-2.0 and is the only permissive engine with editing, forms and annotations at production quality. Out-of-process keeps C++ parser risk isolated. |
| AD-5 | qpdf | qpdf (Apache-2.0) as out-of-process engine pack `qpdf` for encryption/decryption/permissions, linearize and repair. | lopdf-only encryption | qpdf is the reference implementation for these and is permissive. |
| AD-6 | OCR to searchable PDF | Tesseract (Apache-2.0) out of process produces hOCR/text; AriadShift writes the invisible text layer itself in `ariad-pdf`. Apple Vision is used on macOS through the Swift surface calling the same core API. | OCRmyPDF + Ghostscript (AGPL); unpaper (GPL) bundled | Licence rules (AGENTS.md "Licensing"). |
| AD-7 | macOS surface | `apps/macos` (Xcode project, Swift 6, SwiftUI + AppKit) over a new crate `crates/ariad-ffi` (UniFFI bindings to `ariad-host` and `ariad-pdf`). Swift carries no document logic. Distribution: notarized Developer ID + Sparkle; not Mac App Store first. | Tauri on macOS; pure-Rust objc2 UI | Owner decision D4; research `researcher-261031-2046-macos-native-and-market.md`. |
| AD-8 | Windows/Linux | Native per platform later (WinUI 3, GTK). The existing Tauri app (`apps/desktop`) is maintenance-only (security and bug fixes, no new tools); its macOS build is retired when the Swift app ships (0.7.0); Windows/Linux keep the Tauri build until native apps replace it (owner, 2026-10-31). | Shipping new tools into Tauri | Owner decision D11; avoids a fourth UI to keep at parity. |
| AD-9 | Tool tiers | Every tool spec carries `tier: free \| pro`. The decision function is pure and lives in `ariad-core::tools::access` (`allowed(tier, plan) -> bool`) so CLI, MCP, server, `ariad-ffi` and `ariad-wasm` all call the same code. Host/server/macOS enforcement is authoritative; browser gating is advisory (documented: code running on the user's device can be modified). Named `ToolAccess`, distinct from the server's plan `Entitlement`; the server maps plans to tool access in phase 13. Billing is out of scope. | Per-surface gating; a host-only check (WASM cannot link `ariad-host`) | Owner decision D2 with one seam, no credits or meters. |
| AD-10 | IR evolution | Run-level and paragraph styles are added to the IR as an additive, optional extension with a new schema version `ariad-ir/2`; readers of `/1` documents keep working. Detailed in phase 11. | Overloading `provenance` for styles | Required for "edit PDF like Word" (D3, D8). |
| AD-11 | i18n | One source catalogue (English, ICU MessageFormat) in git; web and macOS catalogues are generated from it; 40+ locales at first preview, EN and VI human-reviewed, others machine-bootstrapped and marked "community". Library and TMS chosen in this phase from `researcher-261031-2115-ui-surface-tech.md`. | Separate hand-maintained catalogues per surface | Owner decision D12; one source of truth. |
| AD-12 | Digital signatures | PAdES signing/validation through pyHanko (MIT) in an out-of-process Python `sign` pack, gated like the Docling pack (`licenses.py`); RustCrypto primitives (`cms`, `x509-cert`, `rsa`, `p256`) only for in-process helpers; Swift `SecKey` callback for Keychain identities on macOS | A young Rust PAdES crate (underskrift); GPL `pdf_signer` | Only mature permissive PAdES B-B..B-LTA implementation (stack report §2.6). |
| AD-13 | PDF/A and PDF/UA validation | veraPDF under its MPL-2.0 option as an optional `verapdf` pack (Java runtime from a permissive JDK build); conversion claims conformance only after veraPDF passes | Claiming PDF/A without validation; Ghostscript PDF/A | Avoid false compliance claims (stack report §2.9, risk 1). |
| AD-14 | Engine packaging | Only PDFium gets a standalone binary crate, `crates/ariad-pdfium-engine` (pdfium-render, `libpdfium` loaded dynamically from the pack). qpdf and Tesseract are `ashift __engine qpdf\|tesseract` adapters in `ariad-host` that resolve a pack-shipped tool binary (the `pandoc_bin` pattern). Engine-side protocol helpers (`engines/common.rs`) move to a small shared crate `crates/ariad-engine-io` before the PDFium crate is created. One generic `engines::resolve_pack(id)` replaces the Docling special cases; pack name = engine id. | Three standalone engine crates re-implementing the protocol | Avoids triplicated protocol code; keeps C/C++ out of host processes (AGENTS.md). |
| AD-15 | Engine network | No engine ever gets network access. The trusted host fetches anything remote (TSA, OCSP, CRL, URL→PDF pages) with an SSRF-safe client — https only (http only for explicit localhost dev), DNS resolved once and rejected if any address is private/loopback/link-local/metadata, redirects re-checked, size and time caps — and passes the bytes to the engine as files. URL→PDF renders the fetched static HTML offline (HTML → IR → Typst); JavaScript-driven pages are not executed and get a warning. | Per-host engine allowlists (sandbox is all-or-nothing: `sandbox.rs:244`); headless Chromium | Closes SSRF and exfiltration through engines that parse attacker documents. |
| AD-16 | Secrets | Tool params marked `x-ariad-secret` (passwords, PKCS#12, keys) are never accepted inline on argv, in recipes, in URLs, in MCP arguments or in job rows. Accepted channels: CLI `--secret name=@file`/stdin prompt, MCP/HTTP via a short-lived secret handle, macOS Keychain, browser memory only. The host writes secrets to a `0600` file in the job dir passed as `params_file`, deletes it after the job, and scrubs secret values from engine stderr, errors, logs and reports. Recipes reject secret values (they may only reference `{"$secret": "prompt"}`). | Adding redaction later on top of inline params | Inline secrets leak through `ps`, shell history, recipe files, shared links and DB rows. |
| AD-17 | Edit API | Page-mode edits are stateless: `edit.apply {ops: [...]}` takes document bytes + an ordered op list and returns new bytes (incremental save optional). Document-mode editing happens in the surface editor (Tiptap on web, TextKit on macOS) with editor-native undo; the engine receives a full IR `/2` document on export. No engine session holds edit state; no `ir-patch` protocol. | Warm PDFium sessions per open document; a JSON-Patch IR protocol | `SessionPool` is pooled, reaped after idle and convert-only (`session.rs:38-53`, ARCHITECTURE §7). |
| AD-18 | Untrusted recipes | A recipe from a file, link, deep link or Shortcut is untrusted: surfaces show its steps and require confirmation; output-name templates allow only `[A-Za-z0-9._ -]` plus tokens and are joined under the confined output dir; imported recipes cannot add watch-folder or network-fetch steps. Deep links stay route-only (no paths, no recipes). | Executing shared recipes directly | Prevents link-triggered local execution and path traversal. |

## Files to create / modify

- Modify: `ARCHITECTURE.md` — §1 principles (exemption list), §2.3 crates table (`ariad-pdf`, `ariad-ffi`, `ariad-pdfium-engine`, `ariad-engine-io`), §2.4 engines table (`pdfium`, `qpdf`, `tesseract`, `sign` (pyHanko), `verapdf`, `ai`, `inpaint`), §5 product surfaces (`apps/macos`, Tauri maintenance-only), §7 add "7.3 Engine protocol v2" (request/events/rules, examples), §7.1 packs (new pack names), §11.1 threat model rows for PDFium/qpdf/Tesseract (embedded JavaScript never executed; forms/annotations parsing out of process), §15 licensing rows (PDFium moves from "Docling pack and bench only" to "out-of-process engine pack + browser WASM"; qpdf; Tesseract engine), §18 Decision Log rows AD-1..AD-18, §20 open questions (Pro list, LaMa weights, min macOS).
- Modify: `AGENTS.md` "Current state" — one paragraph on the next stage and links; "Architecture changes" — mention tools and `/2`.
- Create: `docs/tools.md` — tool contract spec (see T2).
- Create: `docs/parity-matrix.md` — feature-parity matrix (see T3).
- Create: `docs/i18n.md` — locale list, catalogue format, pipeline (see T4).
- Create: `fixtures/gen/src/ariad_fixture_gen/pdf_tools.py`, new fixture files under `fixtures/pdf-tools/`, entries in `fixtures/manifest.toml` (see T5).
- Modify: `Cargo.toml` — remove the unused `pdfium-render` entry only if T1 confirms no crate uses it (`rg -n "pdfium" crates apps fuzz`).

## Tasks

| Task | Owner (model) | Files owned | Work |
|---|---|---|---|
| T1 | Gemini 4 Argon high | `ARCHITECTURE.md`, `AGENTS.md`, root `Cargo.toml` (pdfium-render line only) | Transcribe AD-1..AD-18 exactly; write §7.3 protocol v2 with the request/event JSON examples in this file; update every section listed above; keep line style of the Decision Log. |
| T2 | Gemini 4 Argon high | `docs/tools.md` | Tool contract spec. |
| T3 | Claude Sonnet 5.5 high | `docs/parity-matrix.md` | Parity matrix with proposed Free/Pro tier. |
| T4 | Claude Sonnet 5.5 high | `docs/i18n.md` | i18n policy, locale list, pipeline choice. |
| T5 | Gemini 4 Argon high | `fixtures/gen/src/ariad_fixture_gen/pdf_tools.py`, `fixtures/pdf-tools/**`, `fixtures/manifest.toml` (append only) | Generated permissive PDF test corpus. |

T1-T5 own disjoint files and run in parallel in one worktree.

### T1 steps (ARCHITECTURE.md)

1. Read `ARCHITECTURE.md` §1, §2.3, §2.4, §5, §7, §7.1, §11, §15, §18, §20 fully before editing.
2. Add the §7.3 section with these canonical examples (field names are normative):
   ```json
   {"protocol":"ariad-engine/2","job":"job_01K…","op":"run_tool","tool":"pdf.rotate",
    "inputs":[{"path":"/work/in/0.pdf","format":"pdf"}],
    "output":{"dir":"/work/out"},"params":{"pages":"1-3,5","angle":90},
    "work_dir":"/work/tmp","limits":{"max_pages":null,"timeout_s":null,"max_memory_mb":null,"max_nesting_depth":64}}
   ```
   Events are the `/1` set; `capabilities` gains `"protocol":"ariad-engine/2"` and `"tools":[{"id":"pdf.rotate","version":"1"}]`; a `run_tool` may emit several `artifact` events before the single `result`. Error codes stay closed; add `unsupported_tool` only if the T2 spec proves `unsupported_route` cannot express it (default: reuse `unsupported_route`).
3. Add 18 Decision Log rows (AD-1..AD-18) at the end of the table, one line each, same column order.
4. Update §15 licensing table rows; keep the Excluded paragraph and add Ghostscript, MuPDF, OCRmyPDF-with-Ghostscript, unpaper and BentoPDF code explicitly.
5. Run `just spell` and `just fmt-check`; fix only your files.

### T2 steps (docs/tools.md)

Specify, with examples:
- Tool id grammar `^[a-z]+(\.[a-z0-9_]+)+$` (`pdf.merge`, `pdf.page.rotate` style is NOT used — flat `pdf.rotate`), version per tool (`"1"`), group (`organize`, `convert`, `optimize`, `security`, `overlay`, `ocr`, `annotate`, `forms`, `sign`, `redact`, `compare`, `edit`, `ai`, `automation`).
- Inputs: arity (`one`, `many(min,max)`), accepted formats, ordering semantics (merge order = input order).
- Params: JSON object validated against a per-tool JSON Schema generated from Rust types (`schemars`); unknown fields rejected; defaults explicit.
- Page-range grammar (normative, shared by every tool): `all | even | odd | N | N-M | N- | -M | last | last-K`, comma-separated, 1-based, duplicates kept in order for reorder tools, rejected for others; errors name the offending token.
- Outputs: artifact naming (`<stem>.<tool-suffix>.<ext>`, e.g. `report.rotated.pdf`; split → `<stem>.part-001.pdf`), never overwrite inputs, atomic write (temp + rename).
- Limits honoured per tool (`max_pages`, `max_memory_mb`, `timeout_s`, decoded stream caps), progress unit = pages.
- Determinism: same inputs + params + `SOURCE_DATE_EPOCH` → byte-identical output for in-process tools (IDs, dates). Golden tests rely on it.
- Recipe format: `{"version":"ariad-recipe/1","steps":[{"tool":"pdf.merge","params":{}},{"convert":{"to":"docx"}}]}` executed sequentially, output of step N is input of N+1.
- Tier field and the access check (AD-9: pure `ariad-core::tools::access`; browser gating advisory).
- Secrets (AD-16): `x-ariad-secret` schema marker, accepted channels per surface (table: CLI / local MCP / remote MCP / web / macOS / recipe → allowed channel or "rejected"), `params_file` delivery, scrubbing rules.
- Large params (> 256 KiB) travel as `params_file` (AD-3).
- Untrusted recipes (AD-18): confirmation, name-template charset, forbidden steps for imported recipes.
- Tool error exit codes that do not reuse existing ones (`docs/cli.md:37-46` uses 0-6, 130): 8 unknown tool, 9 invalid params, 10 wrong input count/format, 11 not allowed by tier, 12 encrypted input needs a password. MCP error mapping mirrors them.
- MCP tool names: `list_doc_tools` and `run_doc_tool` (avoid confusion with the MCP protocol method `tools/list`).
- Multi-artifact outputs are staged in the job workspace and published as a set into the confined output directory; on any failure nothing is published (all-or-nothing).

### T3 steps (docs/parity-matrix.md)

1. Read `plans/reports/researcher-261031-2046-stirling-pdf.md` §2 and `plans/reports/researcher-261031-2046-bentopdf.md` tool inventory.
2. One table row per competitor capability: `AriadShift tool id | group | Stirling | Bento | phase (2-20) | engine (ariad-pdf/pdfium/qpdf/tesseract/libreoffice/pandoc/typst/docling/ai-pack) | runtimes (wasm/local/cloud) | proposed tier | surfaces (CLI/MCP/web/mac)`.
3. Add a second table "Beyond parity" for AriadShift-only items from the brainstorm (fidelity score, verified redaction report, Document/Page edit modes, NL recipes, semantic compare, crash-safe versions, watermark removal L1/L2, Vietnamese-first OCR, target-size compression).
4. Tier proposal rule (owner decision D2 "some tools are Pro"): propose Pro only for (a) cloud/AI-cloud execution, (b) certificate signing with hardware tokens, (c) batch/automation above a stated local size, (d) Page-mode in-place editing. Everything else Free. Mark every Pro row `proposed` — the owner approves the list at the phase gate.
5. Count rows; every Stirling feature in the report must appear (or be listed under "Not adopted" with a reason, e.g. PDF-to-video disabled upstream).

### T4 steps (docs/i18n.md)

1. Read `plans/reports/researcher-261031-2115-ui-surface-tech.md` (sections A1, A2, B10).
2. Record: library choice for web, catalogue format (ICU MessageFormat JSON, English source at `locales/en.json` — path finalised here), generation into macOS String Catalogs, TMS choice, machine-translation bootstrap rule, review status per locale, RTL support, pseudo-locale for tests, string-freeze rule before each preview.
3. Locale list for the first preview: start from Stirling's 40+ list (cite it), add `vi`; mark `en`, `vi` as reviewed.

### T5 steps (test corpus)

Generate (never download third-party documents) with the existing generator package (`fixtures/gen`, run via `just fixtures`), licence `Apache-2.0`, `origin = "generated"`, one `[[fixture]]` per file with `sha256`, `pages`, `tags`, `phase` (required) and `routes = []`. `tags` is a closed list (`ALLOWED_TAGS` in `crates/ariad-cli/tests/support/fixtures.rs:24-54`): add the new tags there in the same commit. Add `pikepdf` (MPL-2.0) to `fixtures/gen/pyproject.toml` as a dev/test-only dependency (never distributed). **Encrypted fixtures are not committed** (AES uses random IVs, so bytes are not reproducible): generate them at test time from a committed plaintext source and record only the plaintext hash. Files:
- `multipage-20.pdf` (numbered pages, outlines, page labels), `mixed-sizes.pdf` (A4/Letter/A3 landscape), `rotated-pages.pdf` (/Rotate 90/180/270 + inherited rotate), `encrypted-aes256.pdf` (user+owner password), `encrypted-owner-only.pdf` (empty user password, owner password restricting print/copy — the common case lopdf silently decrypts), `encrypted-rc4-40.pdf` (all three generated at test time, passwords in the test helper, not in committed truth files), `form-acroform.pdf` (text, checkbox, radio, combo, signature field), `annotations.pdf` (highlight, note, ink, link), `with-attachments.pdf`, `incremental-update.pdf` (two revisions), `broken-xref.pdf` (repairable), `object-streams.pdf` (PDF 1.5 xref streams), `vi-diacritics-subset-font.pdf` (Vietnamese text with subset TrueType), `watermark-layer.pdf` (OCG watermark), `watermark-artifact.pdf` (`/Artifact /Watermark` marked content), `watermark-repeated-xobject.pdf`, `scanned-skewed-vi.pdf` (raster page with 3° skew), `blank-separators.pdf`, `javascript-action.pdf` (OpenAction JS — must never execute), `tagged-actualtext.pdf` (structure tree with `/ActualText` and `/Alt` carrying a personal-data string, for redaction tests), `form-with-values.pdf` (field values containing a personal-data string), `large-1000-pages.pdf` (generated at test time by a helper, not committed if > 5 MB).
- Each file gets a `*.truth.json` companion describing expected facts (page count, sizes, rotations, field names, watermark kind) used by later phases' tests.

## Verification

```sh
source /Volumes/AriadWork/02-Ariadnev-Eco/.cache/env.sh
just spell && just fmt-check
just fixtures            # regenerates; git diff must be empty after a second run (determinism)
rg -n "pdfium" crates apps fuzz Cargo.toml   # matches the T1 decision
```

## Acceptance (gate to the Mac PM, then owner for tiers)

- [x] ARCHITECTURE.md contains AD-1..AD-18 in the Decision Log, a §7.3 for `ariad-engine/2`, and no remaining sentence saying PDFium is Docling/bench-only (AD rows verified verbatim by script; merged in PR #125).
- [x] `docs/tools.md` defines ids, params, page ranges, outputs, limits, determinism, recipes and tiers with examples (`docs/tools.md` merged in PR #125).
- [x] `docs/parity-matrix.md` covers 100 % of the Stirling feature table rows and Bento tools (or lists them under "Not adopted" with a reason); Pro rows are marked `proposed` (`docs/parity-matrix.md` merged in PR #125).
- [x] Owner approved the Pro list (recorded in `docs/parity-matrix.md` with the date) (Pro list approved by the Mac PM on the owner's delegation on 2026-11-01, recorded in `docs/parity-matrix.md`).
- [x] `docs/i18n.md` fixes library, catalogue path, TMS, locale list (`docs/i18n.md` merged in PR #125).
- [x] The corpus regenerates deterministically and every file has a truth companion and a manifest entry (corpus: 22 PDFs with truth files and manifest entries, deterministic across two runs of `just fixtures`; merged in PR #125).

## Risks

| Risk | Mitigation |
|---|---|
| Worker redesigns a decision | The AD table is normative; reviewer rejects diffs that change Chosen/Rejected text. |
| Corpus generator cannot produce encryption/forms | Use `pikepdf` (MPL-2.0, dev/test-only, not distributed) inside `fixtures/gen` only; record it as a dev-only dependency; never ship it. |
| Parity matrix misses Stirling Pro-only features | T3 step 5 counts rows against the report table. |

Gate decisions applied 2026-11-01: batch cap 25 files or 250 MB enforced by within_run_quota in phase 13; the 14 dagger rows were assigned to phases 3, 4, 6, 7, 9, 10 and 13 and the PM adds their tasks at each phase scout.
