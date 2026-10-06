# AriadShift — System Architecture

> **Status:** Proposal v1 · **Updated:** 2026-10-06
> Version selections are based on registry and release checks recorded on 2026-10-06; lockfiles pin exact resolved versions.

AriadShift is an open-source, local-first document transformation platform. A single core engine powers five surfaces: **CLI, Desktop, Web, REST API/SDK, and MCP server**.

The name is derived from Ariadne's thread: AriadShift's planner navigates the format "labyrinth," just as Ariadne's thread guided Theseus out of the maze.

**Three product differentiators:**

1. **Private by default.** Browser and desktop process first; cloud is an option, not the default.
2. **Measurable quality.** Every conversion route has a score generated from public benchmarks, never hand-coded.
3. **AI agent ready.** Any document → clean Markdown/JSON via MCP, immediately usable as LLM context.

---

## 1. Architectural Principles

| # | Principle | Concrete Implications |
|---|---|---|
| 1 | One core, multiple surfaces | CLI, Desktop, Web, API, and MCP invoke the same `ariad-core`; no surface contains dedicated conversion logic |
| 2 | IR-centric (hub-and-spoke) | N readers + M writers instead of N×M converters; adding a format requires only a single adapter |
| 3 | Out-of-process engines, uniform protocol | All untrusted file parsing runs in sandboxed child processes; an engine crash never crashes the host app |
| 4 | Local-first | Priority order: browser → desktop/CLI → cloud |
| 5 | Evidence over claims | Fidelity and speed scores for each route are generated from `bench/`, where the planner reads them |
| 6 | Secure by default | Resource limits, decompression bomb prevention, no network access for engines, no content logging |
| 7 | First-class self-hosting | `docker compose up` runs the complete stack with zero external cloud accounts required |
| 8 | License clean | Apache-2.0 project; no AGPL in distribution; CI blocks invalid licenses |
| 9 | No lock-in | Open standards at every boundary: S3 API, OpenAPI 3.1, JWT/JWKS, OpenTelemetry, MCP |

Native Markdown and HTML parsing in `ariad-core` run in-process as explicit exceptions to Principle 3. These readers are pure Rust with `#![forbid(unsafe_code)]`, typed errors, and finite nesting, node, and block limits; core readers are fuzzed starting in roadmap 1a. Every other parser runs out of process.

---

## 2. Versioning Policy

- Components **with an LTS channel** → use the **latest LTS**.
- Components **without LTS** → use the **latest stable release**.
- **Do not use** alpha, beta, or RC builds, even when available (e.g., Tauri 3 alpha, PostgreSQL 19 beta).
- Pin exact versions via lockfiles (`pnpm-lock.yaml`, `Cargo.lock`, `uv.lock`). Renovate opens weekly update PRs; patch releases automerge when CI passes.

### 2.1 Runtimes and Platforms

| Component | Pinned Version | Channel | Notes |
|---|---|---|---|
| Node.js | **24.x "Krypton"** (24.21) | LTS | Node 26 enters LTS on **2026-10-28** → upgrade immediately after that date. Node 24 is maintained until 2028-04-30 |
| Rust | **1.99** stable, edition 2024 | Stable (Rust has no LTS) | Pinned via `rust-toolchain.toml` |
| Python | **3.14.x** | Stable, supported until 2030-10 | Python has no LTS; 3.15 is unreleased |
| PostgreSQL | **18.x** (18.6) | Major supported until 2030-11 | 19 is in beta |
| Ubuntu (Docker image, CI) | **26.04 LTS** "Resolute Raccoon" | LTS | Single base image for all containers |
| GitHub Actions runners | `ubuntu-26.04`, `macos-26`, `windows-2025` | GA | Pin labels explicitly; `ubuntu-latest` remains 24.04 until November 2026 |
| LibreOffice | **26.8.x** | Longest-supported line currently available (until 2027-06) | LibreOffice has no LTS; 26.2 expires 2026-11-30 |

### 2.2 Web and Desktop

| Component | Version | Notes |
|---|---|---|
| Next.js | **16.3.x** | Current LTS line; Next 15 reaches end of support on 2026-10-21 |
| React | 19.3 | |
| TypeScript | **7.0** (native Go compiler) | 8–12x faster. Next 16.3 uses it via `experimental.useTypeScriptCli`; JS API available from 7.1 |
| Vite | 8.3 (Rolldown bundler) | Used for desktop |
| Tailwind CSS | 4.3 | |
| shadcn/ui | CLI 4.x, **Base UI** style (`@base-ui/react` 1.8) | Base UI has been the shadcn default since 07/2026 |
| Tauri | **2.12** | Tauri 3 is in alpha → not yet adopted |
| TanStack Query / Zustand / Zod | 5 / 5 / 4 | Server state / UI state / schema |
| React Hook Form / Motion / lucide-react | 7 / 14 / 1.x | |
| Uppy | 6 (+ `@uppy/aws-s3`) | Resumable multipart uploads |
| Better Auth | 1.7 | Includes JWT, API Key, and Passkey plugins |
| pnpm | 12 | Workspace + catalog |
| Biome | 2.5 | Single-binary linter + formatter; independent of TypeScript JS APIs |
| Vitest / Playwright | 5 / 1.63 | |

### 2.3 Rust Crates

| Crate | Version | Role |
|---|---|---|
| tokio | **~1.53** | **LTS line until 2027-09** (pinned to minor with `~`) |
| axum / tower / tower-http | 0.8 / 0.5 / 0.7 | HTTP API |
| sqlx | 0.9 | Postgres, compile-time checked SQL |
| object_store | 0.14 | Unified API for S3, R2, GCS, Azure, and local disk |
| utoipa | 6 | OpenAPI 3.1 generation from code |
| tracing / opentelemetry | 0.1 / 0.33 | Observability |
| serde / thiserror / clap / schemars / jiff / uuid | 1 / 2 / 4.6 / 1.2 / 0.2 / 1 | Common foundational utilities |
| pdfium-render + PDFium | 0.9 + `chromium/8076` (bblanchon/pdfium-binaries, **non-V8** build) | PDF rendering, text extraction, images, metadata |
| typst | 0.15 | IR → PDF |
| rmcp | **3.5.1** (`default-features = false`, `server`, `macros`, `transport-io`) | Official MCP SDK for Rust |
| wasm-bindgen / wasm-pack | 0.2 / 0.15 | Build `ariad-wasm` |
| jsonwebtoken | 11 | JWT verification from Better Auth |
| tauri-specta | 1.0 | TS type generation for Tauri commands |
| comrak | **0.55.0** (`default-features = false`, `shortcodes`) | CommonMark/GFM Markdown reader in `ariad-core`; BSD-2-Clause |
| html5ever | **0.40.1** (MIT OR Apache-2.0) | Native HTML reader in `ariad-core` |
| process-wrap | **10.0.1** (`tokio1`, `process-group`, `job-object`) | Cross-platform engine process-tree control |
| tokio-util | **0.7.19** | Bounded NDJSON line framing |
| unicode-normalization | **0.1.25** | NFC normalization of prose text |
| sha2 | **0.11.0** | SHA-256 asset identifiers and golden hashes |
| base64 | **0.23.1** | JSON asset bytes and embedded image data URLs |
| cap-std | **4.0.3** | Capability-based asset file access |
| zip | **8.6.0** | Runtime DOCX metadata rewrite; also used by goldens |
| serde_stacker | **0.1.14** | Deep IR JSON reads in the native host |
| serde_json | **1.0.151** (`unbounded_depth` in the host) | IR and protocol JSON |
| tempfile | **3.27.0** | Isolated conversion workspaces |
| saphyr-parser | **0.1.0** (MIT OR Apache-2.0) | Pure-Rust YAML 1.2 event parser; its `Alias` events let the reader reject aliases before interpreting front matter; wasm compatibility is checked in phase 4 |
| Dev-only test crates | insta **1.49.0** (`json`, `glob`), jsonschema **0.58.5**, assert_cmd **2.2.2**, quick-xml **0.42.0**, toml **1.1.6+spec-1.1.0**, hex **0.4.3** | Test and fixture validation only; never distributed |

### 2.4 Engines and Infrastructure

