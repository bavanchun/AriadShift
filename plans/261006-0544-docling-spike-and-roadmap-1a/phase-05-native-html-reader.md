---
phase: 5
title: "Native HTML reader"
status: pending
priority: P1
effort: "12h"
dependencies: [3]
---

# Phase 5: Native HTML reader

## Goal

Add an in-process HTML reader to `ariad-core`, built on html5ever with our own tree sink. It is pure safe Rust in our crate, WASM-compilable, and bounded by byte, DOM-depth, IR-nesting and block limits. It is the second native reader covered by the documented exception to Principle 3.

## Context links

- [1a tooling facts](../reports/researcher-261006-1209-phase-1a-tooling-facts.md) §6: html5ever 0.40.1; tree building is quadratic in depth (20k nested `<div>` take 1 s); there is no built-in limit
- `crates/ariad-core/src/reader/markdown.rs`: the conventions to mirror (`ReadOutput`, `ReadError`, warnings with positions, NFC, link allow-list, iterative limits)
- `crates/ariad-host/src/assets.rs`: host-side resolution of relative image paths
- ARCHITECTURE §1 (exception to Principle 3), §6.2, §11.1 (SSRF / local file inclusion)

## Scout first

Read `reader/markdown.rs` end to end and reuse its helpers: NFC and the warning constructors; link checks come from `ariad_core::links` (phase 3). If they are private, move the shared ones into `reader/common.rs` as the first commit. Check html5ever 0.40.1's `TreeSink` trait signature in its docs before designing the sink.

## Requirements

- `reader::html::read(input: &[u8], limits: &Limits) -> Result<ReadOutput, ReadError>`.
- **Decoding:**
  - Strip a UTF-8 BOM.
  - Honour a `<meta charset>` or `http-equiv` declaration within the first 1024 bytes through `encoding_rs`, which is WHATWG-compliant, MIT OR Apache-2.0 and WASM-safe. This covers legacy Vietnamese pages (`windows-1258`).
  - Default to UTF-8. Replace undecodable bytes with U+FFFD and emit one warning.
- **Limits:**
  - Check `max_input_bytes` before parsing, when set.
  - **Bounding html5ever's cost.** The quadratic cost lives in html5ever's own open-element stack (scope checks on each start tag walk it), not in our sink. `TreeSink::append` cannot abort the parse. So:
    - feed the parser in chunks of at most 4 KiB through its incremental input API;
    - have the sink set a shared `limit_hit` flag when the open-element depth passes a DOM cap of 512;
    - stop feeding at the next chunk boundary and return `ReadError::NestingTooDeep`.

    The work done after the cap is bounded by one chunk. Test with 1,000,000 nested `<div>`s (a real, non-ignored test with a generous wall-clock bound measured on CI) and with 600 nested (passes).
  - IR mapping enforces `max_nesting_depth` (64) and `max_blocks`. Non-semantic containers (`div`, `section`, `article`, `main`, `span`, `header`, `footer`, `nav`) flatten and add no IR depth.
- **Mapping:**

| HTML | IR |
|---|---|
| `h1`–`h6` | `Heading` |
| `p` | `Paragraph` |
| `ul`, `ol` (`start`, `reversed` → warning), `li`, `<input type=checkbox>` in `li` | `List` / `ListItem.checked` |
| `table`, `caption`, `thead`/`tbody`/`tfoot`, `tr`, `th`/`td` (`rowspan`, `colspan`, `align` / `style text-align`) | `Table` (+ header flags if phase 3 added them) |
| `blockquote` | `Quote` |
| `pre` (+ `code class="language-x"`) | `Code { lang }` |
| `figure` / `img` / `figcaption`; bare `img` | `Figure` / inline `Image` |
| `a href` (shared `ariad_core::links` allow-list from phase 3) | `Link`, else text + warning |
| `em`/`i`, `strong`/`b`, `s`/`del`/`strike`, `sup`, `sub`, `code`/`kbd`/`samp`, `br` | inline equivalents |
| `hr` | dropped with a warning; the IR has no thematic-break node (listed for the 1b freeze) |
| `math`, `.math`, KaTeX/MathJax markup | `Math { tex }` only when a TeX annotation is present (`annotation encoding="application/x-tex"`), else text + warning |
| footnotes (`role="doc-noteref"` / `doc-footnote`, `aside epub:type=footnote`) | `Footnote` / `FootnoteRef` |
| `script`, `style`, `template`, `noscript`, `iframe`, `object`, `embed`, `svg`, form controls, comments | dropped (warning per kind, once) |
| `head`: `title`, `html lang`, `meta name=author/description/keywords` | `Metadata` |

