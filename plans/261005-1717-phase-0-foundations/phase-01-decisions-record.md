---
phase: 1
title: "Record decisions in ARCHITECTURE and AGENTS"
status: pending
priority: P1
effort: "4h"
dependencies: []
---

# Phase 1: Record decisions in ARCHITECTURE and AGENTS

## Goal

Make ARCHITECTURE.md and AGENTS.md agree with every decision taken on 2026-10-06 before any code exists, including the exact IR v0, protocol, limits and CLI contracts that phases 4–6 implement. Later phases then build against a document that is already correct. AGENTS.md's rule ("update ARCHITECTURE.md, including its Decision Log, in the same change") holds from the first commit.

## Context links

- [ARCHITECTURE.md](../../ARCHITECTURE.md) §2–§7, §9, §11, §14–§18, §20
- [AGENTS.md](../../AGENTS.md)
- [Toolchain research](./research/researcher-01-toolchain-report.md) Q1–Q7
- [OCR and fixtures research](./research/researcher-02-ocr-fixtures-report.md) §1, §4.1
- [plan.md](./plan.md): user decisions, validation log, red-team review

## Key insights

- `ariad` appears as the **binary** in §3, §4, §5, §6.3, §7, §7.1, §17 and §20. Only the binary and user-facing env vars change, to `ashift` and `ASHIFT_*`. Crates, `ariad-ir` and `ariad-engine/1` keep their names.
- The current OCR choice is a correctness defect: PP-OCRv5/v6 dictionaries lack 88–90 of the 90 letters in U+1EA0–U+1EF9. The Decision Log row at §18 "OCR | RapidOCR (ONNX)" must be **replaced**, not appended to, so no conflicting row survives.
- §11.2 keeps local inputs, time and memory unlimited by default (user decision). Stack safety still needs a finite nesting depth and block count, which §11.2 does not list yet.
- Phase 0 parses Markdown in the host process (`ariad-core`, pure Rust). That deviates from Principle 3 ("all untrusted file parsing runs in sandboxed child processes"), so it must be recorded as an explicit exception with its compensating controls.

## Requirements

Functional: every edit below lands in ARCHITECTURE.md or AGENTS.md, and every new or changed choice gets a Decision Log row.
Non-functional: English only, no new root markdown, no section renumbering, and no phase-status notes in ARCHITECTURE.md (status lives in plans).

## Files

| Action | File | Change | Size |
|---|---|---|---|
| Modify | `ARCHITECTURE.md` | Sections listed in the steps | ~+180 / −40 lines |
| Modify | `AGENTS.md` | Binary name, fixture and model license rules, commands | ~+10 lines |

## Implementation steps

1. **Binary rename (§3, §4, §5, §6.3, §7, §7.1, §17, §20).**
   - `ashift` replaces the `ariad` command everywhere: the §3 diagram nodes (`CLI: ashift`, `MCP server: ashift mcp`), §4 surface table and commands, §5 tree comment for `ariad-cli/`, §6.3 sample, §7 `ashift __engine pdfium`, the §7.1 `core` pack contents, and §17 1b `ashift engines install`.
   - In §20, mark Q2 resolved: `as` collides with GNU binutils `as`.
2. **§2 versions and tooling.**
   - §2.1: CI runner labels `ubuntu-26.04`, `macos-26` and `windows-2025` are pinned explicitly; `ubuntu-latest` is 24.04 until November 2026.
   - Pandoc 3.12 is installed from release binaries with SHA-256 pinning by `scripts/install-pandoc.sh`; the Arch package is 3.11.
   - §2.3 new rows:
     - comrak 0.55 (`default-features = false` plus the pure-Rust `shortcodes` feature, BSD-2-Clause)
     - a YAML front-matter parser (chosen and verified in phase 4; pure Rust and permissive)
     - process-wrap 10, tokio-util 0.7, unicode-normalization 0.1, sha2, base64, cap-std, zip (runtime, for the DOCX timestamp rewrite), and serde_stacker for deep IR JSON
     - one row of dev-only test crates: insta (json, glob), jsonschema, assert_cmd, quick-xml, toml, hex
   - §2.4: add the Python fixture generator stack (python-docx, Pillow, typst-py), each marked "dev-only, never distributed".