| Component | Version | License | Role |
|---|---|---|---|
| Docling | 2.134.0 | MIT (weights: Apache-2.0 / CDLA-Permissive-2.0 / MIT) | Structural document parsing for PDF, images, DOCX/PPTX/XLSX (Heron layout, TableFormer); OCR uses Docling's Tesseract CLI option (`TesseractCliOcrOptions`), not the `tesserocr` binding (no Windows wheel, bundles Tesseract 5.5.1, pulls in LGPL cysignals); Linux pack uses CPU-only PyTorch index |
| Granite-Docling | 258M | Apache-2.0 | English complex layouts; ja/ar/zh are experimental; not used for Vietnamese |
| RapidOCR + ONNX Runtime | 3.9.2 + 1.30.0 | Apache-2.0 / MIT | PP-OCRv6 OCR for English, Chinese, Japanese, and most Latin languages; never route Vietnamese here |
| Tesseract + `tessdata_best` | 5.5.3 + `vie`/`eng`/`osd` | Apache-2.0 | Default OCR for Vietnamese and mixed Vietnamese-English |
| PaddleOCR-VL | 1.6 (0.96B; via llama.cpp) | Apache-2.0 / MIT | Optional high-accuracy OCR path |
| Pandoc | **3.12** (official `pandoc.wasm` available) | GPL-2.0+ | Install the official release binary with SHA-256 verification via `scripts/install-pandoc.sh`; the Arch package is 3.11. Hub for DOCX, ODT, EPUB, HTML, LaTeX, and RST |
| FFmpeg | 9.0, LGPL build | LGPL-2.1+ | Audio/video (later phase) |
| libvips | 8.x | LGPL-2.1+ | Large images, batch processing |
| pgmq | 1.13 | PostgreSQL License | In-Postgres job queue |
| NATS | 2.15 | Apache-2.0 | Queue adapter for high-scale needs |
| SeaweedFS | 4.x | Apache-2.0 | Self-hosted S3 (MinIO archived 04/2026) |
| Cloudflare R2 | — | Managed service | Object storage for hosted tier |
| uv / ruff | 0.12 / 0.16 | MIT | Python environment management and linting |
| Fixture generator | Exact versions in `uv.lock` | Permissive | python-docx, Pillow, and typst-py are development-only and never distributed |

---

## 3. System Overview

```mermaid
flowchart TB
  subgraph surfaces["Product Surfaces"]
    CLI["CLI: ashift"]
    MCP["MCP server: ashift mcp"]
    DESK["Desktop: Tauri 2 + React 19"]
    WEB["Web: Next.js 16"]
    API["REST API /v1"]
  end

  subgraph core["ariad-core (Rust, WASM-safe)"]
    IR["Document IR"]
    PLAN["Planner + capability graph"]
    LIM["Limits & validation"]
  end

  HOST["ariad-host: engine runner, sandbox, engine packs"]
  WASM["ariad-wasm + pandoc.wasm + PDFium WASM + Typst"]
  SERVER["ariad-server: api / worker"]
  ENG["Engines: PDFium · Pandoc · Docling + Tesseract / RapidOCR · LibreOffice · Typst"]
  PG[("PostgreSQL 18 + pgmq")]
  S3[("S3 API: SeaweedFS / R2")]

  CLI --> HOST
  MCP --> HOST
  DESK --> HOST
  WEB -- "local processing" --> WASM
  WEB -- "cloud processing" --> API
  API --> SERVER
  SERVER --> PG
  SERVER --> S3
  SERVER --> HOST
  HOST --> core
  WASM --> core
  HOST --> ENG
```

---

## 4. Product Surfaces

| Surface | Target Users | Runtime Location | Available Engines |
|---|---|---|---|
| **CLI** `ashift` | Developers, scripts, CI | Local machine | All installed engines |
| **MCP** `ashift mcp` (stdio) | AI agents: Claude, Cursor, IDEs | Local machine | Same as CLI |
| **Desktop** | Privacy-focused users, large files, batch | Local machine | All, installed via engine packs |
| **Web, Local Mode** | General users | Browser (WASM) | core, Pandoc WASM, PDFium WASM, Typst |
| **Web, Cloud Mode** | Complex files, scans, legacy Office | Server workers | All |
| **REST API + TS SDK** | Developers, enterprises | Server | All |

Representative CLI commands:

```bash
ashift convert notes.md --to docx
ashift inspect scan.pdf          # PDF type, page count, tables, OCR requirement
ashift plan paper.pdf --to docx  # explain the selected route and rationale
ashift engines                  # installed engines, versions, licenses
ashift engines install docling  # download engine pack
ashift doctor                   # check environment
ashift mcp                      # run MCP server over stdio
```

The conversion command is `ashift convert <INPUT> --to <FORMAT> [-o <OUTPUT>] [--overwrite]`. It supports converting between `md`/`markdown`, `html`, `docx`, and `epub` across all 12 distinct format pairs. The default output is `<input stem>.<target extension>` beside the input. Same-format routes (such as `md -> md`) are refused with exit code 3; destinations that canonicalize to the input path are refused with exit code 2 even when `--overwrite` is specified. In Markdown and HTML writers, asset-backed images are embedded as `data:` URIs so output remains a single file. `ashift mcp` confines file access to client roots plus `--allow-dir` paths; overwriting needs the server flag `--allow-overwrite`.

| Exit code | Meaning |
|---|---|
| 0 | ok |
| 1 | conversion failed |
| 2 | usage |
| 3 | unsupported route (prints the supported routes) |
| 4 | limit exceeded |
| 5 | tool missing or wrong version (prints a hint) |
| 6 | destination exists |
| 130 | interrupted |

Warnings are written to stderr as `warning[<code>]: <message>`. Progress is written to stderr only when stderr is a TTY. Stdout contains only the output path. Ctrl-C (and Ctrl-Break on Windows) cancels the run, kills the engine process tree, deletes the workspace, returns 130, and leaves the destination untouched. Failures never leave partial output; `--overwrite` replaces the destination only after a successful conversion. `ASHIFT_PANDOC` selects the Pandoc executable; `SOURCE_DATE_EPOCH` fixes DOCX and EPUB timestamps for reproducible goldens. Future commands reuse the same exit-code table.

---

## 5. Monorepo Structure

```text
ariadshift/
├── ARCHITECTURE.md
├── Cargo.toml                 # Cargo workspace
├── rust-toolchain.toml        # Rust 1.99
├── pnpm-workspace.yaml        # pnpm workspace + shared version catalog
├── pyproject.toml             # uv workspace
├── justfile                   # root entry point: fmt, lint, test, wasm, deny, js, py, pandoc, ci
├── .node-version              # 24
├── scripts/                   # pinned tool installers
├── .tools/                    # gitignored local tools
├── crates/
│   ├── ariad-core/            # IR, format registry, planner, limits (I/O-free, WASM-compilable)
│   ├── ariad-host/            # engine runner, sandbox, engine packs, native adapters (feature flags)
│   ├── ariad-cli/             # `ashift` binary (includes `ashift mcp`)
│   ├── ariad-wasm/            # wasm-bindgen bindings for ariad-core
│   └── ariad-server/          # `ariad-server api | worker` binary
├── engines/
│   └── docling/               # Python engine (uv project) implementing the engine protocol
├── apps/
│   ├── web/                   # Next.js 16
│   └── desktop/               # Vite 8 + React 19
│       └── src-tauri/         # Tauri 2, depends on ariad-host
├── packages/
│   ├── ui/                    # shared React components (shadcn + Base UI)
│   ├── sdk/                   # TypeScript SDK generated from OpenAPI
│   └── config/                # tsconfig, Biome presets
├── schemas/                   # JSON Schema: IR, engine protocol, capabilities (generated with schemars)
├── fixtures/
│   ├── manifest.toml          # source, license and digest for each committed document
│   ├── gen/                   # dev-only Python fixture generator; uv workspace member
│   └── golden/                # generated IR and DOCX snapshots
├── bench/                     # benchmark harness → capabilities.json
├── infra/
│   ├── compose/               # docker-compose.yml for self-hosting
│   └── docker/                # Dockerfile for server and workers
├── brand/                     # code-generated logo and icons
└── docs/
```

The `justfile` initially provides `fmt`, `lint`, `test`, `wasm`, `deny`, `js`, `py`, `pandoc`, and `ci`. `dev` and `bench` arrive with `apps/` and `bench/`.

Only 5 crates exist initially. Further crate splits occur only when distinct boundaries emerge, such as an adapter requiring an independent release cycle.

---

## 6. Core

### 6.1 Crate Responsibilities

| Crate | Responsibility | WASM | Key Dependencies |
|---|---|---|---|
| `ariad-core` | IR types, format registry, planner, limit enforcement, pure-data engine-protocol message types shared with host, browser, and server, native Markdown and HTML readers/writers | ✓ | serde, schemars |
| `ariad-host` | Asset resolution, engine process execution, workspaces, engine pack management, PDFium/Pandoc/LibreOffice/Docling/Typst adapters, and DOCX metadata rewrite | ✗ | tokio, pdfium-render, typst |
| `ariad-cli` | Command-line interface, MCP server | ✗ | clap, rmcp |
| `ariad-wasm` | Browser JS API: plan, convert lightweight routes | ✓ | wasm-bindgen |
| `ariad-server` | `api`: Axum + OpenAPI; `worker`: consumes jobs from pgmq and calls ariad-host | ✗ | axum, sqlx, object_store |

### 6.2 Document IR

The IR serves as the "lingua franca" across readers and writers. Designed in three layers, the layout layer is entirely optional:

The versioned schema is `ariad-ir/0` at `schemas/ir.v0.json` during 0.x; breaking changes are allowed. It freezes as `ariad-ir/1` together with `ariad-engine/1` in roadmap 1b.

