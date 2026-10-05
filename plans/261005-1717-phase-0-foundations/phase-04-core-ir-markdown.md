---
phase: 4
title: "ariad-core: IR v0, Markdown reader, Pandoc AST"
status: pending
priority: P1
effort: "14h"
dependencies: [2, 3]
---

# Phase 4: ariad-core — IR v0, Markdown reader, Pandoc AST

## Goal

Implement the I/O-free, wasm-compilable core of the route in `ariad-core`:

- IR v0 (`ariad-ir/0`) types with a generated JSON Schema;
- a format registry;
- `Limits` with explicit "unlimited" semantics;
- a GFM Markdown → IR reader on comrak, with emoji shortcodes and YAML front matter;
- serialize-only Pandoc AST types (`pandoc-api-version` 1.23);
- an IR → Pandoc AST mapping that reports what it could not carry and never emits unsafe content.

## Context links

- [ARCHITECTURE.md](../../ARCHITECTURE.md) §6.1, §6.2, §11.2 (exact contracts written in phase 1)
- [Toolchain research](./research/researcher-01-toolchain-report.md) Q2, Q3, Q7
- [Phase 3](./phase-03-fixture-suite.md) fixtures (`fixtures/md/`)

## Key insights

- **comrak 0.55** (`default-features = false`, feature `shortcodes`, pure Rust):
  - Source positions are always on AST nodes. `sourcepos` is only a *render* option.
  - comrak silently stops recognizing list markers deeper than `MAX_LIST_DEPTH = 100`. Our `max_nesting_depth` is 64, so the IR walk errors before comrak ever flattens anything. A validator in `Limits` rejects values ≥ 100.
  - Front matter needs `front_matter_delimiter = "---"`. comrak then exposes the raw YAML as a node, which we parse ourselves.
- **Serde shape:** every IR enum variant is a struct variant with `#[serde(tag = "type")]`. Internally tagged enums cannot carry tuple variants (compile error) or newtype variants wrapping sequences or strings (runtime error).
- **Pandoc AST:** this phase only **writes** it. Deserialization and the API-version gate wait for the roadmap 1a readers. Phase 5 proves the contract instead: Pandoc accepts our JSON through `-f json`.
- **`--sandbox` Pandoc cannot read image files.** The mapping embeds assets as `data:` URIs. An image without an embedded asset becomes its alt text plus a warning.
- **Unsafe content never reaches the writer.**
  - `Raw.format` is a closed enum (`html`, `tex`). Only the mapper's own page-break constant produces `openxml`.
  - Link schemes are allow-listed: `http`, `https`, `mailto`, and in-document `#` anchors. Any other link is emitted as plain text with a warning. This blocks `file:`, UNC paths and `javascript:` in a document that opens in Word.
- **NFC applies to prose only**: text runs, alt text, link text, metadata. `Code`, `Math`, `Raw` and URLs are never rewritten.
- **YAML front matter is untrusted input.**
  - Cap it at 64 KiB.
  - Reject anchors and aliases, which enable billion-laughs attacks.
  - Accept only scalar or list-of-scalar values for known keys: `title`, `author`/`authors`, `lang`, `date`, `subject`, `keywords`.
  - Unknown keys get a warning. Invalid YAML gets a warning and the front matter is ignored; it is never fatal.

## Requirements

Functional:
- `markdown::read(&str, &Limits) -> Result<ReadOutput { document, warnings }, ReadError>`.
  - Covers CommonMark + GFM: tables, strikethrough, autolinks, task lists, footnotes, dollar math, raw HTML, emoji shortcodes, front matter.
- `pandoc::from_ir(&Document) -> MapOutput { pandoc, warnings }`.
  - `Metadata` maps to Pandoc meta (`title`, `author`, `lang`, `date`, `subject`, `keywords`). Pandoc writes these into `docProps/core.xml`.
