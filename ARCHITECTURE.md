# AriadShift — System Architecture

> **Status:** Proposal v1 · **Updated:** 2026-10-05
> All version numbers in this document were verified directly against npm, crates.io, PyPI, GitHub Releases, and endoflife.date on 2026-10-05.

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
| rmcp | 3.5 | Official MCP SDK for Rust |
| wasm-bindgen / wasm-pack | 0.2 / 0.15 | Build `ariad-wasm` |
| jsonwebtoken | 11 | JWT verification from Better Auth |
| tauri-specta | 1.0 | TS type generation for Tauri commands |
| cargo-dist | 0.32 | Packaging and distribution for CLI |

### 2.4 Engines and Infrastructure

| Component | Version | License | Role |
|---|---|---|---|
| Docling | 2.133 | MIT (weights: Apache-2.0 / CDLA-Permissive-2.0 / MIT) | Structural document parsing for PDF, images, DOCX/PPTX/XLSX (Heron layout, TableFormer) |
| Granite-Docling | 258M | Apache-2.0 | Optional VLM for complex documents |
| RapidOCR + ONNX Runtime | 3.9 + 1.30 | Apache-2.0 / MIT | OCR (PP-OCR models on ONNX, no PaddlePaddle dependency) |
| Tesseract | 5.x | Apache-2.0 | Fallback multilingual OCR |
| Pandoc | **3.12** (official `pandoc.wasm` available) | GPL-2.0+ | Hub for reading/writing DOCX, ODT, EPUB, HTML, LaTeX, RST |
| FFmpeg | 9.0, LGPL build | LGPL-2.1+ | Audio/video (later phase) |
| libvips | 8.x | LGPL-2.1+ | Large images, batch processing |
| pgmq | 1.13 | PostgreSQL License | In-Postgres job queue |
| NATS | 2.15 | Apache-2.0 | Queue adapter for high-scale needs |
| SeaweedFS | 4.x | Apache-2.0 | Self-hosted S3 (MinIO archived 04/2026) |
| Cloudflare R2 | — | Managed service | Object storage for hosted tier |
| uv / ruff | 0.12 / 0.16 | MIT | Python environment management and linting |

---

## 3. System Overview