Every enum variant is a struct variant serialized with an internally tagged `type` field and a snake_case tag, for example `{"type":"paragraph","content":[...]}`. This is required for Serde's internally tagged representation and avoids tuple or newtype variants. Format variants serialize with a single wire identity matching `Format::id()` (for example `ariad-ir+json` and `pandoc+json`). In addition to `Markdown`, `Html`, and `Docx`, the format registry provides `Epub`, `Pdf`, and `Png` (required by the spike for image inputs and source-format deserialization without a reader in 1a).

New IR fields follow the rule: **omit when empty** (`#[serde(default, skip_serializing_if = ...)]`). Existing fields keep their current `null` serialization. Every snapshot remains byte-identical.

`Document` has `{ version, meta, body, assets, layout, provenance, furniture }`. `furniture: Vec<Block>` carries running page headers and footers outside `body` and is omitted when empty. `Metadata` has `{ title, authors, language, date, subject, keywords, source_format }`.

`LayoutIndex` specifies page geometry plus per-block bounding boxes: `{ pages, blocks }`. Coordinates (`left`, `top`, `right`, `bottom`) and dimensions (`width`, `height`) use typographical points (pt, 1/72 inch) with origin `(0, 0)` at the top-left of each page. Blocks are keyed by stable `/`-separated block paths where path segments match real AST field names and compose recursively for arbitrary nesting:
- Top-level body blocks: `"body/<index>"` (e.g. `"body/0"`).
- Furniture blocks: `"furniture/<index>"` (e.g. `"furniture/0"`).
- Nested quote blocks: `"body/<index>/blocks/<block_index>"`.
- Nested footnote blocks: `"body/<index>/blocks/<block_index>"`.
- Nested list item blocks: `"body/<index>/items/<item_index>/blocks/<block_index>"`.
- Nested table cell blocks: `"body/<index>/head/<row_index>/cells/<cell_index>/blocks/<block_index>"` or `"body/<index>/body/<row_index>/cells/<cell_index>/blocks/<block_index>"`.

Paths compose recursively across container boundaries (for example, a list item inside a quote is `"body/<index>/blocks/<block_index>/items/<item_index>/blocks/<inner_block_index>"`). Each block position entry contains `{ page_no, bbox: { left, top, right, bottom } }`. `LayoutIndex` is omitted when empty. Old free-form layout objects (such as arbitrary JSON maps) are ignored under the `ariad-ir/0` draft; layout content must conform to the `LayoutIndex` schema.

`Block` variants and fields are:

- `Heading { level, content }`, `Paragraph { content }`, `Code { lang, text }`, `Math { tex, display }`, `Quote { blocks }`, and `PageBreak {}`.
- `List { ordered, start, tight, items }`, where each `ListItem` is `{ checked, blocks }`.
- `Table { caption, columns, head, body, footnotes }`, with `ColumnSpec { align }`, cells `{ rowspan, colspan, header, blocks }`, and `footnotes: Vec<Inline>` (omitted when empty). `TableCell.header: Option<bool>` models row headers / stub columns and is omitted when None.
- `Figure { asset, caption }`, `Footnote { id, blocks }`, and `Raw { format, text }`.

`Inline` variants are `Text { text }`, `Emph { content }`, `Strong { content }`, `Strikeout { content }`, `Superscript { content }`, `Subscript { content }`, `Code { text }`, `Link { url, title, content }`, `Image { target, alt, title }`, `SoftBreak {}`, `LineBreak {}`, `Math { tex, display }`, `FootnoteRef { id }`, and `Raw { format, text }`.

`AssetRef` is `Asset { id }` (lowercase hexadecimal SHA-256) or `Url { href }`. `AssetStore` is a `BTreeMap` from ids to `{ media_type, bytes }`; JSON encodes `bytes` as base64. `RawFormat` is closed to `html` and `tex`, so IR content cannot carry OOXML.

Prose text, alt text, link text, and metadata strings are normalized to NFC. `Code`, `Math`, `Raw`, and URLs are never rewritten. YAML front matter between `---` lines fills `Metadata` using `saphyr-parser` 0.1.0, a maintained, pure-Rust, permissively licensed YAML 1.2 event parser. Its explicit alias events let the reader reject aliases before interpreting front matter. Known keys are `title`, `author`/`authors`, `lang`, `date`, `subject`, and `keywords`; values must be scalars or lists of scalars, and `author` accepts either a string or a list. Unknown keys warn, invalid YAML warns and is ignored, and anchors and aliases are rejected. The Pandoc mapper maps `title` to `title`, `authors` to `author`, `language` to `lang`, and `date`, `subject`, and `keywords` to their matching Pandoc metadata keys. `ariad-core` remains I/O-free; the host resolves source assets before invoking writers.

The comrak reader accepts CommonMark and GFM tables, strikethrough, autolinks, task lists, footnotes, dollar math, raw HTML, emoji shortcodes, and YAML front matter. It strips a UTF-8 BOM, normalizes CRLF and CR to LF, and iteratively enforces configured input, nesting, and block limits. Unsupported nodes map to the closest IR block with a warning.

The native HTML reader uses `html5ever` with a custom `TreeSink` arena and chunked 4 KiB feeding to bound DOM tree building. It strips a UTF-8 BOM, honours `<meta charset>` or `http-equiv` prescan within the first 1024 bytes through `encoding_rs` (supporting legacy Vietnamese `windows-1258`), and defaults to UTF-8 with `U+FFFD` replacement warnings. The sink enforces the DOM depth cap (1024) and total node bounds during parsing, rejecting oversized DOM structures with typed `NestingTooDeep` and `TooManyNodes` errors. The DOM depth cap of 1024 counts all elements including implicit `html` and `body`, so exactly 1022 explicitly nested `div`s pass and 1023 fail with `ReadError::NestingTooDeep { limit: 1024 }`. The user-facing node bound is `min(8192 + 1 * input_len, 8192 + 16 * max_blocks)`, allowing large table-heavy pages (such as 6 MB with 120k rows) to parse under local limits while bounding amplification. Hostile formatting-element reconstruction input exhibits memory amplification of about 230 bytes of RSS per input byte (each reconstructed element clones its attributes), while the in-process HTML reader is bounded by its node budget and the container memory limit. Iterative heap-based conversion enforces IR nesting depth (`max_nesting_depth` counting both block and inline nesting) and block limits without recursion. Headings, paragraphs, lists, tables with alignment and cell spans, quotes, preformatted code, figures, links validated against `ariad_core::links`, math annotations, and doc-footnote/doc-noteref roles map to IR equivalents; phrasing elements flatten into inline content without splitting surrounding paragraphs, `div`, `section`, `form`, `dialog`, and similar containers are flattened, while scripts, styles, form controls, and other unsupported elements are dropped with once-per-kind warnings.

The shared link policy in `ariad_core::links` enforces an allow-list for link schemes (`http`, `https`, `mailto`, and in-document `#` anchors); other links become plain text with a warning. Asset-backed images are embedded as data URLs. An image that remains a URL becomes its alt text with a warning. The mapper may emit its own `openxml` page-break constant, but `RawFormat` never admits `openxml` from IR content.

Layout is optional. The `editable` profile discards it, while the `faithful` profile can use it to preserve positioning. Pandoc AST JSON is the bridge to the writer; Docling mappings arrive with the Docling engine in roadmap 1b.

### 6.3 Conversion Graph and Planner

- **Nodes** represent formats (including `ariad-ir`). **Edges** represent engine capabilities.
- Each edge carries **empirical metrics** from `bench/`: fidelity, editability, p50 time per page, peak memory, runtime availability (wasm / local / cloud), and license.
- **Profiles** determine routing weights:

| Profile | Priority |
|---|---|
| `editable` (default) | Semantic accuracy, editability: real headings, lists, tables |
| `faithful` | Visual page layout preservation |
| `fast` | Execution speed |
| `private` | Only local/wasm-capable edges; cloud routes are pruned from the graph |

The planner runs Dijkstra's shortest-path algorithm over weighted costs. Decisions are always **explainable**:

```text
$ ashift plan paper.pdf --to docx
input   paper.pdf · pdf (digital) · 37 pages · 7 tables · 4 formulas
route   pdf ─docling→ ariad-ir ─pandoc→ docx
score   fidelity 0.86 · editability 0.95 · estimated 9s · runs locally ✓
alt     pdf ─pdfium(text)→ ariad-ir ─pandoc→ docx · 6x faster but loses table structure
```

### 6.4 Format Matrix (v0.x Scope)

