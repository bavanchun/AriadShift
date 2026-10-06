---
phase: 6
title: "Writers and route execution"
status: completed
priority: P1
effort: "14h"
dependencies: [4, 5]
---

# Phase 6: Writers and route execution

## Goal

Add the writer edges for Markdown and HTML (native, in `ariad-core`) and for EPUB (Pandoc), then turn the host's single hard-wired MD→DOCX pipeline into a route executor that runs any reader edge followed by any writer edge. `ashift convert` then covers MD↔DOCX/HTML/EPUB and every DOCX/HTML/EPUB cross route through IR. Route *selection* stays a fixed table here; phase 7 replaces it with the planner.

## Context links

- [1a tooling facts](../reports/researcher-261006-1209-phase-1a-tooling-facts.md) §5 (EPUB under the sandbox: fixed UUID, 1970 dates, cover fails, a missing title produces invalid EPUB)
- `crates/ariad-host/src/{convert.rs,docx_meta.rs,workspace.rs}`, `crates/ariad-cli/src/main.rs`, `crates/ariad-cli/tests/docx_golden.rs`
- ARCHITECTURE §4 (CLI contract and exit codes), §6.4 (format matrix: Markdown and HTML write in `ariad-core`, EPUB via Pandoc)

## Scout first

Read `convert.rs` and `docx_golden.rs` fully, list every place that assumes Markdown input or DOCX output (`SUPPORTED_ROUTE`, `is_markdown_docx_route`, `docx_meta::stamp`), and check how `Workspace::promote` handles a single file. The route executor must keep every guarantee those functions give today: atomic promotion, no partial output, Ctrl-C cleanup and exit codes.

## Requirements

Core writers (I/O-free):
- **`writer::markdown::write(&Document) -> WriteOutput`**, producing GFM:
  - ATX headings, `-` bullets, `1.` ordered lists that honour `start`, task lists, and pipe tables;
  - non-representable tables (spans, block cells) become a simplified pipe table with a warning;
  - fenced code with the longest-backtick rule, footnotes `[^id]`, `~~strike~~`, `$…$` / `$$…$$` math, and `<sup>`/`<sub>`;
  - escaping of every Markdown-active character in text;
  - links pass through `ariad_core::links` (phase 3), and a refused scheme becomes plain text with a warning;
  - images: asset-backed images are embedded as `data:` URIs (user decision), so the output stays one file. URL images stay URLs only when the scheme passes the allow-list;
  - YAML front matter from `Metadata`, using only known keys, quoted safely.
  - Property: for every Markdown fixture, `read(write(read(md)))` equals `read(md)` modulo source positions. This is the round-trip test.
- **`writer::html::write(&Document) -> WriteOutput`**: a standalone HTML5 document.
  - `<meta charset="utf-8">`, `lang`, `<title>` and `meta` author/keywords.
  - All text and attributes are escaped. Links pass through `ariad_core::links`, so `javascript:`, `data:text/html` and similar are refused with a warning.
  - **Raw HTML is sanitized, never passed through.** `Raw(html)` goes through `ammonia` 4.2.1 (MIT OR Apache-2.0; it uses html5ever ^0.40, the same parser as the reader). The allow-list keeps harmless formatting (`kbd`, `details`/`summary`, `sub`/`sup`, `abbr`, `mark`, …), strips every `script`, `style`, `iframe`, event-handler attribute and `style` attribute, restricts URL attributes to the same scheme allow-list, and adds `rel="noopener noreferrer"`. If sanitizing removed anything, emit one warning per document. The Decision Log row and the §2.3 pin come from phase 2.
  - Assets embedded as `data:` URIs, so the output is a single file.
  - Footnotes as a `<section role="doc-endnotes">`.
  - A small fixed inline stylesheet for readable defaults; no scripts and no external resources.
  - Property: `html::read(html::write(ir))` preserves the structure for every Markdown fixture.

Pandoc writer:
- The Pandoc engine accepts output `epub`. It runs `-t epub3` with `--sandbox` and metadata injected into the AST:
  - `identifier = urn:uuid:<UUIDv5(namespace URL ariadshift.ariadnev.com, SHA-256 of the IR JSON)>`;
  - `title`, falling back to the input file stem (the host passes it in `options.title_fallback`);
  - `lang`, falling back to `und`.