```mermaid
flowchart TB
  subgraph surfaces["Product Surfaces"]
    CLI["CLI: ariad"]
    MCP["MCP server: ariad mcp"]
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
  ENG["Engines: PDFium · Pandoc · Docling + RapidOCR · LibreOffice · Typst"]
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
| **CLI** `ariad` | Developers, scripts, CI | Local machine | All installed engines |
| **MCP** `ariad mcp` (stdio) | AI agents: Claude, Cursor, IDEs | Local machine | Same as CLI |
| **Desktop** | Privacy-focused users, large files, batch | Local machine | All, installed via engine packs |
| **Web, Local Mode** | General users | Browser (WASM) | core, Pandoc WASM, PDFium WASM, Typst |
| **Web, Cloud Mode** | Complex files, scans, legacy Office | Server workers | All |
| **REST API + TS SDK** | Developers, enterprises | Server | All |

Representative CLI commands:

```bash
ariad convert paper.pdf --to docx --profile editable
ariad convert ./inbox --to md --recursive --out ./converted
ariad inspect scan.pdf            # PDF type, page count, tables, OCR requirement
ariad plan paper.pdf --to docx    # explain the selected route and rationale
ariad engines                     # installed engines, versions, licenses
ariad engines install docling     # download engine pack
ariad doctor                      # check environment
ariad mcp                         # run MCP server over stdio
```

---

## 5. Monorepo Structure

```text
ariadshift/
├── ARCHITECTURE.md
├── Cargo.toml                 # Cargo workspace
├── rust-toolchain.toml        # Rust 1.99
├── pnpm-workspace.yaml        # pnpm workspace + shared version catalog
├── pyproject.toml             # uv workspace
├── justfile                   # root commands: just dev | test | lint | bench | ci
├── .node-version              # 24
├── crates/
│   ├── ariad-core/            # IR, format registry, planner, limits (I/O-free, WASM-compilable)
│   ├── ariad-host/            # engine runner, sandbox, engine packs, native adapters (feature flags)
│   ├── ariad-cli/             # `ariad` binary (includes `ariad mcp`)
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
├── fixtures/                  # test document suite (distributable documents only) + golden outputs
├── bench/                     # benchmark harness → capabilities.json
├── infra/
│   ├── compose/               # docker-compose.yml for self-hosting
│   └── docker/                # Dockerfile for server and workers
├── brand/                     # code-generated logo and icons
└── docs/
```

Only 5 crates exist initially. Further crate splits occur only when distinct boundaries emerge, such as an adapter requiring an independent release cycle.

---

## 6. Core

### 6.1 Crate Responsibilities

| Crate | Responsibility | WASM | Key Dependencies |
|---|---|---|---|
| `ariad-core` | IR types, format registry, planner, limit enforcement, native Markdown and HTML readers/writers | ✓ | serde, schemars |
| `ariad-host` | Engine process execution, sandboxing, temp workspaces, engine pack management, PDFium/Pandoc/LibreOffice/Docling/Typst adapters | ✗ | tokio, pdfium-render, typst |
| `ariad-cli` | Command-line interface, MCP server | ✗ | clap, rmcp |
| `ariad-wasm` | Browser JS API: plan, convert lightweight routes | ✓ | wasm-bindgen |
| `ariad-server` | `api`: Axum + OpenAPI; `worker`: consumes jobs from pgmq and calls ariad-host | ✗ | axum, sqlx, object_store |

### 6.2 Document IR

The IR serves as the "lingua franca" across readers and writers. Designed in three layers, the layout layer is entirely optional:

```rust
pub struct Document {
    pub meta: Metadata,                 // title, authors, language, source format
    pub body: Vec<Block>,               // semantic tree
    pub assets: AssetStore,             // images, fonts, attachments (content-hash identified)
    pub layout: Option<LayoutIndex>,    // page, bbox, column per block id (when source has layout)
    pub provenance: Vec<Provenance>,    // which engine produced which part, with confidence score
}

pub enum Block {
    Heading { level: u8, content: Vec<Inline> },
    Paragraph(Vec<Inline>),
    List { ordered: bool, items: Vec<Vec<Block>> },
    Table(Table),                       // merged cells, headers, captions
    Figure { asset: AssetId, caption: Vec<Inline> },
    Code { lang: Option<String>, text: String },
    Math { tex: String, display: bool },
    Quote(Vec<Block>),
    Footnote { id: String, body: Vec<Block> },
    PageBreak,
}
```

IR rules:

- **Versioned** (`ariad-ir/1`). JSON Schema is automatically generated into `schemas/`, and any breaking change increments the major version.
- **Tested bidirectional mappings** to `DoclingDocument` (Docling ingestion) and **Pandoc AST JSON** (bridge to Pandoc). This allows the IR to avoid reinventing representations already perfected by these ecosystems.
- **Layout is optional.** The "editable" profile discards layout; the "faithful" profile leverages layout to preserve positioning.

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
$ ariad plan paper.pdf --to docx
input   paper.pdf · pdf (digital) · 37 pages · 7 tables · 4 formulas
route   pdf ─docling→ ariad-ir ─pandoc→ docx
score   fidelity 0.86 · editability 0.95 · estimated 9s · runs locally ✓
alt     pdf ─pdfium(text)→ ariad-ir ─pandoc→ docx · 6x faster but loses table structure
```

### 6.4 Format Matrix (v0.x Scope)