3. **OCR (§2.4, §3, §6.4, §7.1, §15, §18, §20).** Apply the OCR research §1:
   - Tesseract 5.5.3 + `tessdata_best` `vie`/`eng` is the default OCR for any job whose languages include `vi`.
   - RapidOCR 3.9.2 / PP-OCRv6 covers other languages only, and `vi` is never routed to it.
   - PaddleOCR-VL-1.6 via llama.cpp is an opt-in accurate path.
   - Granite-Docling is English-only.
   - The §3 engines node reads "Docling + Tesseract / RapidOCR".
   - §6.4 scan row: Tesseract for `vi`, RapidOCR otherwise.
   - §7.1: Tesseract + `vie`/`eng`/`osd` (~30 MB) go **into the `docling` pack** (user decision). Other Tesseract languages stay in `ocr-extra`. Add an opt-in `ocr-vlm` pack.
   - §15: rows for Tesseract + tessdata, llama.cpp and PaddleOCR-VL weights; extend "Excluded" to model weights under OpenRAIL-M, custom or missing licenses.
   - **Replace** the §18 OCR row with: "Tesseract (vi) + RapidOCR (others) + PaddleOCR-VL opt-in | RapidOCR-only, Surya/Chandra (OpenRAIL-M weights), EasyOCR | PP-OCR dictionaries cannot emit Vietnamese tone letters".
   - Mark §20 Q3 resolved (vi + en).
4. **§6.1 crate responsibilities.**
   - `ariad-core` also owns the engine protocol message types (pure data, shared by host, browser and server).
   - `ariad-host` owns asset resolution, workspaces, the runner, the Pandoc engine, and the DOCX metadata rewrite.
5. **§6.2 IR v0, the exact contract phase 4 implements.**
   - Version id `ariad-ir/0` and schema `schemas/ir.v0.json` while 0.x; breaking changes are allowed. It freezes as `ariad-ir/1` together with the protocol in roadmap 1b (user decision).
   - **Every enum variant is a struct variant**, serialized as `{"type": …}`. For example `Paragraph { content }`, `Quote { blocks }`, `AssetRef::Url { href }`. Serde's internally tagged form cannot carry tuple or newtype-of-sequence variants.
   - `Document { version, meta, body, assets, layout, provenance }`, where `Metadata { title, authors, language, date, subject, keywords, source_format }`.
   - Blocks:
     - `Heading`, `Paragraph`, `Code`, `Math`, `Quote`, `PageBreak`
     - `List { ordered, start, tight, items: [ListItem { checked, blocks }] }`
     - `Table { caption, columns: [ColumnSpec { align }], head, body }` with cells `{ rowspan, colspan, blocks }`
     - `Figure { asset: AssetRef, caption }`
     - `Footnote { id, blocks }`
     - `Raw { format, text }`
   - Inlines:
     - text and formatting: `Text`, `Emph`, `Strong`, `Strikeout`, `Superscript`, `Subscript`, `Code`
     - links and media: `Link { url, title, content }`, `Image { target, alt, title }`
     - breaks: `SoftBreak`, `LineBreak`
     - other: `Math { tex, display }`, `FootnoteRef { id }`, `Raw { format, text }`
   - `Raw.format` is a **closed enum** (`html`, `tex`). IR content can never carry OOXML.
   - `AssetRef` is `Asset { id }` (lowercase hex SHA-256) or `Url { href }`. `AssetStore` is an ordered map of `id → { media_type, bytes(base64) }`.
   - Prose text (text runs, alt text, link text, metadata strings) is normalized to NFC. `Code`, `Math`, `Raw` and URLs are never rewritten.
   - Front matter (YAML between `---` lines) fills `Metadata`.
   - Assets referenced by the source are resolved by the host; `ariad-core` stays I/O-free.