| Format | Read | Write | In Browser |
|---|---|---|---|
| PDF (digital) | Docling (structure), PDFium (text, images, metadata) | Typst (from IR) | PDFium WASM: inspect, text, images, page render |
| PDF (scan) | Docling + Tesseract for `vi`; RapidOCR otherwise; optional PaddleOCR-VL | — | ✗ (requires cloud or desktop) |
| DOCX | Docling, Pandoc | Pandoc | Pandoc WASM |
| PPTX, XLSX | Docling | LibreOffice (→ PDF) | ✗ |
| DOC, XLS, PPT, RTF, ODT, ODS, ODP | LibreOffice → OOXML/ODF → reader | LibreOffice | ✗ |
| Markdown (GFM), HTML | ariad-core | ariad-core | ✓ |
| EPUB, LaTeX, RST | Pandoc | Pandoc | Pandoc WASM |
| PNG, JPEG, WebP, TIFF | image crate / libvips (+ OCR when text needed) | image crate / libvips | Basic operations |
| JSON (IR, DoclingDocument) | ariad-core | ariad-core | ✓ |
| Audio, video | — | — | Later phase (FFmpeg + Docling ASR) |

Markdown and HTML are written natively in `ariad-core` (`writer::markdown::write` producing GFM with embedded `data:` URIs and dropping raw HTML tags to preserve inner text safely, and `writer::html::write` dropping inline raw HTML to escaped text while sanitizing block raw HTML through a single ammonia policy). The Markdown writer emits only what is provably safe: math outside a small safe character class becomes a fenced `math` block or a code span with a warning, and a task-list checkbox is dropped with a warning when the item starts with a code, math or raw block. Known limit: inline raw HTML formatting is reduced to its text in both writers. Writing DOCX and EPUB proceeds out of process via **IR → Pandoc AST → Pandoc** (`-t docx` and `-t epub3` under `--sandbox`), followed by host package metadata stamping (`package_meta`) honoring `SOURCE_DATE_EPOCH`. A single Pandoc adapter handles both formats. Writing a native DOCX writer in Rust is only justified if benchmarks prove Pandoc to be a bottleneck.

---

## 7. Engine Protocol

Every heavy engine and untrusted parser except the native readers explicitly exempted in §1 runs as an **isolated process** speaking a common protocol: **JSON Lines over stdin/stdout** (`ariad-engine/1`). CLI, Desktop, and Workers invoke engines identically; cloud workers simply layer on job queuing and stricter sandboxing.

Requests are op-tagged single JSON lines sent to stdin:

1. **`op = "convert"`**: runs document conversion.
```json
{"protocol":"ariad-engine/1","job":"job_01JABC","op":"convert",
 "input":{"path":"/work/in/document.ir.json","format":"ariad-ir+json"},
 "output":{"dir":"/work/out","format":"docx"},
 "work_dir":"/work/tmp","options":{},
 "limits":{"max_pages":null,"timeout_s":null,"max_memory_mb":null,"max_nesting_depth":64}}
```

2. **`op = "describe"`**: queries engine capability, version, and tool health with no workspace:
```json
{"protocol":"ariad-engine/1","job":"job_01JXYZ","op":"describe"}
```

Events: multiple JSON lines read from stdout.

```json
{"type":"capabilities","engine":"pandoc","version":"0.1.0","tool":{"name":"pandoc","version":"3.12","status":"found"},"license":"GPL-2.0-or-later","routes":[{"input":"ariad-ir+json","output":"docx"}],"enforces_memory_limit":true}
{"type":"progress","stage":"convert","done":12,"total":37}
{"type":"warning","code":"font_missing","message":"Font 'Cambria Math' substituted with 'STIX Two Math'"}
{"type":"artifact","path":"/work/out/document.docx","format":"docx"}
{"type":"result","ok":true,"metrics":{"pages":37,"elapsed_ms":8421}}
```

`Request` is an op-tagged enum:
- `Request::Convert { protocol, job, input { path, format }, output { dir, format }, work_dir, options, limits }`.
- `Request::Describe { protocol, job }`.

The `Event` enum is tagged by `type`:
- `capabilities { engine, version, tool { name, version, status }, license, routes, enforces_memory_limit, models? }`: reports engine metadata and health without running conversions. `tool.status` is strictly `found`, `missing`, or `wrong_version`; it never exposes file paths to protect privacy. `routes` lists supported `(input, output)` format pairs using registry format IDs.
- `progress { stage, done?, total? }`: progress unit is pages (`done: u64`, `total: u64`), and canonical stage name is `"convert"`.
- `warning { code, message }`: non-fatal conversion notices.
- `artifact { path, format }`: produced outputs; paths stay inside `output.dir`.
- `result { ok, metrics?, error? }`: occurs exactly once and last.

Error codes are closed to `invalid_request`, `unsupported_route`, `limit_exceeded`, `engine_failure`, `tool_missing`, `tool_version`, and `io`. Corrupt or encrypted archive inputs rejected by preflight map to `engine_failure` with a descriptive message rather than `invalid_request`.

Protocol rules:

- Engines **read and write only within the designated workspace** and **open no network connections**.
- **Stdout discipline:** stdout carries NDJSON protocol lines only; all diagnostic logging belongs on stderr. The host runner treats any non-JSON line on stdout as a protocol violation. Adapters redirect native library stdout (fd 1) to stderr (fd 2) and emit protocol events only over a duplicated protocol descriptor.
- **Engine configuration contract:** engine settings travel in request `options` (e.g. `artifacts_path`, `tessdata_path`). The runner clears the environment (`env_clear()`) and inherits only allow-listed variables: `PATH`, `TMPDIR`/`TMP`/`TEMP` (pointing to `work_dir` on convert), `ASHIFT_PANDOC`, and on Windows `SYSTEMROOT`. Windows profile variables (such as `USERPROFILE` and `APPDATA`) were not measured in the spike and are decided in 1b. Adapters set engine-specific flags (`HF_HUB_OFFLINE`, `TRANSFORMERS_OFFLINE`, `TESSDATA_PREFIX`) from request `options`.
- **Memory rule (no host limiter in 1a):** `max_memory_mb` is enforced by the engine (e.g. Pandoc `+RTS -M` or Python adapter `resource.setrlimit(RLIMIT_DATA)`). An engine that cannot enforce it reports `enforces_memory_limit: false` in `describe`. When `limits.max_memory_mb` is specified, the host checks engine capabilities before starting conversion, querying `describe` with a fixed 30 s timeout (`DESCRIBE_TIMEOUT = 30s`) and caching the capabilities lookup per engine program and arguments within the process. If `enforces_memory_limit` is false, the host refuses the convert request with a typed `limit_exceeded` error before spawning the conversion process rather than silently ignoring it. Host-side generic limiting is deferred to 1b because Linux `RLIMIT_AS` causes aborts in GHC/PyTorch runtimes and safe in-process `pre_exec` rlimits conflict with `#![forbid(unsafe_code)]`.
- `describe` runs with a fixed 30 s timeout (`DESCRIBE_TIMEOUT = 30s`), preventing hung engines from blocking host discovery commands or MCP tools.
- `result` with `ok: true` and process exit 0 is success. `result` with `ok: false` is a typed engine failure regardless of exit code. No `result`, or `ok: true` with a non-zero exit, is a crash. A line after `result` is a protocol violation.
- Receiving `capabilities` during a convert request is a protocol violation.
- The schema is located at `schemas/engine-protocol.v1.json` with `"x-status": "draft"`. All engines must pass a shared conformance test suite.
- `ariad-engine/1` is a draft until the Docling engine passes conformance (roadmap phase 1b); breaking changes are allowed only before that point.
- PDFium also runs out-of-process: `ariad-host` re-executes its own binary via `ashift __engine pdfium`.

Decided extensions (implemented in 1b):
- **Stateful session mode:** wire shape consists of an optional `session_id: Option<String>` field on `Request::Convert` and an advertised boolean `supports_session: bool` in `capabilities`. When `supports_session: true` is reported, a session is opened on the first `convert` request carrying that `session_id`. Subsequent `convert` requests carrying the same `session_id` reuse the warm running engine process without re-initialization (1.56x measured throughput speedup). The session closes when the host closes stdin, on idle timeout, or on host shutdown.

The Pandoc engine is reached through `ashift __engine pandoc`. For `describe`, it reports tool status, version, and supported reader/writer routes (`docx` and `epub` to/from `ariad-ir+json`). For `convert`, the job workspace contains `in/`, `out/`, `tmp/`, and `log/`; the request's `work_dir` is `tmp/`. For writer conversions (`ariad-ir+json` to `docx` or `epub`), the engine reads the IR, maps it to Pandoc AST JSON, and runs:

```text
pandoc [+RTS -M{max_memory_mb}M -RTS] --sandbox --log=<workspace>/log/pandoc-log.json -f json -t <docx|epub3> -o <output.dir>/document.<docx|epub>
```

The supported Pandoc range is `>= 3.12, < 4`; DOCX goldens assert the shared `PANDOC_GOLDEN_VERSION` constant, set to `3.12`. The `+RTS -M…` arguments are omitted when `max_memory_mb` is unlimited. The AST is written to stdin, stdout is discarded, and stderr is drained with a 64 KiB bound. `TMPDIR`, `TMP`, and `TEMP` point to `work_dir` (`tmp/`). Assets are embedded as `data:` URIs; Pandoc's structured log entries become `warning` events. Pandoc `--sandbox` is a reader/writer guard, not OS isolation.