| Format | Read | Write | In Browser |
|---|---|---|---|
| PDF (digital) | Docling (structure), PDFium (text, images, metadata) | Typst (from IR) | PDFium WASM: inspect, text, images, page render |
| PDF (scan) | Docling + RapidOCR | — | ✗ (requires cloud or desktop) |
| DOCX | Docling, Pandoc | Pandoc | Pandoc WASM |
| PPTX, XLSX | Docling | LibreOffice (→ PDF) | ✗ |
| DOC, XLS, PPT, RTF, ODT, ODS, ODP | LibreOffice → OOXML/ODF → reader | LibreOffice | ✗ |
| Markdown (GFM), HTML | ariad-core | ariad-core | ✓ |
| EPUB, LaTeX, RST | Pandoc | Pandoc | Pandoc WASM |
| PNG, JPEG, WebP, TIFF | image crate / libvips (+ OCR when text needed) | image crate / libvips | Basic operations |
| JSON (IR, DoclingDocument) | ariad-core | ariad-core | ✓ |
| Audio, video | — | — | Later phase (FFmpeg + Docling ASR) |

Writing DOCX proceeds via **IR → Pandoc AST → Pandoc**. A single adapter outputs DOCX, ODT, EPUB, HTML, and LaTeX simultaneously. Writing a native DOCX writer in Rust is only justified if benchmarks prove Pandoc to be a bottleneck.

---

## 7. Engine Protocol

Every heavy engine or untrusted data parser runs as an **isolated process** speaking a common protocol: **JSON Lines over stdin/stdout** (`ariad-engine/1`). CLI, Desktop, and Workers invoke engines identically; cloud workers simply layer on job queuing and stricter sandboxing.

Request: a single JSON line sent to stdin.

```json
{"protocol":"ariad-engine/1","job":"job_01JABC","op":"convert",
 "input":{"path":"/work/in/report.pdf","format":"pdf"},
 "output":{"dir":"/work/out","format":"ariad-ir+json"},
 "options":{"ocr":"auto","languages":["vi","en"]},
 "limits":{"max_pages":5000,"timeout_s":600,"max_memory_mb":4096}}
```

Events: multiple JSON lines read from stdout.

```json
{"type":"progress","stage":"layout","done":12,"total":37}
{"type":"warning","code":"font_missing","message":"Font 'Cambria Math' substituted with 'STIX Two Math'"}
{"type":"artifact","path":"/work/out/document.ir.json","format":"ariad-ir+json"}
{"type":"result","ok":true,"metrics":{"pages":37,"elapsed_ms":8421}}
```

Protocol rules:

- Engines **read and write only within the designated workspace** and **open no network connections**.
- Errors must always return `{"type":"result","ok":false,"error":{"code":"...","message":"..."}}`. A non-zero exit code without a result message is treated as a crash.
- The schema is located at `schemas/engine-protocol.v1.json`. All engines must pass a shared conformance test suite.
- PDFium also runs out-of-process: `ariad-host` re-executes its own binary via `ariad __engine pdfium`.

### 7.1 Engine Packs

The base installation must remain lightweight. Heavy engines are downloaded on demand:

| Pack | Contents | Default |
|---|---|---|
| `core` | ariad, PDFium, Pandoc, Typst | Included in base install |
| `docling` | Python 3.14 (python-build-standalone) + uv + Docling + RapidOCR + models | Download on demand |
| `office` | Uses existing system LibreOffice 26.8; guides official installation if absent | Auto-detected |
| `ocr-extra` | Tesseract + language packs | Download on demand |
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
| Decompression bomb | 10 KB OOXML/ZIP expands to 100 GB | Stream-level byte counting; limits on total decompressed bytes, entry count, recursion depth |
| Structural bomb | 2,000,000-page PDF, 100k × 100k image | Pre-inspection via PDFium before processing; limits on pages, pixels, and object counts |
| Embedded code | LibreOffice macros, JavaScript in PDF | LibreOffice: maximum macro security, per-process user profiles (`-env:UserInstallation`); non-V8/non-XFA PDFium builds |
| SSRF / local file inclusion | Remote images in HTML, XXE in OOXML, `\input` in LaTeX | Pandoc `--sandbox`; zero engine network access; Landlock restricts filesystem access to workspace |
| Parser vulnerabilities | CVEs in C/C++ libraries | Isolated processes + seccomp; cloud workers isolated via gVisor (`runsc`); Renovate; fuzzing |
| Data leakage | Content logged in traces, lingering temp files | Zero content logging; hashed filenames; tmpfs workspaces purged after every job; storage TTLs |
| Resource exhaustion | Heavy job spamming | Per-user/IP quotas, Turnstile verification, queue prioritization |