- `Document` round-trips losslessly through serde_json.
- `schemas/ir.v0.json` is generated (draft 2020-12, `$id` `https://ariadshift.ariadnev.com/schemas/ir.v0.json`). A drift test guards it.

Non-functional:
- `#![forbid(unsafe_code)]`; compiles for `wasm32-unknown-unknown`.
- No panics on any input.
- Bounded recursion: the AST walk is iterative, and nesting depth and block count are limited.

## Architecture

```text
crates/ariad-core/src/
├── lib.rs
├── ir/mod.rs              Document, Metadata, Block, Inline, Table, ListItem, AssetRef, AssetStore, LayoutIndex, Provenance, IR_VERSION
├── format.rs              Format: Markdown, Html, Docx, AriadIrJson, PandocJson (id, extensions, media type, from_extension)
├── limits.rs              Limits (see below) + local()/cloud() + validate()
├── warning.rs             Warning { code: WarningCode, message, source_pos? }
├── reader/markdown.rs     comrak → IR (the only file importing comrak)
├── reader/front_matter.rs YAML → Metadata (the only file importing the YAML crate)
└── pandoc/{mod.rs, ast.rs, from_ir.rs}   serialize-only AST + mapping
schemas/ir.v0.json
fixtures/golden/<id>.ir.snap
```

**`Limits`** (serde + schemars; `None` and an absent JSON field both mean unlimited):

| Field | Type | `local()` | `cloud()` |
|---|---|---|---|
| `max_input_bytes` | `Option<u64>` | None | 2 GB |
| `max_pages` | `Option<u32>` | None | 5,000 |
| `timeout_s` | `Option<u64>` | None | 600 |
| `max_memory_mb` | `Option<u32>` | None | 4,096 |
| `max_asset_bytes` | `Option<u64>` | None | 50 MB |
| `max_nesting_depth` | `u16`, always finite | 64 | 64 |
| `max_blocks` | `u32`, always finite | 10,000,000 | 1,000,000 |
| `max_front_matter_bytes` | `u32`, always finite | 64 KiB | 64 KiB |

`validate()` rejects `max_nesting_depth` ≥ 100, and rejects 0 for any `Some` value.

**IR v0 shape:** exactly as written into ARCHITECTURE §6.2 by phase 1. All variants are struct variants. `AssetRef` is `Asset { id }` or `Url { href }`. `Raw.format` is `RawFormat::{Html, Tex}`.

**IR → Pandoc mapping:**