After Pandoc exits, the host rewrites package metadata timestamps (`docProps/core.xml` `created` and `modified` timestamps for DOCX, and `EPUB/content.opf` `dcterms:modified` and `dc:date` for EPUB) to the conversion time, honoring `SOURCE_DATE_EPOCH` for reproducible goldens. Whether an engine may use loopback to contact its own `llama-server` child for `ocr-vlm` remains open; all other engine network access stays disabled.

For reader conversions (`docx` or `epub` to `ariad-ir+json`), the engine runs:

```text
pandoc +RTS -M{cap}M -RTS --sandbox --log=<workspace>/log/pandoc-log.json -f <docx|epub> -t json --extract-media=<work_dir>/media <input_path>
```

For archive inputs (DOCX, EPUB), the engine always bounds Pandoc memory usage with `+RTS -M<cap>M -RTS`. If `limits.max_memory_mb` is set, `<cap>` is `max_memory_mb` (e.g. 4096 MB under cloud defaults). Otherwise, `<cap>` defaults to `(limits.max_decompressed_bytes / (1024 * 1024)).max(512)` in megabytes (e.g. 4096 MB under local defaults), ensuring that archive reader conversions are memory-bounded even when `max_memory_mb` is omitted. Stdout is captured with a byte cap of `max_ir_json_bytes` and pre-scanned for JSON depth before deserializing the Pandoc AST. The AST is mapped to IR v0, media files in `<work_dir>/media` referenced by the document are verified within the directory without following symlinks and hashed into `AssetStore`, and the output document is written to `<output.dir>/document.ir.json`.

### 7.1 Engine Packs

The base installation must remain lightweight. Heavy engines are downloaded on demand:

| Pack | Contents | Default |
|---|---|---|
| `core` | ashift, PDFium, Pandoc, Typst | Included in base install |
| `docling` | Python 3.14 (python-build-standalone) + uv + Docling + Tesseract 5.5.3 with `tessdata_best` `vie`/`eng`/`osd` + RapidOCR 3.9.2 and PP-OCRv6 models | Download on demand |
| `office` | Uses existing system LibreOffice 26.8; guides official installation if absent | Auto-detected |
| `ocr-extra` | Additional Tesseract language packs beyond `vie`/`eng`/`osd` | Download on demand |
| `ocr-vlm` | llama.cpp + PaddleOCR-VL-1.6 GGUF | Opt-in download |
| `media` | FFmpeg (LGPL), libvips | Later phase |

Each pack includes a manifest specifying version, SHA-256, and a **minisign signature**. `ariad-host` verifies signatures before extracting packages into the application data directory.

---

## 8. Three Execution Modes

### 8.1 Browser (Local)

```text
File ─stream→ OPFS ─→ Web Worker ─→ ariad-wasm / pandoc.wasm / PDFium WASM / Typst ─→ OPFS ─→ download
```

- Files **never enter React state**. Workers read and write via OPFS (`createSyncAccessHandle`).
- WASM modules are **lazy-loaded per route** and cached using Cache Storage. The home page loads zero engines.
- Multithreaded WASM requires `SharedArrayBuffer`, necessitating COOP/COEP headers. `Cross-Origin-Opener-Policy: same-origin` and `Cross-Origin-Embedder-Policy: credentialless` are enabled exclusively on `/app/*` routes, preventing OAuth popups and embedded content on other pages from breaking.
- When file inspection detects that OCR, legacy Office formats, or oversized files are involved, the UI presents two choices: **"Open in Desktop"** or **"Process in Cloud"**. Users always have complete visibility into where their data travels.

### 8.2 Desktop

- **Tauri 2.12** with React 19 + Vite 8. Shared UI components sourced from `packages/ui`.
- Commands are strictly typed via **tauri-specta**: TypeScript types generated from Rust, never written by hand.
- `src-tauri` calls `ariad-host` directly without intermediate sidecars.
- Plugins used: `deep-link`, `updater`, `single-instance`, `dialog`.
- Desktop-specific features: batch conversion, **folder watching** (`notify` crate), background queues, offline mode, and engine pack installations.
- **Web → Desktop bridge:** The web app triggers `ariadshift://open?route=pdf-docx&profile=editable`. Because browsers cannot pass arbitrary file handles to native apps, the desktop app opens a file picker with the preselected route. A localhost server is intentionally avoided, as Chrome 142+ prompts for Local Network Access permissions, degrading user experience.
- macOS 26 icon: Liquid Glass icon authored in Icon Composer (`.icon`) from the layers in `brand/icon-composer/`, compiled via `actool`, with `.icns` as fallback. Other platforms use `tauri icon` with `brand/png/ariadshift-app-icon-1024.png`.
- Operating system integration in later phases: macOS Finder Quick Actions, Windows 11 Explorer context menus (requiring `IExplorerCommand` and sparse packaging), and Linux Nautilus/Dolphin integration.

### 8.3 Cloud

```mermaid
sequenceDiagram
  participant B as Browser
  participant N as Next.js (Better Auth)
  participant A as ariad-server api
  participant S as S3 (R2 / SeaweedFS)
  participant Q as Postgres + pgmq
  participant W as ariad-server worker

  B->>N: Obtain short-lived JWT (JWT plugin)
  B->>A: POST /v1/uploads (Bearer JWT)
  A-->>B: Presigned URLs for each part (multipart)
  B->>S: Parallel upload via Uppy, resumable
  B->>A: POST /v1/jobs {upload, to, profile}
  A->>Q: INSERT job + pgmq.send(queue by pack)
  A-->>B: 202 {id, status: queued}
  B->>A: GET /v1/jobs/{id}/events (SSE)
  W->>Q: pgmq.read (visibility timeout)
  W->>S: Download input into tmpfs workspace
  W->>W: ariad-host runs engine in sandbox
  W->>S: Write output
  W->>Q: Update status + pg_notify
  A-->>B: SSE progress / done
  B->>A: GET /v1/jobs/{id}/outputs
  A-->>B: Presigned download URL (expires in 5 minutes)
```

---

## 9. Cloud Backend

### 9.1 `/v1` API

| Method | Path | Purpose |
|---|---|---|
| POST | `/v1/uploads` | Create multipart upload |
| POST | `/v1/uploads/{id}/parts` | Sign URLs for upload parts |
| POST | `/v1/uploads/{id}/complete` | Complete upload |
| POST | `/v1/plan` | Dry-run planner, return route and scores |
| POST | `/v1/jobs` | Create conversion job |
| GET | `/v1/jobs/{id}` | Job status |
| GET | `/v1/jobs/{id}/events` | Real-time progress via SSE |
| GET | `/v1/jobs/{id}/outputs` | Output download URLs |
| DELETE | `/v1/jobs/{id}` | Immediately delete input and output |
| GET | `/v1/capabilities` | Formats, routes, benchmark scores |

OpenAPI 3.1 generated via utoipa → `packages/sdk` generates the TypeScript SDK. Clients are never hand-written.

### 9.2 Data

- **Owned by Better Auth:** `user`, `session`, `account`, `verification`, `apikey`, `jwks`.
- **Owned by AriadShift:** `uploads`, `jobs`, `job_steps`, `job_events`, `artifacts`, `usage_events`.
- Job lifecycle: `queued → running → succeeded | failed | canceled → expired`.
- Migrations managed via `sqlx migrate`. **Back up before every migration** in production environments.

### 9.3 Queues and Workers

- **pgmq** is the default queue: requires no additional services, supports visibility timeouts, and includes job archiving. Each engine pack operates an independent queue (`jobs_core`, `jobs_docling`, `jobs_office`).
- Queues sit behind a `JobQueue` trait. A **NATS JetStream** adapter will be introduced if throughput exceeds PostgreSQL capacity. This decision is strictly metrics-driven, never premature.
- Workers share a single binary launched with varying arguments: `ariad-server worker --packs docling`. Each pack is containerized into a dedicated image (`worker-core`, `worker-docling`, `worker-office`) and scales based on queue depth.
- Progress updates are recorded in `job_events` and broadcast via `pg_notify`; the API listens and streams events to clients via SSE. No Redis or dedicated WebSocket servers are required.

### 9.4 Object Storage

- Powered by the `object_store` crate: a single unified API across R2, S3, GCS, Azure, and local disk.
- **Self-hosted:** SeaweedFS (Apache-2.0, single-command deployment). RustFS reached 1.0 on 2026-10-03; worth monitoring, but not yet chosen as default.
- **Hosted tier:** Cloudflare R2 (zero egress fees).
- Storage keys follow `t/{tenant}/{job}/{in|out}/{sha256}`, encrypted at rest (server-side encryption), and governed by automatic expiration lifecycle rules.

### 9.5 Tenancy, Entitlements, and Metering