### 11.2 Default Limits (Configurable)

| Limit | Cloud | Local |
|---|---|---|
| Input size | 2 GB | No hard limit |
| Page count | 5,000 | 50,000 |
| Decompressed bytes | 4 GB | 32 GB |
| Archive depth / entry count | 3 / 10,000 | 5 / 100,000 |
| Pixels per image | 200 MP | 1 GP |
| Execution timeout | 10 minutes | Unlimited (cancellable) |
| RAM / CPU | 4 GB / 4 cores | Host-constrained |
| Engine network access | Disabled | Disabled |

### 11.3 Isolation Layers

- **Linux (CLI, worker):** rlimits + **Landlock**, restricting filesystem access and blocking TCP connections (Linux ≥ 6.7) + seccomp.
- **Cloud:** Worker containers execute under gVisor; read-only root filesystem; workspaces mounted on tmpfs.
- **macOS / Windows (desktop):** Isolated processes + timeouts + memory caps. Seatbelt (macOS) and Job Objects (Windows) will be introduced in later hardening phases.

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
| Fuzzing | cargo-fuzz | Core readers, limit validation |
| E2E | Playwright (web), WebDriver (Tauri) | Primary user journeys |

- `bench/` generates `capabilities.json`, which is committed to the repository.
- The planner reads this file; the website's "Quality" page displays metrics directly from it.
- Every PR modifying an engine re-runs the benchmark suite and **reports score differentials**.
- Fixtures use only freely distributable documents (public domain, CC-BY, custom generated), with explicit attribution and licensing per document.

---

## 15. Licensing

| Component | License | Integration Method | Obligations |
|---|---|---|---|
| AriadShift | Apache-2.0 | — | — |
| PDFium | BSD-3-Clause / Apache-2.0 | Dynamic library | Include notice |
| Pandoc | GPL-2.0-or-later | Isolated process (aggregation) | Include license + link to source code |
| Docling + weights | MIT + Apache-2.0 / CDLA-Permissive-2.0 / MIT | Isolated process | Include notices |
| RapidOCR / ONNX Runtime | Apache-2.0 / MIT | Bundled in docling pack | Include notices |
| LibreOffice | MPL-2.0 | Isolated process, not bundled by default | — |
| Typst | Apache-2.0 | Crate | Include notice |
| FFmpeg / libvips | LGPL-2.1+ | LGPL build, dynamic linking or process | Include license, permit library replacement |
| Plus Jakarta Sans | OFL-1.1 | Outlines in logo | — |