6. **§7 engine protocol (draft).**
   - Update the request example to the final fields:
     - `protocol`, `job`, `op`
     - `input { path, format }`, `output { dir, format }`, `work_dir`, `options`
     - `limits`, whose unlimited fields are omitted or `null`
   - Add the **outcome rule**, replacing "non-zero exit without result = crash":
     - `result ok:true` + exit 0 is success.
     - `result ok:false` + any exit is a typed engine failure.
     - No `result`, or `result ok:true` with a non-zero exit, is a crash.
     - Any line after `result` is a protocol violation.
   - Add the Pandoc engine contract:
     - command line: `pandoc [+RTS -M{max_memory_mb}M -RTS] --sandbox --log=<work_dir>/pandoc-log.json -f json -t docx`
     - AST on stdin; stdout discarded; stderr drained and bounded
     - `TMPDIR`/`TMP`/`TEMP` point to `<work_dir>`
     - assets embedded as `data:` URIs; log entries map to `warning` events
     - `--sandbox` is a Pandoc guard, not OS isolation
   - After Pandoc, the host rewrites `docProps/core.xml` created/modified to the conversion time, honoring `SOURCE_DATE_EPOCH` (user decision).
   - Add an open question: loopback access to an engine's own child (`llama-server` in `ocr-vlm`).
