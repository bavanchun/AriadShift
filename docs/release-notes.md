# Release notes

Changes in each AriadShift release, newest first. Installation commands and download tables for every version are on [GitHub Releases](https://github.com/bavanchun/AriadShift/releases). Each entry cites the pull request that merged it into `dev`.

## 0.3.0 (unreleased)

Preview of the PDF suite. `ashift` gains typed document tools: 25 structural PDF tools that merge, split, reorder, rotate, crop, scale and impose pages and edit document structure and metadata without rendering anything. They run through the new `ashift tool`, `ashift tools` and `ashift recipe` commands and through the local MCP server. Conversions, `ariad-engine/1` and `ariad-ir/1` are unchanged. Installers, desktop apps and the web app do not offer the tools yet; this release ships the CLI, the MCP server and the crates.

### Upgrade notes

- **OAuth signup challenge configuration.** When signup is enabled and `AUTH_TURNSTILE_SECRET` is set, auth requires its paired `AUTH_TURNSTILE_SITE_KEY`. Deployments with signup disabled can set the secret without the site key.

- **Two new crates are published.** `ariad-pdf` (pure-Rust PDF manipulation) and `ariad-engine-io` (shared engine protocol helpers) join `ariad-core`, `ariad-typst`, `ariad-host` and `ariad-cli` on crates.io. `ariad-host` now depends on both, so a source build or `cargo install ariad-cli` pulls them in.
- **New exit codes `8` to `12`.** Tool commands report an unknown tool (8), invalid parameters (9), wrong input count or format (10), a tool not allowed by the caller's tier (11) and encrypted input (12). Codes `0` to `6` and `130` keep their meaning. See [cli.md](cli.md#exit-codes) and [tools.md](tools.md#tool-error-exit-codes).
- **Tools never overwrite silently.** A destination that already exists fails with exit 6 unless `--overwrite` is passed. With `--overwrite`, the previous file is moved aside first and restored byte-identical if publication fails, so a multi-file result is published completely or not at all.
- **Encrypted PDFs are refused, never stripped.** Every structural tool exits with code 12 on an encrypted input and points to `pdf.decrypt`, which a later release adds.

### Added

- **25 structural PDF tools**, plus the `pdf.info` inspection tool, all in the free tier and runnable on the CLI and the local MCP server. They need no rendering and no engine pack. The full contract is in [tools.md](tools.md) and one runnable example per tool is in [cli.md](cli.md#ashift-tool):
  - Page sets: `pdf.merge`, `pdf.split`, `pdf.extract_pages`, `pdf.delete_pages`, `pdf.reorder`, `pdf.alternate_mix`, `pdf.insert_blank`.
  - Page geometry: `pdf.rotate`, `pdf.rotate_content`, `pdf.crop`, `pdf.scale`, `pdf.single_page`, `pdf.decompress`.
  - Imposition and layout: `pdf.nup`, `pdf.booklet`, `pdf.overlay`, `pdf.divide_pages`, `pdf.posterize`.
  - Document structure: `pdf.metadata`, `pdf.outline`, `pdf.attachments`, `pdf.page_labels`, `pdf.viewer_prefs`, `pdf.portfolio`, `pdf.layers`.
- **`ashift tools`, `ashift tool` and `ashift recipe run`.** List the registry, run one tool on one or more inputs, or run an `ariad-recipe/1` pipeline. Parameters come from `--param KEY=VALUE`, `--params-json` or a recipe, and are validated against the tool's JSON Schema before anything runs. See [cli.md](cli.md#ashift-tools).
- **Local MCP tools `list_doc_tools` and `run_doc_tool`.** They run the same registry under the server's `--allow-dir` confinement and limits, and publish outputs atomically as resource links. See [mcp.md](mcp.md).
- **Deterministic output.** The same inputs and parameters give byte-identical PDFs. `SOURCE_DATE_EPOCH` fixes the document dates and `/ID`, and object numbers follow traversal order.
- **Repaired-input warning.** A PDF with a damaged cross-reference table is inspected and processed after reconstruction, the tool prints `warning[repaired_xref]`, and the saved file is a fully rewritten valid PDF.
- **`ariad-engine/2` and the tool contracts.** A second engine protocol version carries `op = "run_tool"` for out-of-process tool engines, next to the frozen `ariad-engine/1`. The machine-readable contracts are [`schemas/tools.v1.json`](../schemas/tools.v1.json), [`schemas/recipe.v1.json`](../schemas/recipe.v1.json) and [`schemas/engine-protocol.v2.json`](../schemas/engine-protocol.v2.json).
- **Browser bindings.** `ariad-wasm` exposes `listTools` and `runTool` for the same in-process tools. No page in the web app uses them yet.

### Changed

- **`ariad_pdf::ToolCtx` construction.** Create contexts with `ToolCtx::new`,
  `with_cancel` or `with_deadline`, and use `with_max_pages` to set a page limit.
  The struct is non-exhaustive so future fields can be added without breaking callers.
- **Updated Rust dependencies.** `jsonschema` moves to 0.59.0, `libc` to 0.2.191 and `unicode-normalization` to 0.1.26. `ariad-host` now uses `jsonschema` at run time to validate tool parameters. The licence policy additionally allows `MIT-0` ([deny.toml](../deny.toml)).

### Security

- **Bounded PDF work.** One per-document budget caps stream decompression across object streams, page content, form XObjects and XMP metadata. The trailer scan is linear, and page loops honour the deadline and cancellation of the running tool.
- **Confined tool artifacts.** Engine-reported output files are checked with symlink-safe handles, confined to the job's output directory and size-capped before they are read. Inputs staged for MCP tools are never reopened by path after confinement.
- **Secrets stay out of messages.** Secret parameters are matched case-insensitively and recursively, scrubbed from errors and debug output, and scoped per recipe step. `params_file` must live inside the job directory.
- **Pack verification at resolve time.** Installed engine packs are re-verified against a digest record bound to the minisign-verified manifest, and unlisted executables or extra files in a pack root are rejected.
- **Fuzz coverage.** CI adds four fuzz targets for page ranges, recipes, bounded PDF opening and the structural tools, for 22 targets in all.

### Known limitations

- **Structural tools only.** Rendering-dependent operations arrive in later previews: automatic whitespace cropping (`pdf.crop` accepts only explicit boxes), blank-page detection by appearance (`pdf.split --param mode=blank-separator` finds only pages with no text or drawing content), compression, OCR, annotations, forms, redaction and signatures.
- **No encryption support yet.** `pdf.encrypt` and `pdf.decrypt` are not in this release, so encrypted inputs stop with exit 12.
- **Secret parameters are not available over MCP.** `run_doc_tool` rejects secret-typed parameters until secret handles exist; use the CLI for those.
- **No desktop or web screens for the tools.** The desktop app and the web app keep their conversion-only interfaces in this release.

## 0.2.0 (2026-10-31)

Roadmap 1b. `ashift` adds PDF, Office, OCR and image routes, signed engine packs, and a Linux engine sandbox. This release also adds the desktop app and the in-browser converter. The `ariad-engine/1` engine protocol and the `ariad-ir/1` document schema are now frozen. The self-hosted cloud product, its web mode and the remote MCP endpoint are a **preview**: they are not part of the release archives or the desktop installers, and the [Known limitations](#known-limitations) list what is open.

### Upgrade notes

- **IR JSON is now `ariad-ir/1`.** `--to json`, the MCP server and the browser write `"version": "ariad-ir/1"`, described by [`schemas/ir.v1.json`](../schemas/ir.v1.json). Readers still accept `ariad-ir/0` and upgrade its version on output, and `schemas/ir.v0.json` remains available. Tools that check the version string must accept `ariad-ir/1`. Unknown or missing versions are rejected. (#49)
- **EPUB identifiers change.** EPUB identifiers and the table-of-contents checksums that depend on them are derived from the IR bytes, so a given input now gets different identifiers than it did in 0.1.0. Repeated conversions are still byte-identical. (#49)
- **Typst is the default PDF route.** Markdown, HTML, EPUB and IR JSON always write PDF through the built-in Typst writer, in every profile, even when LibreOffice is installed. LibreOffice renders PDF only from the user's own Office or ODF file, as the first step of a route. (#34, #49)
- **Engines run sandboxed by default on Linux and macOS.** Out-of-process engines run under Landlock and seccomp on Linux, and under a Seatbelt profile on macOS, in `best-effort` mode. Set `ASHIFT_SANDBOX` to `off`, `best-effort` or `required`; unknown values fail closed, and `required` works on Linux and on macOS with usable Seatbelt, but not on Windows. Without isolation support, `best-effort` prints `warning[sandbox_unavailable]` and `required` exits with code 5. (#33, #103)
- **LibreOffice on Linux needs user and mount namespaces.** It runs with a private `/tmp`. If namespace setup is denied, its routes fail with `sandbox_unavailable` (exit 5) in every mode except `off`. On Ubuntu, either allow `userns` for `ashift` in AppArmor or run `sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0`. Default Docker and Kubernetes seccomp profiles also block the setup. See [the CLI reference](cli.md#ashift-convert). (#34)
- **Exit codes cover more cases.** Exit 5 now also reports unavailable required sandbox isolation, a missing Docling engine or Tesseract, and a pack built for a different `ashift` version. Exit 4 now also covers the engine process-tree memory limit and oversized pack archives. [cli.md](cli.md) lists how pack failures map to codes 1 to 5. (#33, #35, #38)
- **Engine packs are versioned with the host.** Docling packs for this release require `ashift` `>=0.2.0, <2.0.0`, following the frozen protocol compatibility policy in [packs.md](packs.md#compatibility), and installs fail with exit 5 on any other version. Packs are verified against the signing keys compiled into `ashift`. (#35, #38, #40)
- **`/v1` is stable.** The cloud API's OpenAPI document carries `"x-stability": "stable"`. Within `/v1` the API changes only in backward-compatible ways, and a test compares the generated document with a frozen snapshot. Breaking changes would ship as `/v2` next to `/v1`. See [api.md](api.md). (#103)
- **The browser checks `pandoc.wasm` against a pinned SHA-256.** Self-hosted web exports must serve the unmodified official Pandoc 3.12 `pandoc.wasm`. The sync script refuses any other binary. A cached or downloaded copy that does not match is evicted, and the conversion stops with an `engine_integrity` error. (#48)

### Added

- **Many inputs in one `convert` call.** With more than one input, `-o` names an existing output directory, and duplicate destinations are refused before anything converts (exit 2). `--json` prints a `results` array in input order. A failed input does not stop the later ones, and the exit code is that of the first failure. Single-input behaviour is unchanged. (#33)
- **New input formats:** PDF (built-in text reader), DOC, XLS, PPT, RTF, ODT, ODS, ODP, PPTX, XLSX, PNG, JPEG, TIFF, WebP and DoclingDocument JSON (`*.docling.json`). **New targets:** `pdf`, `json` (IR JSON), and `xlsx` and `pptx` where a route reaches them. (#34, #38)
- **Built-in PDF reader and Typst PDF writer.** The reader extracts text from digital PDFs. The writer embeds Vietnamese-capable fonts and supports headings, lists and task lists, tables with spans, code, quotes, figures and footnotes. TeX math is kept as text with a `math_not_converted` warning. (#34)
- **Automatic PDF reader selection.** `ashift` checks whether a PDF has a text layer. Digital PDFs use the built-in reader; PDFs that need OCR select Docling. Without Docling, a scanned PDF fails with exit 5 and an installation hint. (#34, #38)
- **`--engine <ENGINE>`** on `convert` and `plan` chooses the input reader, for example `pdf` or `docling`. The requested reader is never silently swapped for another one. (#34, #38)
- **Optional system LibreOffice `>=24.2`** for DOC/RTF/ODT → DOCX, XLS/ODS → XLSX, PPT/ODP → PPTX, and Office/ODF → PDF. Each job uses a fresh profile with macros disabled. (#34)
- **Docling engine pack** (`ashift engines install docling`) for OCR and layout-aware PDF, DOCX, PPTX, XLSX and image extraction. A bounded, fuzzed Rust mapper converts its output to IR. OCR uses the system Tesseract 5 (`vie+eng`), or RapidOCR when every language hint is a non-Vietnamese language that RapidOCR covers. The pack works offline and never downloads anything during a conversion. See [docling.md](docling.md). (#38, #43, #46)
- **Signed engine packs.** `ashift engines install`, `remove` and `verify` check a minisign signature, then each component's size and SHA-256. Interrupted downloads resume, and `--from <DIR>` installs offline. `engines`, `doctor` and MCP `list_engines` report installed packs. See [packs.md](packs.md). (#35)
- **More `inspect` output.** For PDFs it reports page count, text and image-only pages, `needs_ocr`, encryption and producer. It also detects OOXML, ODF, OLE2, RTF and image formats from their content. (#34)
- **Sandbox and pack checks in `doctor`.** It reports the engine sandbox, the pack data directory, trusted signing keys, every installed pack, LibreOffice and the Docling engine. (#33, #35, #38)
- **Warm engine sessions.** Engines that support sessions stay warm across the inputs of one batch, and MCP `convert` calls share one session pool. Idle sessions close after five minutes, and engine processes retire after 100 jobs. (#33)
- **Process-tree memory watchdog.** `max_memory_mb` now caps each engine's whole process tree, on Linux, macOS and Windows. (#33)
- **Clearer `plan` failures.** When an engine was found but failed to start, `plan` prints `startup_failure (<engine>): <cause>`. The plan JSON is unchanged. (#34)
- **Desktop app (new).** A Tauri 2 app for macOS, Windows and Linux that converts through `ariad-host` on the device, so documents are never uploaded. The GitHub Release carries the installers: `.dmg` for macOS, `.msi` and NSIS `.exe` for Windows, and AppImage, `.deb` and `.rpm` for Linux. See [desktop.md](desktop.md). (#40, #50)
  - The conversion queue runs 1 to 4 jobs at once, with cancel, retry, and an explanation of the route before converting.
  - Up to 16 watch folders, polled with a stable-file check. Subfolders are optional. Outputs must be outside the watched folder, which prevents conversion loops. Watches run only while the app is open.
  - Engine packs can be installed, verified and removed in the app, with progress and cancellation, under the same signature policy as the CLI. Diagnostics runs the host `doctor`.
  - Links of the form `ariadshift://open?route=<from>-<to>&profile=<profile>` preselect a route and a profile. They never carry file paths.
  - In-app updates come from GitHub Releases and are verified against the updater public key. An update is never installed without the user's confirmation.
- **Web app (new).** The website and the in-browser converter live in `apps/web`. Conversions run on the device through `ariad-wasm`. Files stay in per-tab private browser storage (OPFS), with a 64 MiB in-memory fallback where OPFS or Web Locks are unavailable. See [web.md](web.md). (#36, #41, #42, #48)
  - Markdown, HTML and IR JSON convert with the core module. (#36)
  - DOCX and EPUB are read and written through the official `pandoc.wasm` 3.12 reactor. Digital PDF text is read in the browser, and PDF output uses the Typst compiler with embedded fonts. (#48)
  - Heavy engines download on first use, with progress, and stay cached for offline use. When a new build activates, the service worker deletes retired engine downloads. The landing page loads no WebAssembly. (#48)
  - Inputs over 16 MB, and routes that need native engines such as OCR, offer **Open in Desktop**. The link carries the route and profile, never the file. (#36, #48)
- **macOS engine sandbox.** On macOS, engines launch through the system `/usr/bin/sandbox-exec` with a deny-default Seatbelt profile: engines read their installation and system libraries, write only their job workspace, and have no IP networking. LibreOffice stays inside the sandbox with a private IPC directory. `doctor --json` adds `sandbox.seatbelt_available`, `sandbox.seatbelt_usable` and `sandbox.seatbelt_enforced`. A Seatbelt profile that is present but refused fails the job with `sandbox_unavailable` in both `best-effort` and `required` modes. See [cli.md](cli.md#macos-engine-sandbox). (#103)
- **Desktop OS integration.** The installers add "Convert with AriadShift" to the file manager for every supported input format except JSON: Open With and Dock drops plus an installable Quick Action on macOS, per-user Explorer verbs on Windows (both the MSI and the NSIS installer install for the current user), and Open With plus Dolphin and Nautilus entries on Linux. Choosing it adds the files to the Convert selection, with at most 1,000 files per launch. See [desktop.md](desktop.md#os-integration). (#103)
- **Cancel for browser conversions.** The converter has a **Cancel** action for in-flight on-device conversions, and **Start over** stays enabled while one runs. (#98)
- **Licence notices page.** The web app serves `/licenses` with the full notices for its JavaScript and Rust dependencies and for the bundled Pandoc WebAssembly build, and CI checks that the page matches the lock files and the production build. (#99)
- **Self-hosted cloud (preview).** `ariad-server` serves the `/v1` HTTP API and the conversion workers. Around it sit an OpenAPI 3.1 document (`just openapi`), the TypeScript client `@ariadshift/sdk`, a Better Auth service (`services/auth`) for accounts, JWTs and API keys, and a Docker Compose stack with `core`, `office` and `docling` profiles on PostgreSQL 18 with pgmq and SeaweedFS S3. The Compose stack builds its images from a source checkout. See [self-hosting.md](self-hosting.md) and [server.md](server.md). (#53)
  - New accounts are disabled by default; set `AUTH_ALLOW_SIGN_UP=true` to allow them. A Cloudflare Turnstile challenge can protect signup. A fleet-wide ceiling, `ARIAD_GLOBAL_ACTIVE_JOBS` (default 64), limits queued and running jobs across API replicas. (#103)
  - `GET /v1/jobs` lists the account's jobs in pages, `GET /v1/limits` reports the caller's plan and quotas, and `DELETE /v1/uploads/{id}` deletes an upload that no job uses. The SDK adds `listJobs`, `limits` and `deleteUpload`. (#103)
  - Signing keys now rotate every 30 days, with a 30-minute publication grace. (#103)
- **Cloud mode in the web app (preview).** A build with `NEXT_PUBLIC_ARIAD_API_URL` set can convert on a self-hosted server and adds the `/sign-in`, `/account`, `/api-keys` and `/jobs` pages. Without that variable, `/app` stays a static, account-free converter. Account deletion revokes the account's API keys first, and cloud jobs stay listed on the jobs page after leaving the converter. See [web.md](web.md#cloud-mode). (#84)
- **Remote MCP (preview).** With `ARIAD_PUBLIC_URL` set, `ariad-server api` serves a stateless Streamable HTTP MCP endpoint at `/mcp` (MCP 2026-07-28) with the tools `list_engines`, `plan`, `inspect`, `convert` and `job_status`. Clients sign in with OAuth 2.1 (dynamic client registration, PKCE) or send an API key in `x-api-key`. Inline `convert` input is Markdown or HTML up to 256 KiB. Larger files go through `/v1/uploads`. See [mcp.md](mcp.md#remote-mcp-over-http). (#103)
- **Library additions.** `ariad-core` adds an in-memory conversion pipeline with inspection, the PDF reader, Typst markup output, the DoclingDocument mapper, and warm-session fields in the engine protocol. `ariad-host` adds the engine registry, the session pool, the Linux sandbox, the memory watchdog, caller-supplied limits and signed pack installation. The new `ariad-typst` crate is the in-memory Typst compiler shared by native builds and the browser, and is published to crates.io with the other library crates. The new `ariad-wasm` crate provides browser bindings for `version`, `capabilities`, `inspect`, `plan` and `convert`. (#33, #34, #35, #36, #37, #38, #48)

### Changed

- **Frozen contracts.** `ariad-engine/1` and `ariad-ir/1` are frozen. Shared conformance tests cover Pandoc, PDF, Typst, LibreOffice and the real Docling engine. (#49)
- **Measured route scores.** Route scores for PDF, OCR and Office now come from English and Vietnamese benchmark inputs instead of unmeasured defaults; see [bench-report.md](bench-report.md). Automatic PDF reader selection still uses the built-in reader for digital text and Docling for scans. (#49)
- **Embedded Markdown images.** Markdown `data:` images now become embedded IR assets, so they survive conversion to DOCX and EPUB. No resource URLs are fetched. (#48)
- **`ashift engines` output.** Human output adds a pack section after the engine table. JSON rows backed by a pack carry a `pack` object, whose `status` can be `ready`, `missing`, `wrong version` or `not installed`. MCP `list_engines` returns the same `pack` object. (#35)
- **No Visual C++ Redistributable for `ashift`.** The Windows release binary now links the C runtime statically. (#46)
- **Docling pack without proprietary Intel components.** x86_64 Docling packs exclude proprietary Intel IPP, oneMKL and Intel OpenMP binaries: OpenCV is built from source without IPP, x86_64 CPU PyTorch is built with static OpenBLAS and Apache-2.0 oneDNN, and ARM packs keep the audited official wheels. LLVM OpenMP is the only OpenMP runtime (with MSVC compatibility wrappers on Windows), Windows native builds use MSVC via Ninja, and a binary and notice gate rejects proprietary Intel artifacts on every target. (#51)
- **MCP limits match the CLI.** `ashift mcp` no longer caps `inspect` and `plan` staging at 32 MiB, and `convert` uses the host's local limits, so JSON output and error codes for large inputs match the CLI. A failed or unanswered roots refresh revokes client-provided roots after ten seconds and keeps the explicit `--allow-dir` directories. See [mcp.md](mcp.md). (#90)
- **Repeated Markdown images are written once.** The Markdown writer emits one reference definition per repeated asset and the HTML writer one embedded copy, instead of encoding the asset again for every reference. Sparse Markdown table rows now omit trailing empty cells, which reads back as the same cells. (#92)
- **Docling pack licence gate.** The pack build checks every installed distribution's metadata and notices, inventories native files against the wheel `RECORD` hash and size, and rejects unlisted vendored libraries. GNU runtime libraries ship with their licence texts and a written source offer. Pillow moves to 13.0.0. (#95)
- **Updated Rust dependencies,** including comrak 0.56.0 and saphyr-parser 0.2.2. (#31)

### Fixed

- In-memory inspection now analyses only complete inputs. (#36)
- Each pack's staging directory now has its own name, so cleaning up one pack never deletes another pack's staged files. (#35)
- Windows Docling installs now print the Visual C++ runtime prerequisite, and Docling import failures on Windows now name their cause. (#43, #46)
- The Markdown, HTML and Typst writers no longer panic on multibyte raw HTML such as `<p>Tiếng Việt</p>`. (#88)
- The HTML reader keeps blocks that appear inside inline or heading contexts, joined with separators and a warning, instead of dropping them. Figure captions, credits, extra blocks and image alt text and titles keep their source order. (#93)
- Local DOCX and EPUB output with a large embedded image no longer fails. The 64 MiB per-entry cap now applies only to the metadata XML entry that is rewritten; other entries are copied unchanged. (#96)
- The browser converter no longer refuses DOCX, EPUB and PDF to Markdown or HTML conversions of inputs under 16 MB. The 16 MB cap applies to the original input only, and the intermediate IR has its own 256 MiB budget. (#97)

### Security

- **Linux engine sandbox.** Out-of-process engines run under Landlock and seccomp. Incomplete network isolation and local socket limits are reported instead of being silently accepted. (#33)
- **Engine artifact reads are confined** to each engine's own output directory. (#33)
- **Pack verification.** Packs are verified as a whole against a pinned maintainer public key, and cached downloads are isolated from one another. (#35, #40)
- **Tesseract confinement.** Sandboxed engines can read Tesseract installation data and shared data only. (#43, #46)
- **IR JSON limits.** IR JSON reads now apply the input byte cap, and empty containers and tables count toward the nesting limit. Unknown limit names in `ariad-wasm` convert options are rejected. (#33, #36)
- **Desktop confinement.** Native desktop commands are limited to the main window capability, and per-job batch request storage is bounded. (#40)
- **Browser engine integrity.** `pandoc.wasm` is verified against its pinned SHA-256 before it is cached or compiled. The service worker serves only hashed assets cache-first. (#36, #48)
- **Reader and writer work is bounded.** Hostile input no longer causes unbounded work or memory in the Markdown reader (inline nesting counts toward the depth limit, and footnote scans are linear), the PDF reader (hyphen joins, font and width arrays, CMaps and operands), the HTML reader (attributes count toward the node budget) and the text writers (output ceilings, capped table output, percent-encoded footnote ids). (#89, #91, #92, #93)
- **IR JSON is validated after it is read.** Untrusted IR JSON from engines, MCP, the web app and the server is walked once before any writer sees it. The walk enforces the nesting depth, the per-asset size cap, that every asset id is the lower-case SHA-256 of its bytes, heading levels, unique footnote ids and table column spans. (#100)
- **MCP staging and sidecar images.** Staging directories are created with mode `0700` and files with `0600`, carry a per-process prefix, and stale ones are swept through verified handles. Explicit `--allow-dir` directories are pinned for the process lifetime. Sidecar images are resolved through the authorized input directory and refuse hidden or symlinked components. (#90)
- **Cloud transfer limits (preview).** A local download releases its admission slot after the idle timeout, upload verification is bounded across storage clones, local downloads are served as attachments with `nosniff`, and the SDK retries a transfer only on 429 and 408, at most three times, within a finite `Retry-After` budget. File-system storage is documented as single-tenant only. (#102)
- **Fuzz coverage.** CI now fuzzes the package metadata stamper, archive preflight, pack archives and manifests, URI parsers, asset resolution, the engine protocol, capability JSON and versioned IR JSON, in addition to the readers and writers, for 18 targets in all. (#103)

### Known limitations

- **Unsigned desktop builds.** Desktop installers are not code-signed for macOS or Windows. Gatekeeper blocks the first launch of the downloaded macOS app until the user allows it. Updater artifacts are still signed and verified. (#40)
- **No Intel macOS desktop build.** macOS installers and in-app updates support Apple silicon only. (#40)
- **Docling pack platforms:** `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu` and `aarch64-apple-darwin` (macOS 14 or later). The Windows `x86_64-pc-windows-msvc` pack is planned for v1.x ([#83](https://github.com/bavanchun/AriadShift/issues/83)); when it ships it needs the latest x64 Microsoft Visual C++ Redistributable. There is no Intel macOS pack and no musl Linux pack. CI builds the `aarch64` Linux pack but does not run it. Every platform needs Tesseract 5 on `PATH` for OCR, but digital documents convert without it. (#38, #43, #46)
- **The sandbox covers Linux and macOS only.** Windows keeps Job Objects and cannot satisfy `ASHIFT_SANDBOX=required`. The macOS sandbox relies on `sandbox-exec`, which Apple deprecates, so `doctor` checks that it is present and usable instead of assuming it. (#33, #103)
- **Pandoc stays external** (`>=3.12, <4`) for DOCX and EPUB in the CLI and the desktop app. (#34, #40)
- **Browser limits.** The first DOCX or EPUB conversion downloads about 61 MB for `pandoc.wasm`. Inputs over 16 MB, OCR, legacy Office and LibreOffice routes need the CLI or the desktop app. The static web export does not fit the Cloudflare Pages 25 MiB file limit. Netlify, GitHub Pages (which serves it without cross-origin isolation) and Node `next start` are supported. (#36, #48)
- **The self-hosted cloud is a preview.** `ariad-server`, the auth service, the Compose stack and the remote MCP endpoint are not in the release archives or the desktop installers; run them from a source checkout as described in [self-hosting.md](self-hosting.md). The web app's cloud mode exists only in builds that set `NEXT_PUBLIC_ARIAD_API_URL`; the default build has no cloud mode. Open items: a failed token refresh falls back to anonymous cloud requests instead of being reported ([#65](https://github.com/bavanchun/AriadShift/issues/65)), social-only accounts have no passwordless account deletion ([#66](https://github.com/bavanchun/AriadShift/issues/66)), large cloud downloads are buffered in browser memory ([#67](https://github.com/bavanchun/AriadShift/issues/67)), the fleet job ceiling's production capacity approval is pending, local file storage is single-tenant only, and Turnstile in cloud builds has not been checked in a real browser. (#53, #84, #102, #103)
- **Desktop installers.** Windows cold-start and installer behaviour for the file-manager entries still needs platform checks. If Explorer starts several processes for a multi-file selection and the startup lock stalls for 10 seconds, more than one window can open. See [desktop.md](desktop.md#os-integration). (#103)
- **Package channels.** 0.1.0 was not published to Homebrew, crates.io or WinGet; 0.2.0 is on crates.io and the Homebrew tap, and all four channels (Homebrew, shell installer, PowerShell installer and cargo-binstall) passed the release smoke run; current Homebrew needs `brew trust --tap bavanchun/tap` before installing; there is no WinGet submission yet.
- **Docling pack.** No Docling pack is published for 0.2.0, so `ashift engines install docling` and the worker-docling image fail closed until the pack work lands (#109).

## 0.1.0 (2026-10-21)

Roadmap 1a: Markdown, HTML, DOCX and EPUB conversion through `ariad-ir+json`, the `convert`, `inspect`, `plan`, `engines` and `doctor` commands, and the `ashift mcp` server. See the [v0.1.0 release](https://github.com/bavanchun/AriadShift/releases/tag/v0.1.0).