- The host rewrites `dcterms:modified` and `dc:date` in the EPUB's OPF to the conversion time, honouring `SOURCE_DATE_EPOCH`, as `docx_meta` does for DOCX. Generalize `docx_meta` into a `package_meta` module with one function per format, not a new copy.
- No cover image in 1a (Pandoc's `--epub-cover-image` fails under `--sandbox`).

Route executor (host):
- `Route = [ReaderEdge, WriterEdge]` where:
  - `ReaderEdge ∈ {native md, native html, pandoc docx, pandoc epub}`;
  - `WriterEdge ∈ {native md, native html, pandoc docx, pandoc epub}`.

  Phase 7 lets the planner produce it; here a fixed table maps `(input format, output format)` to a route.
- **One request type.** `convert()` today takes 7 parameters (`convert.rs:76-84`). An 8th would trip clippy's `too_many_arguments` under `CARGO_BUILD_WARNINGS=deny`. Replace the parameters with a `ConvertRequest` struct (input, output, target format, profile, overwrite, engine program, title fallback) and keep the cancellation token and event callback as arguments.
  - Its callers are `crates/ariad-cli/src/main.rs:96` now, and later the phase 9 host commands and the phase 10 MCP tools. List them in the phase report.
- **Default output and safety:**
  - The default output is `<input stem>.<target extension>`. Today it is hard-coded to `.docx` at `main.rs:66-68`.
  - Same-format routes (`md → md`, `docx → docx`, and so on) are refused with exit 3. They were never requested and they risk overwriting the source.
  - An output that canonicalizes to the input path is refused with exit 2, even with `--overwrite`.
- **Steps and cancellation.** It runs:
  1. the reader, in-process or through the engine;
  2. asset resolution for md and html input;
  3. the writer, in-process or through the engine;
  4. the metadata stamp for docx and epub;
  5. atomic promotion through the existing single-file `Workspace::promote`.

  The cancellation token is checked before every step, so native-only routes such as `md↔html` are cancellable too. Promotion is the commit point: once it starts, the result is reported as success. `main.rs` (and phase 10's MCP layer) must report the task's actual result instead of printing "interrupted" for a conversion that finished (`main.rs:113-122` discards it today).
- `--to` accepts `md`, `markdown`, `html`, `docx` and `epub`. Exit 3 lists the supported routes, generated from the route table rather than the `SUPPORTED_ROUTE` constant.
- Existing tests that change on purpose:
  - `crates/ariad-cli/tests/convert_cli.rs:148-155` expects `note.html --to docx` to fail with `md/markdown -> docx`. Rewrite it to use an input that stays unsupported (a `.pdf` file) and to assert the generated route list.
  - Every other existing CLI test passes unchanged.

Goldens (requested scope: MD↔DOCX/HTML/EPUB, plus a small cross sample):
- `md→html` (normalized HTML snapshot), `md→epub` (sorted zip entry list plus the pretty-printed OPF, nav and chapter XHTML; template-derived parts hashed with CRLF normalized, as in DOCX), and `docx→md`, `html→md` and `epub→md` (text snapshots).
- A cross-route sample: `docx→html` and `html→docx` on 2 fixtures each. The other cross routes are executable but not snapshotted.
- Update the fixture manifest `routes` for every fixture covered.

## Files

- Create: `crates/ariad-core/src/writer/{mod.rs,markdown.rs,html.rs}`; modify `crates/ariad-core/src/lib.rs`, `crates/ariad-core/Cargo.toml` (`ammonia`)
- Modify: `crates/ariad-host/src/{convert.rs,engines/pandoc.rs,lib.rs}`, `crates/ariad-host/Cargo.toml` (`uuid` feature `v5`); rename `docx_meta.rs` → `package_meta.rs` (with `git mv`)
- Modify: `Cargo.lock`. This phase owns it in its wave; `ammonia` is pinned in the root `Cargo.toml` by phase 2
- Modify: `crates/ariad-cli/src/main.rs` (`ConvertRequest`, default output, real outcome on interrupt), `crates/ariad-cli/tests/convert_cli.rs` (the rewritten unsupported-route test)
- Create: `crates/ariad-cli/tests/route_golden.rs`, `crates/ariad-core/tests/writer_roundtrip.rs`, new `fixtures/golden/*.snap`; modify `fixtures/manifest.toml`
- Modify: `ARCHITECTURE.md` §4 (supported routes, default output, same-format refusal, Markdown images as `data:` URIs), `README.md` (usage)

## Implementation steps

1. Markdown writer + round-trip property test. Commit.
2. HTML writer + read-back test. Commit.
3. `package_meta` refactor (DOCX goldens must stay byte-identical), then EPUB support in the engine and the OPF stamp. Commit each.
4. `ConvertRequest`, then the route table and executor with cancellation checks before each step; rewrite the one unsupported-route test; every other existing CLI test passes unchanged. Commit.
5. Default output per target, same-format refusal, output-equals-input refusal, and the real outcome on interrupt, with tests. Commit.
6. Route goldens, reviewed by hand (open the EPUB in a reader on Linux, for example `ebook-viewer` or LibreOffice, and validate with `epubcheck` if it is available). Record the check in the commit body. Commit.
7. Docs, then `just ci` and three-OS CI.

## Test matrix

| Priority | Case |
|---|---|
| Critical | Every Markdown fixture round-trips through the Markdown writer |
| Critical | EPUB has `dc:title`, `dc:language`, a stable identifier, and stamped dates; byte-identical across two runs with `SOURCE_DATE_EPOCH` |
| Critical | Ctrl-C during an EPUB conversion and during a native `md→html` conversion leaves no output and no workspace |
| Critical | Hostile Markdown (`<script>`, `<img onerror>`, `[x](javascript:…)`) produces HTML with no script, no handler and no `javascript:` URL |
| Critical | `convert notes.md --to md` is refused (exit 3); `-o` equal to the input is refused even with `--overwrite` |
| High | Markdown output embeds images as `data:` URIs and round-trips |
| High | Existing DOCX goldens unchanged |

## Success criteria

- `ashift convert` works for every pair of different formats in {md, html, docx, epub} on three OSes; the MD↔X goldens and the cross sample are reviewed.
- Exit 3 lists the supported routes, which are generated, not hard-coded.

## Risk assessment

| Risk | Signal | Response |
|---|---|---|
| Pandoc EPUB templates differ per OS (CRLF) | Windows-only golden diff | Normalize as for DOCX; hash template-derived parts |
| Markdown writer escaping misses an edge | Round-trip test fails | Fix the writer; never relax the property |
| Large images make Markdown output heavy | `.md` files of many MB | Accepted trade-off (user decision); an `--extract-assets` option is a later follow-up |

## Security

- The HTML writer is the only place we emit markup. Text is escaped, links pass the shared allow-list, and raw HTML passes through ammonia's allow-list sanitizer. A wrapper element is never treated as a security boundary. The MCP inline text in phase 10 returns Markdown, not HTML.