| IR | Pandoc |
|---|---|
| Heading | `Header` |
| Paragraph | `Para` |
| List | `BulletList` / `OrderedList` (`Decimal`/`Period`, start) |
| Task item | Prefix `☐ `/`☒ ` (as Pandoc's gfm reader does) |
| Table | Pandoc 1.23 table model (alignments, spans, head and body) |
| Figure | `Figure` |
| Code | `CodeBlock` with class |
| Math | `Math` (`DisplayMath`/`InlineMath`) |
| Quote | `BlockQuote` |
| Footnotes | `Note` placed at the reference. A missing definition keeps the marker and warns. An unused definition warns |
| PageBreak | Mapper constant `RawBlock "openxml"` (page break) |
| Raw `html`/`tex` | `RawBlock`/`RawInline` with that format. The docx writer drops them; the mapper warns `raw_dropped` when the target is docx |
| Link, allowed scheme | `Link` |
| Link, other scheme | Link text + warning `link_dropped` |
| Image `Asset` | `Image` with a `data:<media_type>;base64,…` URL |
| Image `Url` | Alt text + warning `image_not_embedded` |
| Metadata | `meta` (`MetaInlines`/`MetaList`/`MetaString`) |

## Files

| Action | File | Size | Test impact |
|---|---|---|---|
| Modify | `crates/ariad-core/Cargo.toml` | serde, serde_json, schemars, thiserror, comrak (shortcodes), YAML crate, unicode-normalization, sha2, base64; dev: insta (json, glob), jsonschema | — |
| Create | `crates/ariad-core/src/{ir/mod.rs,format.rs,limits.rs,warning.rs}` | ~500 | unit |
| Create | `crates/ariad-core/src/reader/{mod.rs,markdown.rs,front_matter.rs}` | ~520 | unit + snapshots |
| Create | `crates/ariad-core/src/pandoc/{mod.rs,ast.rs,from_ir.rs}` | ~550 | unit |
| Create | `crates/ariad-core/tests/{ir_snapshots.rs,schema_drift.rs,limits.rs}` | ~220 | integration |
| Create | `schemas/ir.v0.json` | generated | drift |
| Create | `fixtures/golden/*.ir.snap` | insta | snapshots |
| Modify | `deny.toml` | Allow `BSD-2-Clause` and the YAML crate's license if needed | deny |
| Modify | `ARCHITECTURE.md` | Only if implementation forces a contract change (same commit + Decision Log) | — |

## Implementation steps

1. **YAML crate.**
   - Choose a maintained, pure-Rust, permissive YAML parser that compiles to wasm32 and exposes anchors and aliases so we can reject them. Candidates: `saphyr`, `yaml-rust2`; do not use the deprecated `serde_yaml`.
   - Verify it on crates.io (version, license, last release).
   - Record it in ARCHITECTURE §2.3 in the same commit.
2. `limits.rs`: implement the table above, with `validate()`. Doc comment: the cloud plan entitlements (§9.5) fill this shape.
3. `ir/mod.rs`:
   - All struct variants, `#[serde(tag = "type", rename_all = "snake_case")]`.
   - `IR_VERSION = "ariad-ir/0"`.
   - `AssetStore` is a `BTreeMap`, so output stays deterministic.
4. `format.rs`: registry plus case-insensitive `from_extension` (`md`, `markdown`, `mdown`, `html`, `htm`, `docx`, `json`).
5. **`reader/markdown.rs`.**
   - Enforce `max_input_bytes` when set. Strip a BOM and normalize CRLF/CR to LF.
   - comrak options: GFM extensions, `math_dollars`, `footnotes`, `tasklist`, `shortcodes`, `front_matter_delimiter = "---"`.
   - Walk the AST **iteratively** with an explicit stack into IR. Return `NestingTooDeep` above `max_nesting_depth` and `TooManyBlocks` above `max_blocks`.
   - Normalize prose to NFC. Images become `AssetRef::Url`. Map unsupported nodes to the closest block, with a warning.
6. **`reader/front_matter.rs`.**
   - Enforce the size cap first.
   - Parse, and reject anchors and aliases.
   - Map the known keys into `Metadata` (NFC). `author` may be a string or a list.
   - Unknown keys produce `front_matter_key_ignored`. A parse error produces `front_matter_invalid` and the front matter is ignored.
7. **Stack-safety tests.** No panic, a typed error, and completion under 1 s in debug for each of:
   - 100,000 nested `>`;
   - 100,000 nested list markers (expect `NestingTooDeep` at depth 65);
   - 50,000 `[`;
   - 50,000 `*` emphasis openers;
   - a 10,000-column table row.

   If comrak itself overflows the stack (the test thread crashes), add a linear pre-scan that rejects container, bracket and emphasis nesting beyond the limit before parsing. Fuzzing comes in 1a.
8. **`pandoc/ast.rs`.** `Serialize`-only types for `Pandoc { pandoc_api_version: [1,23], meta, blocks }`, `Block`, `Inline`, `MetaValue`, `Attr` and the 1.23 `Table`, using `#[serde(tag = "t", content = "c")]`.
9. **`pandoc/from_ir.rs`.** Implement the mapping table with warnings. Output is deterministic: ids follow document order. Link schemes are checked with a small parser, not a regex: `http`, `https`, `mailto`, and an empty scheme beginning with `#`.
10. **Schema drift test (`schema_drift.rs`).**
    - Generate with `SchemaSettings::draft2020_12()` plus `$id`, and compare with `schemas/ir.v0.json`.
    - With the dev-only `ARIAD_BLESS_SCHEMAS=1`, rewrite the file instead.
    - Output: 2-space indent and a trailing newline.
11. **IR snapshot test (`ir_snapshots.rs`).**
    - `insta::glob!("../../../fixtures/md/*.md")`, with the snapshot path set to `fixtures/golden/` and names `<stem>.ir`.
    - **Assert at least 20 files matched**, so the test can never pass on zero inputs.
    - Validate each IR against `schemas/ir.v0.json` with `jsonschema`.
12. **Unit tests:**
    - each GFM feature, plus shortcodes and front matter (the title/authors/lang/date mapping, aliases rejected, an oversize cap, unknown keys, invalid YAML);
    - NFC on prose only (code is untouched);
    - CRLF and LF give identical IR; BOM handling;
    - each limit and `validate()`;
    - each mapper table row, including `link_dropped` for `file:`, `javascript:` and `\\host\share`;
    - `Raw` cannot be built with an `openxml` format, because the type does not allow it;
    - exact JSON for small AST samples compared with hand-checked expected strings.
13. Run `just wasm` and `just ci`.

## Todo

- [ ] YAML crate chosen, verified, recorded in §2.3
- [ ] Limits (Option semantics, validate)
- [ ] IR v0 types (struct variants), Format, Warning
- [ ] Markdown reader (iterative, NFC prose-only, CRLF, BOM, shortcodes, limits)
- [ ] Front matter parser (cap, no aliases, known keys)
- [ ] Stack-safety tests (+ pre-scan if needed)
- [ ] Serialize-only Pandoc AST
- [ ] Mapping with link-scheme allow-list, closed Raw formats, metadata
- [ ] `schemas/ir.v0.json` + drift test
- [ ] IR snapshots (≥ 20 matched) + schema validation
- [ ] wasm32 check + `just ci` green on 3 OSes

## Test scenario matrix

| Priority | Scenario | Expected |
|---|---|---|
| Critical | All `fixtures/md/*.md` | Snapshots match, schema-valid, ≥ 20 matched |
| Critical | NFD fixture | Prose in NFC |
| Critical | 100k nested lists or quotes | `NestingTooDeep`, no crash |
| Critical | `[x](file:///etc/passwd)`, `[x](\\host\s)` | Text + `link_dropped` |
| Critical | YAML with aliases (billion laughs) | Rejected with a warning, fast |
| High | 64-level nested list fixture | Accepted (exactly at the limit) |
| High | Front matter title and authors | In `Metadata` and in the Pandoc meta |
| High | `:smile:` | 😄 in IR text |
| High | Footnote ref without a definition | Marker kept + warning |
| High | Stale schema file | Drift test fails with the bless hint |
| Medium | wasm32 build | Compiles |

## Success criteria

- All Markdown fixtures snapshot and validate.
- Every mapping row is tested.
- No unsafe content can be emitted.
- `just ci` is green on three OSes.

## Risk assessment

| Risk | Mitigation |
|---|---|
| comrak API churn | One module; lockfile pin |
| comrak overflows the stack on hostile input | Step 7 tests + pre-scan; fuzzing in 1a |
| The YAML crate lacks alias detection | It is a selection criterion in step 1; otherwise reject any `&`/`*` token by a pre-scan of the front matter |
| IR changes during the work | Contract changes update ARCHITECTURE §6.2 in the same commit |

## Security considerations

- The reader parses untrusted input in-process, under the Principle 3 exception recorded in phase 1: no unsafe code, typed errors, depth, block and front-matter limits, iterative walking.
- The mapper is the boundary that keeps OOXML and dangerous link schemes out of the DOCX.

## Next steps

Phase 5 puts IR on the wire, resolves assets and runs Pandoc.