- **Whitespace:** collapse it per HTML rendering rules outside `pre`, and trim at block edges.
- **Images:**
  - A `data:` URI image is decoded (bounded by `max_asset_bytes`), sniffed and stored in `AssetStore`.
  - A relative `src` becomes `AssetRef::Url` and the host resolves it with the existing `assets::resolve` confinement.
  - An `http(s)` image is never fetched: it stays a URL, and the writer warns as it does today.
- No network, no filesystem access and no `unsafe` in our code. `#![forbid(unsafe_code)]` stays.
- The host wires `.html` and `.htm` input into the reader edge `html → IR`. Writers arrive in phase 6.

## Files

- Create: `crates/ariad-core/src/reader/html.rs`, `crates/ariad-core/src/reader/html_sink.rs`; maybe `crates/ariad-core/src/reader/common.rs`
- Modify: `crates/ariad-core/src/reader/{mod.rs,markdown.rs (only to share helpers)}`, `crates/ariad-core/Cargo.toml`
- Modify: `Cargo.lock` (this phase owns it in this wave). `html5ever` and `encoding_rs` are pinned in the root `Cargo.toml` by phase 2; this phase only references them from `crates/ariad-core/Cargo.toml`
- Modify: `ARCHITECTURE.md` §6.2 (HTML reader paragraph mirroring the Markdown one); the §2.3 rows come from phase 2
- Create: `crates/ariad-core/tests/html_reader.rs`; extend `crates/ariad-core/tests/ir_snapshots.rs` to the 6 HTML fixtures; `fixtures/golden/<html-id>.ir.snap`
- Must not touch: `crates/ariad-core/src/pandoc/**`, `crates/ariad-host/src/engines/**` (phase 4)

## Implementation steps

1. Shared reader helpers, if needed (pure move, no behaviour change; the snapshots must stay identical). Commit.
2. Tree sink with an arena and the `limit_hit` flag, plus chunked feeding; tests with 600 nested `div`s (passes), 1,000,000 nested `div`s (typed error within the time bound) and misnested formatting. Commit.
3. Decoding with `encoding_rs` (kept by validation); tests with inline byte strings, not new fixtures (phase 4 owns the fixture manifest in this wave): BOM, `windows-1258` Vietnamese text, `meta charset` after 1024 bytes (ignored), and invalid UTF-8. Commit.
4. Mapping table with one test per row, including dropped elements and the link allow-list. Commit.
5. IR snapshots for the 6 HTML fixtures, reviewed by hand. Commit.
6. `cargo check -p ariad-core --target wasm32-unknown-unknown`, `just ci`, then ARCHITECTURE. Commit.

## Success criteria

- All 6 HTML fixtures produce reviewed IR snapshots with headings, tables, links and Vietnamese text intact.
- The wasm32 build passes, and `cargo deny check` passes with the new dependencies.
- The 1,000,000-depth test proves bounded time in CI on three OSes.

## Risk assessment

| Risk | Signal | Response |
|---|---|---|
| html5ever's `TreeSink` API differs from the research notes | Compile errors against 0.40.1 | Follow its docs and examples; the sink design (arena + caps) stays |
| Flattening `div`s merges paragraphs wrongly | Snapshot shows joined text | Treat block-level `div` boundaries as paragraph breaks when they hold inline content |
| `encoding_rs` adds noticeable WASM size | Build size jump | Acceptable for 1a; measure and note in the commit body |

## Security

- Hostile markup is bounded by byte, depth and block caps, and phase 11 fuzzes it. Script-bearing elements are dropped. Remote resources are never fetched. Local `src` paths go through the host's confined resolver, never through the core.