- Every account owns a workspace (tenant) from day one. A personal account starts with a one-member workspace; object storage keys already include `t/{tenant}`.
- Resolve limits and quotas from a `plan_entitlements` record keyed by plan, starting with `free`. Handlers never hard-code entitlements; `ariad-core::Limits` is the shape the record fills.
- `usage_events` is an append-only, idempotent ledger with a unique event key. It records pages, bytes, and engine-seconds per tenant.
- Introduce a `BillingProvider` adapter only when paid plans launch. The provider remains an open question.

---

## 10. Authentication

| Flow | Implementation |
|---|---|
| Web | Better Auth 1.7 in Next.js: email/password, GitHub, Google, **passkeys**, 2FA |
| Web → API | Better Auth JWT plugin issues 15-minute tokens; Axum verifies against cached JWKS (`/api/auth/jwks`) via `jsonwebtoken` |
| API key | Better Auth API Key plugin; hashed storage; Axum validates against the `apikey` table (read-only access) |
| Desktop | System browser OAuth + PKCE → `ariadshift://auth/callback`; tokens stored in OS keychain |
| Anonymous | Small files permitted; rate limited by IP + Cloudflare Turnstile |

Self-hosted deployments do not require external OAuth: email/password authentication is fully self-contained.

---

## 11. Security and Sandboxing

### 11.1 Threat Model

| Threat | Example | Mitigation |
|---|---|---|
| Decompression bomb | 10 KB OOXML/ZIP expands to 100 GB | Copy before inspect (TOCTOU guard); central directory preflight; stream-level byte counting with `take(limit+1)`; entry count and decompressed byte caps; Pandoc archive reader runs bounded by `+RTS -M<cap>` derived from `max_decompressed_bytes` |
| Structural bomb | 2,000,000-page PDF, 100k × 100k image | Pre-inspection via PDFium before processing; limits on pages, pixels, and object counts |
| Embedded code | LibreOffice macros, JavaScript in PDF | LibreOffice: maximum macro security, per-process user profiles (`-env:UserInstallation`); non-V8/non-XFA PDFium builds |
| SSRF / local file inclusion | Remote images in HTML, XXE in OOXML, `\input` in LaTeX | Pandoc `--sandbox`; zero engine network access; Landlock restricts filesystem access to workspace |
| Parser vulnerabilities | CVEs in C/C++ libraries | Isolated processes + seccomp; cloud workers isolated via gVisor (`runsc`); Renovate; fuzzing |
| Data leakage | Content logged in traces, lingering temp files | Zero content logging; hashed filenames; tmpfs workspaces purged after every job; storage TTLs |
| Resource exhaustion | Heavy job spamming | Per-user/IP quotas, Turnstile verification, queue prioritization |

### 11.2 Default Limits (Configurable)

| Limit | Cloud | Local |
|---|---:|---:|
| `max_input_bytes` | 2 GB | Unlimited |
| `max_pages` | 5,000 | Unlimited |
| `timeout_s` | 600 | Unlimited |
| `max_memory_mb` | 4,096 | Unlimited |
| `max_asset_bytes` | 50 MB | Unlimited |
| `max_nesting_depth` | 64 | 64 |
| `max_blocks` | 1,000,000 | 10,000,000 |
| `max_front_matter_bytes` | 64 KiB | 64 KiB |
| `max_archive_entries` | 10,000 | 20,000 |
| `max_decompressed_bytes` | 2 GiB | 4 GiB |
| `max_ir_json_bytes` | 3 GiB | 6 GiB |

An unlimited value is represented by an absent field or `null` (`None` in Rust); it adds no timeout and no Pandoc `+RTS -M` flag. Local runs remain cancellable: Ctrl-C (and Ctrl-Break on Windows) kills the engine process tree and removes its workspace. The nesting limit stays below comrak's internal list-depth cap of 100. Unlike workload limits, archive and IR JSON limits are finite locally on purpose to guard against bombs.

IR JSON reading (`ariad_host::ir_io::read`) streams incrementally through a bounding reader adapter to prevent memory amplification and stack overflows:
1. **Streaming byte cap:** `BoundedScannerReader` counts incoming bytes against `max_ir_json_bytes` while streaming directly into serde, eliminating whole-file buffering and double residency. Exceeding the cap immediately aborts the stream with a typed `limit_exceeded` error before allocating the document tree.
2. **Streaming depth scanner:** the adapter incrementally feeds chunks to an I/O-free state machine (`ariad_core::json_depth::JsonDepthScanner`) that tracks JSON `[` and `{` depth outside string literals without recursion. Each IR nesting level produces 2 to 5 levels of JSON nesting (e.g. nested lists or tables in table cells); the budget formula `json_depth_budget_for_nesting(depth) = (depth * 8) + 64` covers legitimate deep nesting while rejecting excessive depth before deserializing, preventing stack overflow on build or drop.
3. **Deep deserialization:** `serde_stacker` deserializes on a grown stack.
4. **Block count check:** `Document::block_count` asserts total blocks (body and furniture) stay within `max_blocks`. Every failure maps to a typed `limit_exceeded` error.

### 11.3 Isolation Layers

- **Linux (CLI, worker):** rlimits + **Landlock**, restricting filesystem access and blocking TCP connections (Linux ≥ 6.7) + seccomp.
- **Cloud:** Worker containers execute under gVisor; read-only root filesystem; workspaces mounted on tmpfs.
- **macOS / Windows (desktop):** Isolated processes with optional timeout and memory limits. Seatbelt (macOS) and Job Objects (Windows) will be introduced in later hardening phases.

---

## 12. Privacy and Data Lifecycle

- Local Mode: files **never leave the user's machine**, as clearly indicated in the UI.
- Cloud Mode: files are **deleted after 1 hour** by default; an immediate "Delete Now" button is provided; users can opt for "Delete after download."
- Download links are presigned URLs that expire in 5 minutes.
- User documents are never used to train any models.
- Desktop and CLI telemetry is **opt-in** and never contains document content or file names.

---

## 13. Observability

- `tracing` + OpenTelemetry (OTLP) → OpenTelemetry Collector → configurable backends. Self-hosters can deploy Grafana LGTM; hosted deployments can use any OTLP-compatible vendor.
- Key metrics: `conversion_duration_seconds{route,profile}`, `conversion_success_ratio`, `queue_wait_seconds{pack}`, `pages_processed_total`, `engine_peak_memory_bytes`, `engine_crash_total{engine}`.
- Tracing propagates across `api → queue → worker → engine`, correlated by `job_id`.
- Document content is **never** logged. Filenames are recorded exclusively as hashes.

---

## 14. Quality: Testing and Benchmarking

| Layer | Tooling | Scope |
|---|---|---|
| Unit | cargo-nextest, Vitest, pytest | Pure business logic |
| Snapshot | insta | IR generated from each fixture |
| Golden | `fixtures/` | Route outputs against reference standards |
| Structural metrics | `bench/` | Text CER/WER, heading tree distance, TEDS for tables, reading order |
| Visual regression | Typst/LibreOffice → PDF → PDFium raster → SSIM | "faithful" profile routes |
| Conformance | `schemas/engine-protocol.v1.json` | Protocol compliance across all engines |
| Fuzzing | cargo-fuzz 0.13.2 | Core readers, limit validation (stable Rust with `-s none`, Linux CI only) |
| E2E | Playwright (web), WebDriver (Tauri) | Primary user journeys |

- Bench lives in `bench/` as a uv workspace member: jiwer 4.0.0 (CER/WER), rapidfuzz, apted 1.0.3, and our own TEDS. It generates `capabilities.json`, which is committed to the repository.
- The planner reads this file; the website's "Quality" page displays metrics directly from it.
- Every PR modifying an engine re-runs the benchmark suite and **reports score differentials**.
- Fuzzing runs via cargo-fuzz 0.13.2 on stable with `-s none`, Linux CI only; the `fuzz/` crate is excluded from the Cargo workspace and never distributed.
- `cargo test --doc` runs alongside cargo-nextest because nextest does not run doctests.
- DOCX goldens snapshot each fixture's sorted ZIP entry list and pretty-printed XML parts with insta; they hash template-derived XML parts and media. A shared `PANDOC_GOLDEN_VERSION` is asserted, and `SOURCE_DATE_EPOCH=1700000000` fixes timestamps.
- Fixtures use only distributable public-domain, CC-BY, or generated documents; exclude CC-BY-SA, GPL test suites, and research-only or non-commercial datasets. `fixtures/manifest.toml` records each file's source and license.

---

## 15. Licensing