- **Excluded:** AGPL-licensed libraries in distributed artifacts, such as PyMuPDF and its dependents like `pdf2docx`. Non-OSI custom licenses (e.g., MinerU's license) are likewise excluded.
- **Automated CI gating:** `cargo-deny` (licenses + security advisories), plus npm and Python license checkers. Every release generates `THIRD_PARTY_LICENSES` and an SBOM.

---

## 16. Tooling, CI/CD, and Distribution

- **Task runner:** `just` serves as the sole entry point (`just dev`, `just test`, `just lint`, `just bench`, `just ci`), wrapping pnpm, cargo, and uv underneath.
- **CI (GitHub Actions):** Matrix builds across Linux / macOS / Windows; Rust caching; runs linting, tests, conformance suites, and license audits; benchmarks run on labeled PRs and nightly schedules.
- **Dependency updates:** Renovate, grouped by ecosystem, strictly adhering to the LTS policies defined in Section 2.
- **Release channels:**

| Target | Tooling | Distribution Channels |
|---|---|---|
| CLI | cargo-dist | GitHub Releases, Homebrew tap, shell/PowerShell installers, `cargo binstall`, winget |
| Desktop | Tauri bundler + `tauri-action` | Signed + notarized `.dmg`, signed `.msi`/NSIS, AppImage/deb/rpm, auto-updates |
| Server | Docker buildx, multi-arch | GHCR; signed with cosign; SBOM |
| Self-hosted | `infra/compose` | `docker compose up`: web, api, worker, postgres, seaweedfs |

- **Budgeted code-signing costs:** Apple Developer account ($99/year) and a Windows code-signing certificate (e.g., Azure Trusted Signing).

---

## 17. Roadmap

| Phase | Scope | Acceptance Criteria |
|---|---|---|
| **0 · Foundations** | Monorepo, 3-OS CI matrix, IR v0 + schema, engine protocol v1, ≥ 50 document fixtures suite, branding | `just ci` green on Linux, macOS, Windows |
| **1 · v0.1 CLI + MCP** | `convert / inspect / plan / engines / doctor / mcp`; routes: MD↔DOCX/HTML/EPUB, PDF→MD/HTML/JSON/DOCX, Office→PDF, IR→PDF | Public benchmark report; `capabilities.json` consumed by planner; distributed via Homebrew |
| **2 · v0.2 Desktop** | Tauri app, engine packs, batch processing, folder watching, deep linking, auto-updates | Signed installers for 3 platforms; base install excludes docling pack |
| **3 · v0.3 Web Local** | Next.js site, `/app` running WASM (core, Pandoc, PDFium, Typst) + OPFS, format-pair SEO pages, "Open in Desktop" button | In-browser DOCX↔MD↔HTML↔EPUB conversions; Lighthouse score ≥ 90 |
| **4 · v0.4 Cloud** | `ariad-server` api + worker, pgmq, R2/SeaweedFS, Better Auth, Uppy, SSE, TTL, sandboxing, self-host compose, TS SDK | `docker compose up` executes end-to-end; scanned PDF→DOCX job succeeds in sandbox |
| **5 · v1.0** | Fuzzing, security audit, quotas, API stability commitment, remote MCP, OS integration, NATS adapter if warranted by metrics | Zero open high-severity issues; `/v1` API frozen |

This sequence progresses from lowest to highest operational expense. The CLI validates core correctness. The Desktop app establishes differentiation. Web Local incurs zero server costs. Cloud introduces security overhead, hosting expenses, and abuse mitigation, and is therefore tackled last.

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
| OCR | RapidOCR (ONNX) | Full PaddleOCR, EasyOCR | Identical PP-OCR models without heavyweight PaddlePaddle/PyTorch runtimes |
| Desktop | Tauri 2.12 | Electron, Tauri 3 alpha | Lightweight, directly embeds Rust core; avoids unvetted alphas |
| Desktop Python runtime | python-build-standalone + uv | PyInstaller | Reproducible, minimizes antivirus false positives |
| Web framework | Next.js 16 LTS | Vite SPA, Astro | Requires SSG/SSR for SEO pages; unifies dashboard and auth in one application |
| TS linting/formatting | Biome 2.5 | ESLint + Prettier | Extremely fast; typescript-eslint depends on JS APIs unavailable in TS 7.0 |
| TS-to-Rust Auth | JWT plugin + JWKS | Database session lookups on every request | Stateless, open standard, Axum verifies locally |
| Task runner | just | Turborepo, Nx | Polyglot repo; a lightweight command runner suffices |
| Engine isolation | Out-of-process + NDJSON protocol | Linking libraries directly into host process | Crashes and exploits cannot bring down the host application |

---

## 19. Branding

Logos, application icons, and wordmarks are code-generated under `brand/`. Run `pnpm --dir brand build` to regenerate. Design guidelines are documented in `docs/brand/design-direction.md`.

---

## 20. Open Questions

1. **Cloud business model:** Will there be a free tier, what are its limits, and will paid tiers exist? This determines quota enforcement and whether a billing system is needed.
2. **CLI command name:** `ariad` (proposed). Avoided `ash` to prevent collision with the `ash` shell.
3. **Priority OCR languages:** Vietnamese + English? Impacts RapidOCR model selection and packaged Tesseract language packs.
4. **Domain name and GitHub organization** for the public repository.
5. ~~**Public documentation language**~~ — resolved: this document is maintained in English.