7. **§11.2 limits.**
   - Add rows: `max_nesting_depth` 64 (cloud and local, capped below comrak's internal list depth of 100), `max_blocks` 1,000,000 / 10,000,000, `max_asset_bytes` 50 MB / unlimited, front matter 64 KiB / 64 KiB.
   - State that "unlimited" means the field is absent (no timeout, no `-M` flag), and that local runs stay cancellable with Ctrl-C, which kills the engine tree and deletes the workspace.
8. **Principle 3 exception (§1 + §18).** Native readers in `ariad-core` (Markdown now, HTML later) run in the calling process. They are pure Rust with `forbid(unsafe_code)`, typed errors, nesting and block limits, and are fuzzed from roadmap 1a. Every other parser runs out of process. Add a Decision Log row.
9. **§4 CLI contract.**
   - Command: `ashift convert <INPUT> --to <FORMAT> [-o <OUTPUT>] [--overwrite]`.
   - Exit codes:

     | Code | Meaning |
     |---|---|
     | 0 | ok |
     | 1 | conversion failed |
     | 2 | usage |
     | 3 | unsupported route |
     | 4 | limit exceeded |
     | 5 | required tool missing or wrong version |
     | 6 | destination exists |
     | 130 | interrupted |

   - Output: warnings print to stderr as `warning[<code>]: <message>`; stdout carries only the output path.
   - Env vars: `ASHIFT_PANDOC` (path to Pandoc) and `SOURCE_DATE_EPOCH` (fixed DOCX timestamps).
   - Future commands reuse the same exit-code table.
10. **§5 tree and §16 tooling.**
    - Add `scripts/` (installers), `.tools/` (gitignored), and `fixtures/gen/` (dev-only Python generator, uv workspace member).
    - The justfile recipe set is `fmt`, `lint`, `test`, `wasm`, `deny`, `js`, `py`, `pandoc`, `ci`. `dev` and `bench` arrive with `apps/` and `bench/`.
    - §16: the repository is public on the personal account `bavanchun`; CI runs the 3-OS matrix on every push and PR, with actions pinned by commit SHA.
    - Add a Decision Log row for the Python/uv fixture generator.
11. **§9.5 Tenancy, plans and metering** (new subsection):
    - Every account owns a workspace (tenant) from day one; a personal user gets a one-member workspace. Storage keys already use `t/{tenant}`.
    - Limits and quotas resolve from a `plan_entitlements` record keyed by plan (`free` at launch), never from constants in handlers. `ariad-core::Limits` is the shape those entitlements fill.
    - `usage_events` is an append-only, idempotent metering ledger (unique event key), recording pages, bytes and engine-seconds per tenant.
    - A `BillingProvider` adapter is introduced only when paid plans launch; the provider stays an open question.
    - Add a Decision Log row.
12. **Domain and identifiers.**
    - Site: `https://ariadshift.ariadnev.com`.
    - JSON Schema `$id` base: `https://ariadshift.ariadnev.com/schemas/`.
    - Desktop bundle id `com.ariadnev.ariadshift`; the deep-link scheme stays `ariadshift://`.
    - Mark §20 Q4 resolved: no GitHub organization for now.
13. **§14.**
    - `cargo test --doc` runs next to cargo-nextest.
    - Golden strategy:
      - per-fixture snapshot of the sorted zip entry list;
      - pretty-printed XML parts via insta;
      - hashes for template-derived parts and media;
      - Pandoc version asserted against one shared constant;
      - `SOURCE_DATE_EPOCH` fixed.
14. **§20.** Mark Q1 resolved: free tier first, billing later, see §9.5. Add the open questions for the billing provider and the loopback exception.
15. **AGENTS.md.**
    - Licensing: fixtures exclude CC-BY-SA, GPL test suites, and research-only or non-commercial datasets; per-file licenses live in `fixtures/manifest.toml`. Model weights follow the same license rules as code.
    - Architecture changes: the binary is `ashift`.

## Todo

- [ ] Binary rename across §3, §4, §5, §6.3, §7, §7.1, §17, §20
- [ ] §2 versions, Pandoc pin, new crates, generator stack
- [ ] OCR changes + §18 OCR row replaced (not appended)
- [ ] §6.1 responsibilities; §6.2 exact IR v0 contract
- [ ] §7 request fields, outcome rule, Pandoc contract, timestamp rewrite, loopback question
- [ ] §11.2 new limits + unlimited semantics + Ctrl-C
- [ ] Principle 3 exception row
- [ ] §4 CLI contract (exit codes, env vars, output streams)
- [ ] §5/§16 tree, recipes, public CI; generator row
- [ ] §9.5 tenancy/entitlements/metering + row
- [ ] Domain, `$id` base, bundle id
- [ ] §14 golden strategy; §20 updates
- [ ] AGENTS.md lines

## Verification

- Remaining binary references: `grep -nP '\bariad\b(?!-)' ARCHITECTURE.md | grep -v -e ariadshift -e ariadnev`. Every remaining hit must be a non-binary use, and each one is reviewed.
- `grep -n 'RapidOCR' ARCHITECTURE.md`: no line claims RapidOCR handles Vietnamese, and §18 has exactly one OCR row.
- Decision Log: these **8** rows are present and unique:
  1. OCR (replaced)
  2. binary name
  3. Markdown reader (comrak)
  4. tenancy and metering
  5. in-process native readers (Principle 3 exception)
  6. Python fixture generator
  7. IR version policy
  8. DOCX timestamp rewrite
- §20 has no open item that the user already answered.
- The IR, protocol, limits and CLI contracts in ARCHITECTURE.md match phases 4–6 word for word (field names, exit codes).

## Success criteria

A reader of ARCHITECTURE.md alone can implement phases 2–6 without consulting this plan for any name, contract, limit, OCR, tenancy or domain decision.

## Risk assessment

| Risk | Mitigation |
|---|---|
| Rename misses a spot | Negative-lookahead grep plus review of every remaining hit |
| Contract drift between ARCHITECTURE and the code | Phases 4–6 update ARCHITECTURE.md in the same commit when a contract must change |
| The tenancy text overcommits to a billing design | Name the adapter boundary only; leave the provider as an open question |

## Security considerations

The text records the threat-model facts the code relies on:
- `--sandbox` is not OS isolation.
- The Principle 3 exception for native readers comes with its compensating controls.
- `Raw` can never carry OOXML.
- Non-permissive model weights are banned.
- Local runs default to unlimited per the user's decision, so cancellation is mandatory and specified.

## Next steps

Phase 2 scaffolds the workspace using the names and contracts fixed here.