| Component | License | Integration Method | Obligations |
|---|---|---|---|
| AriadShift | Apache-2.0 | — | — |
| PDFium | BSD-3-Clause / Apache-2.0 | Dynamic library | Include notice |
| Pandoc | GPL-2.0-or-later | Isolated process (aggregation) | Include license + link to source code |
| Docling + weights | MIT + Apache-2.0 / CDLA-Permissive-2.0 / MIT | Isolated process | Include notices |
| RapidOCR / ONNX Runtime | Apache-2.0 / MIT | Bundled in docling pack | Include notices |
| Tesseract + `tessdata_best` | Apache-2.0 | Isolated process; `vie`/`eng`/`osd` bundled in docling pack | Include notices |
| llama.cpp | MIT | Isolated process in the opt-in `ocr-vlm` pack | Include notice |
| PaddleOCR-VL weights | Apache-2.0 | Downloaded in the opt-in `ocr-vlm` pack | Include notice |
| LibreOffice | MPL-2.0 | Isolated process, not bundled by default | — |
| Typst | Apache-2.0 | Crate | Include notice |
| FFmpeg / libvips | LGPL-2.1+ | LGPL build, dynamic linking or process | Include license, permit library replacement |
| Plus Jakarta Sans | OFL-1.1 | Outlines in logo | — |

- **Excluded:** AGPL-licensed libraries in distributed artifacts, such as PyMuPDF and its dependents like `pdf2docx`; CC-BY-SA and GPL test-suite fixtures; research-only or non-commercial datasets; and model weights with OpenRAIL-M, custom, non-OSI, or missing licenses.
- **Automated CI gating:** `cargo-deny` (licenses + security advisories), plus npm and Python license checkers. Every release generates `THIRD_PARTY_LICENSES` and an SBOM.

---

## 16. Tooling, CI/CD, and Distribution

- **Repository:** Use the personal account `bavanchun` for the public repository `bavanchun/AriadShift`; no GitHub organization.
- **Task runner:** `just` is the sole root entry point. Phase 0 recipes are `fmt`, `lint`, `test`, `wasm`, `deny`, `js`, `py`, `pandoc`, and `ci`, wrapping pnpm, cargo, and uv. `dev` and `bench` arrive with `apps/` and `bench/`.
- **CI (GitHub Actions):** Run the Linux / macOS / Windows matrix on every push and pull request using `ubuntu-26.04`, `macos-26`, and `windows-2025`. Pin every action to a full commit SHA. CI runs formatting, linting, tests, conformance suites, and license audits; benchmarks run on labeled PRs and nightly schedules.
- **Dependency updates:** Renovate, grouped by ecosystem, strictly adhering to the LTS policies defined in Section 2.
- **Release channels:**

| Target | Tooling | Distribution Channels |
|---|---|---|
| CLI | dist 0.33.0 | GitHub Releases, Homebrew tap, shell/PowerShell installers, `cargo binstall`, winget |
| Desktop | Tauri bundler + `tauri-action` | Signed + notarized `.dmg`, signed `.msi`/NSIS, AppImage/deb/rpm, auto-updates |
| Server | Docker buildx, multi-arch | GHCR; signed with cosign; SBOM |
| Self-hosted | `infra/compose` | `docker compose up`: web, api, worker, postgres, seaweedfs |

- CLI distribution uses dist 0.33.0. Targets: `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`, `x86_64-pc-windows-msvc`. Installers: shell, PowerShell, Homebrew tap `bavanchun/homebrew-tap` with a run dependency on `pandoc`. cargo-binstall and winget (package identifier `VChun.AriadShift`, moniker `ashift`). The dist installer is fetched by exact version and checked against a recorded SHA-256 (`allow-dirty = ["ci"]`).
- **Budgeted code-signing costs:** Apple Developer account ($99/year) and a Windows code-signing certificate (e.g., Azure Trusted Signing).

---

## 17. Roadmap

| Phase | Scope | Acceptance Criteria |
|---|---|---|
| **0 · Foundations** | Monorepo, 3-OS CI matrix, IR v0 + schema, engine protocol v1 draft, ≥ 50 document fixtures suite, branding, one end-to-end route (MD → IR → DOCX through the engine protocol) | `just ci` green on Linux, macOS, Windows; the MD → DOCX route passes its golden test |
| **1a · v0.1 CLI + MCP, core routes** | `convert / inspect / plan / engines / doctor / mcp`; planner over `bench/` scores; routes: MD↔DOCX/HTML/EPUB via IR + Pandoc; cargo-fuzz targets for `ariad-core` readers and limit validation | `capabilities.json` consumed by planner; fuzz targets run in CI; distributed via Homebrew |
| **1b · v0.2 CLI, heavy routes** | Engine pack mechanism (manifest, SHA-256, minisign verification, `ashift engines install`); `docling` pack; routes: PDF→MD/HTML/JSON/DOCX (incl. OCR for scans), Office→PDF, IR→PDF | Engine protocol v1 frozen after the Docling engine passes conformance; public benchmark report covering PDF routes; signed `docling` pack installs and verifies on 3 platforms; scanned PDF→DOCX succeeds locally |
| **2 · v0.3 Desktop** | Tauri app, engine pack management UI, batch processing, folder watching, deep linking, auto-updates | Signed installers for 3 platforms; base install excludes docling pack |
| **3 · v0.4 Web Local** | Next.js site, `/app` running WASM (core, Pandoc, PDFium, Typst) + OPFS, format-pair SEO pages, "Open in Desktop" button | In-browser DOCX↔MD↔HTML↔EPUB conversions; Lighthouse score ≥ 90 |
| **4 · v0.5 Cloud** | `ariad-server` api + worker, pgmq, R2/SeaweedFS, Better Auth, Uppy, SSE, TTL, sandboxing, per-user/IP quotas and rate limits, self-host compose, TS SDK | `docker compose up` executes end-to-end; scanned PDF→DOCX job succeeds in sandbox; quotas enforced before the public cloud opens |
| **5 · v1.0** | Security audit, API stability commitment, remote MCP, OS integration, NATS adapter if warranted by metrics | Zero open high-severity issues; `/v1` API frozen |

This sequence progresses from lowest to highest operational expense. The CLI validates core correctness. The Desktop app establishes differentiation. Web Local incurs zero server costs. Cloud introduces security overhead, hosting expenses, and abuse mitigation, and is therefore tackled last.

Phase 0 ends with one working route rather than an empty scaffold, so the IR and protocol are shaped by a real conversion. The protocol stays a draft until Docling, the first heavy out-of-process engine, has run through it; changing it before then costs nothing. Phase 1 is split so that a usable release ships before the hardest work: 1a proves the IR, planner and engine protocol on lightweight routes, while 1b adds the heavy Python engine, OCR and PDF fidelity. Engine packs land in 1b because the CLI's `engines install` needs them; the desktop only adds a UI on top. Fuzzing starts with 1a because the core readers parse untrusted input from the first release. Quotas ship with the cloud phase because a public cloud without them invites abuse from day one; `plan_entitlements` supplies each tenant's limits.

---

## 18. Decision Log

| Topic | Chosen | Rejected | Rationale |
|---|---|---|---|
| Queue | pgmq on Postgres 18 | NATS JetStream upfront | Eliminates an extra service for self-hosters; SSE leverages LISTEN/NOTIFY; trait abstracts future NATS addition |
| Self-hosted storage | SeaweedFS | MinIO (archived 04/2026), RustFS (just hit 1.0), Garage (AGPL) | Mature, Apache-2.0, single-command setup |
| Storage client | `object_store` | `aws-sdk-s3` | Single API across all backends, including local disk in tests |
| Uploads | Uppy + S3 multipart presigned | tus + tusd | Avoids operating a separate upload server; R2 and S3 natively support multipart |
| UI primitives | shadcn + Base UI | Radix, MUI, Ant | Base UI is the new shadcn default; fully customizable |
| Writing DOCX/ODT/EPUB | IR → Pandoc AST → Pandoc | Custom `docx-rs`, `pdf2docx` | Single adapter targets multiple formats; avoids AGPL dependency chains |
| PDF export | Typst | LaTeX, headless Chromium | Fast, embeddable (crate + WASM), Apache-2.0 |
| OCR | Tesseract for Vietnamese, RapidOCR for other languages, PaddleOCR-VL opt-in | RapidOCR-only, EasyOCR, Surya/Chandra weights | PP-OCRv5/v6 dictionaries lack 88–90 Vietnamese letters in U+1EA0–U+1EF9; Tesseract covers `vi` and mixed `vi`/`en` |
| Binary name | `ashift` | `as` | GNU binutils already uses `as`; Rust crate names retain `ariad-*` |
| Markdown reader | comrak 0.55.0 with default features off and shortcodes on | pulldown-cmark; markdown-rs | Active CommonMark/GFM implementation, pure Rust build, and an AST for the core reader |
| Local limit defaults | Unlimited input, pages, asset bytes, time, and memory; finite nesting, block, and front-matter limits | Generous fixed local input/time/memory caps | Explicit unlimited semantics preserve local use while structural limits remain finite |
| Tenancy and metering | Per-account workspace, plan-keyed entitlements, append-only usage events; billing adapter later | Per-handler quota constants; billing before paid tiers | Supports a free launch and lets paid plans arrive without changing the limits contract |
| In-process native readers | Markdown and HTML in `ariad-core`, with pure Rust, typed errors, finite depth, node, and block limits, and fuzzing from 1a | Running every native reader out of process | Enables lightweight native routes under a documented, bounded exception to Principle 3 |
| Python fixture generator | Dev-only uv workspace member at `fixtures/gen/`; dependencies never ship | Shipping fixture-generation dependencies | Reproducible fixture creation without runtime dependencies |
| IR version policy | `ariad-ir/0` with `schemas/ir.v0.json` through 0.x; freeze `/1` with the protocol in 1b | Starting Phase 0 at `/1` | Allows breaking changes while the foundation is still a draft |
| DOCX timestamp rewrite | Host rewrites `created` and `modified` after Pandoc, honoring `SOURCE_DATE_EPOCH` | Keeping Pandoc's sandbox epoch-zero timestamps | User-facing documents get conversion-time metadata and golden outputs remain reproducible |
| Repository publication | Public `bavanchun/AriadShift`; pinned 3-OS CI on pushes and pull requests | Private repository or floating action refs | Matches the accepted public-repository decision and keeps CI actions immutable |
| Product identifiers | `ariadshift.ariadnev.com`, its `/schemas/` ID base, bundle `com.ariadnev.ariadshift`, and `ariadshift://` | Unrelated domains or bundle identifiers | Keeps web, schema, desktop, and deep-link identities aligned |
| Desktop | Tauri 2.12 | Electron, Tauri 3 alpha | Lightweight, directly embeds Rust core; avoids unvetted alphas |
| Desktop Python runtime | python-build-standalone + uv | PyInstaller | Reproducible, minimizes antivirus false positives |
| Web framework | Next.js 16 LTS | Vite SPA, Astro | Requires SSG/SSR for SEO pages; unifies dashboard and auth in one application |
| TS linting/formatting | Biome 2.5 | ESLint + Prettier | Extremely fast; typescript-eslint depends on JS APIs unavailable in TS 7.0 |
| TS-to-Rust Auth | JWT plugin + JWKS | Database session lookups on every request | Stateless, open standard, Axum verifies locally |
| Task runner | just | Turborepo, Nx | Polyglot repo; a lightweight command runner suffices |
| Engine isolation | Out-of-process + NDJSON protocol | Linking libraries directly into host process | Crashes and exploits cannot bring down the host application |
| HTML reader | html5ever 0.40.1 with our own tree sink, fed in bounded chunks so a depth cap stops the parse | scraper, lol_html, tl, Pandoc for HTML input | WASM-safe, latest html5ever, no duplicate parser; html5ever's own stack makes deep nesting quadratic, so the cap must stop feeding, not just re-parent nodes. |
| MCP file access | Client roots plus `--allow-dir`, canonicalized prefix checks, writes through capability handles, overwrite only with `--allow-overwrite`, hidden paths refused | Any user-readable path; overwrite on the agent's word | A prompt-injected agent must not read private files or destroy user files through the server. |
| Bench | Python under `bench/` with jiwer, rapidfuzz, apted and an in-house TEDS | Packaged TEDS (GPL dependencies), Rust metric crates (stale) | Only permissive metrics libraries are available in Python. |
| Fuzzing | cargo-fuzz on stable 1.99 with `-s none`, Linux CI | Nightly ASan on every PR, Windows fuzzing | No second toolchain on PRs. Our crates forbid `unsafe`, but parse-path dependencies (html5ever, serde_stacker) do not; sanitizer runs are covered by the validation decision recorded in the plan. |
| Linux release binaries | musl, static | glibc builds on ubuntu-26.04 or 24.04 | No glibc floor for users. |
| Release supply chain | Pinned actions through `[dist.github-action-commits]`, SHA-256-verified dist installer; `brew update` accepted as the only unpinned step and run in a job without the tap token; release secrets are fine-grained tokens scoped to one repo, stored in a `release` environment limited to `v*` tags | Unverified `curl \| sh`; classic PATs as repository secrets | Keeps the pinning policy except where Homebrew offers no pin, and keeps a compromised step from reaching a write token. |
| EPUB determinism | Pandoc `--sandbox`, an explicit `identifier` (UUIDv5 from the input hash), a title fallback to the file stem, and `package_meta` stamping of `dc:date` / `dcterms:modified` honoring `SOURCE_DATE_EPOCH` | Running without the sandbox for reproducible dates, random UUIDs, or current wall-clock timestamps | Keeps the sandbox boundary and produces valid, reproducible EPUB3 across builds with matching IR and `SOURCE_DATE_EPOCH`. |
| OCR binding | Tesseract CLI through Docling | `tesserocr` | Covers Windows, uses one Tesseract version, avoids LGPL. |
| Raw HTML in HTML output | Dropping inline raw HTML tags to escaped text with one document-level warning; block raw HTML sanitized through a single `ammonia` 4.2.1 allow-list policy with links validated via `allowed_link` and a single document-level `RawDropped` warning | Hand-rolled inline tokenizer sanitizer (rejected to prevent tag-leak hijacking); passing raw HTML through; dropping all raw HTML | Accepted fidelity-for-safety trade-off for inline fragments; preserves safe block formatting (`div`, `details`, `table`) while eliminating unbalanced tag breakouts and XSS vectors. |
| Raw HTML in Markdown output | Drop raw HTML tags, emit inner text escaped with a single document-level `RawDropped` warning | Passing raw HTML through (sanitized or unsanitized) | Markdown writer drops raw HTML (security trade-off of fidelity for safety); eliminates stored XSS and formatting breakout vectors while the HTML writer keeps its sanitized pass-through for block raw HTML only. |
| Markdown output images | Embedded `data:` URIs | Sidecar asset directory; dropping images | One file keeps single-file atomic promotion and loses no image. |
| Sanitizer fuzzing | A nightly scheduled CI job on a date-pinned Rust nightly, running cargo-fuzz with AddressSanitizer on the in-process parsers; never used for builds or releases | No sanitizer runs | Parse-path dependencies contain `unsafe`; this is the only exception to the no-pre-release-toolchain policy, and AGENTS.md "Versions" names it. |
| crates.io | Publish `ariad-core`, `ariad-host` and `ariad-cli` from a CI job in the `release` environment, in dependency order, with a scoped token | Not publishing (binstall `--git` only) | Gives `cargo binstall ariad-cli` and `cargo install ariad-cli`; the crate names keep the `ariad-*` prefix. |
| winget identifier | `VChun.AriadShift`, moniker `ashift` | `bavanchun.AriadShift`, `AriadShift.AriadShift` | Matches the copyright holder in LICENSE and NOTICE. |
| Engine memory limits | Engine-enforced in 0.x; host limiter in 1b | Generic host-side limiter via `RLIMIT_AS` or `pre_exec` in 1a | `RLIMIT_AS` causes hard PyTorch/GHC static TLS crashes below 6 GiB; `pre_exec` violates `#![forbid(unsafe_code)]` in `ariad-host`. When `limits.max_memory_mb` is `Some`, the host queries `describe` (with 30s timeout), caches capabilities within the process, and refuses with `limit_exceeded` before converting if `enforces_memory_limit` is false. |
| Format wire id | Equals the registry id (`Format::id()`, e.g. `ariad-ir+json`) | snake_case variant identifiers (e.g. `ariad_ir_json`) | Unifies format identity across CLI, IR, engine protocol, and capabilities schema with zero drift. |
| MPL-2.0 dependencies | Ammonia brings MPL-2.0 transitive crates; accepted as unmodified dependencies in native and WASM builds; each new MPL crate needs its own exception | Global MPL-2.0 allow list, hand-rolled HTML sanitizer | Keeps robust ammonia sanitization without opening a global allow-list; complies with OSI-only policy |

---

## 19. Branding

Logos, application icons, and wordmarks are code-generated under `brand/`. Run `pnpm --dir brand build` to regenerate. Design guidelines are documented in `docs/brand/design-direction.md`.

Canonical product identifiers are `https://ariadshift.ariadnev.com` for the site, `https://ariadshift.ariadnev.com/schemas/` as the JSON Schema `$id` base, and `com.ariadnev.ariadshift` as the desktop bundle id. The deep-link scheme remains `ariadshift://`.

---

## 20. Open Questions

1. ~~**Cloud business model**~~ — resolved: free tier first; paid tiers and billing later. Entitlements and metering are specified in §9.5.
2. ~~**CLI command name**~~ — resolved: `ashift`; `as` conflicts with GNU binutils `as`.
3. ~~**Priority OCR languages**~~ — resolved: Vietnamese and English; see §2.4 and §7.1.
4. ~~**Domain name and GitHub organization**~~ — resolved: `ariadshift.ariadnev.com`; no GitHub organization.
5. ~~**Public documentation language**~~ — resolved: this document is maintained in English.
6. **Billing provider:** Which provider should a future `BillingProvider` adapter integrate with when paid tiers launch?
7. **OCR-VLM loopback:** May an engine connect to its own `llama-server` child over loopback, or must it use an in-process binding?
8. **OCR-VLM Python runtime:** Should the optional VLM use Transformers on Python 3.14 or a separate Python 3.13 environment?
