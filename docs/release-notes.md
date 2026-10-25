# Release notes

Changes in each AriadShift release, newest first. Installation commands and download tables for every version are on [GitHub Releases](https://github.com/bavanchun/AriadShift/releases).

## 0.2.0 (unreleased)

Roadmap 1b. `ashift` gains PDF, Office, OCR and image routes, signed engine packs and a Linux engine sandbox, and the release adds the desktop app, the browser converter and the self-hosted cloud server. The `ariad-engine/1` engine protocol and the `ariad-ir/1` document schema are frozen.

### Breaking changes and upgrade notes

- **IR JSON is `ariad-ir/1`.** `--to json`, the MCP server and the browser now write `"version": "ariad-ir/1"`, described by [`schemas/ir.v1.json`](../schemas/ir.v1.json). Readers still accept `ariad-ir/0` documents and upgrade the version on output; `schemas/ir.v0.json` stays available. Tools that check the version string must accept `ariad-ir/1`. Unknown or missing versions are rejected. (#49)
- **EPUB identifiers change.** EPUB identifiers and the dependent table-of-contents checksums are derived from the IR bytes, so the same input produces different identifiers than 0.1.0. Repeated conversions remain byte-identical. (#49)
- **Linux engines run in a sandbox by default.** Out-of-process engines run under Landlock and seccomp in `best-effort` mode. Set `ASHIFT_SANDBOX` to `off`, `best-effort` or `required`; unknown values fail closed, and `required` is Linux-only. Missing kernel support prints `warning[sandbox_unavailable]` in `best-effort` mode and exits with code 5 in `required` mode.
- **LibreOffice on Linux needs user and mount namespaces.** When LibreOffice is installed, it runs with a private `/tmp`. If namespace setup is denied, its routes fail with `sandbox_unavailable` (exit 5) in every mode except `off`. On Ubuntu, allow `userns` for `ashift` in AppArmor or run `sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0`. Default Docker and Kubernetes seccomp profiles also block the setup. See [the CLI reference](cli.md#ashift-convert).
- **Exit codes cover more cases.** Exit 5 now also reports unavailable required sandbox isolation, a missing Docling engine or Tesseract, and a pack that requires a different `ashift` version. Exit 4 also covers engine process-tree memory and oversized pack archives. Pack failures map to codes 1 to 5 as listed in [cli.md](cli.md).
- **`ashift engines` output grows.** Human output adds a pack section after the engine table. JSON rows backed by an engine pack carry a `pack` object, and `status` can be `ready`, `missing`, `wrong version` or `not installed`. MCP `list_engines` returns the same `pack` object.
- **IR-family inputs write PDF through Typst.** Markdown, HTML, EPUB and IR JSON always use the built-in Typst writer for PDF, in every profile. LibreOffice only renders the user's own Office or ODF file as the first route step. (#49)

### CLI (`ashift`)

- `convert` accepts several inputs in one call. With more than one input, `-o` names an existing output directory, and duplicate destinations are refused before conversion (exit 2). `--json` prints a `results` array in input order; a failed input does not stop later ones, and the exit code is the first failure's code. Single-input output is unchanged.
- New input formats: PDF (built-in text reader), DOC, XLS, PPT, RTF, ODT, ODS, ODP, PPTX, XLSX, PNG, JPEG, TIFF, WebP and DoclingDocument JSON (`*.docling.json`). New targets: `pdf`, `json` (IR JSON), `xlsx` and `pptx` where a route reaches them.
- `--engine <ENGINE>` on `convert` and `plan` selects the input reader, for example `pdf` or `docling`. A requested reader never silently falls back to another reader.
- PDF input is checked for a text layer. Digital PDFs use the built-in reader; PDFs that need OCR select Docling. Without Docling, a scanned PDF fails with exit 5 and an installation hint.
- `inspect` reports PDF page counts, text and image-only pages, `needs_ocr`, encryption and producer, and sniffs OOXML, ODF, OLE2, RTF and image formats.
- `ashift engines install`, `remove` and `verify` manage signed engine packs. Installs verify a minisign signature against keys compiled into `ashift`, then each component's size and SHA-256, and resume interrupted downloads. `--from <DIR>` installs offline. See [packs.md](packs.md).
- `doctor` reports the engine sandbox, the pack data directory, trusted signing keys, each installed pack, LibreOffice and the Docling engine.
- `max_memory_mb` is enforced by a host watchdog over each engine's whole process tree on Linux, macOS and Windows.
- Engines that support sessions stay warm across the inputs of one batch.
- The Windows release binary links the C runtime statically and needs no Visual C++ Redistributable.
- `plan` shows `startup_failure (<engine>): <cause>` when an engine was found but could not start. The plan JSON is unchanged.

### MCP server (`ashift mcp`)

- `convert` calls share a warm engine session pool. Idle sessions close after five minutes, and processes retire after 100 jobs.
- Engines run under the same Linux sandbox and `ASHIFT_SANDBOX` setting as the CLI.
- `list_engines` lists installed engine packs. Installing packs stays a CLI and desktop operation.

### Desktop app (new)

- A Tauri 2 app for macOS, Windows and Linux that converts through `ariad-host` directly; documents are never uploaded. Installers are `.dmg` (macOS), `.msi` and NSIS `.exe` (Windows), and AppImage, `.deb` and `.rpm` (Linux), attached to the GitHub Release.
- Conversion queue with up to four concurrent jobs, cancel, retry and a route explanation before converting.
- Watch folders: up to 16 polled folders with a stable-file check, optional subfolders and loop protection. Watches run while the app is open.
- Engine pack install, verify and remove with progress and cancellation, using the same signature policy as the CLI. Diagnostics runs the host `doctor`.
- `ariadshift://open?route=<from>-<to>&profile=<profile>` links preselect a route and profile. Links never carry file paths.
- In-app updates from GitHub Releases, verified against the updater public key. Updates are offered and never installed without confirmation.

See [desktop.md](desktop.md).

### Web app (new)

- The website and in-browser converter in `apps/web`. Conversions run on the user's device through `ariad-wasm`; files stay in per-tab private browser storage (OPFS), with a 64 MiB in-memory fallback where OPFS or Web Locks are unavailable. Inputs over 16 MB are refused and offered a desktop hand-off.
- Markdown, HTML and IR JSON convert with the core module. DOCX and EPUB read and write through the official `pandoc.wasm` 3.12 reactor, which is checked against a pinned SHA-256 before use. Digital PDF text is read in the browser, and PDF output uses the Typst compiler with embedded fonts. (#48)
- Heavy engines download on first use with progress and stay cached for offline use. The service worker removes retired engine downloads when a new build activates. The landing page loads no WebAssembly. (#48)
- Inputs that need native engines, such as OCR, offer **Open in Desktop**. The link carries the route and profile, never the file. (#48)
- Cloud processing is hidden in the interface.

See [web.md](web.md).

### Cloud server (new, self-hosted)

- `ariad-server` provides the `/v1` HTTP API and conversion workers: one presigned upload, a job, server-sent job events and short-lived download URLs. The contract is [`schemas/openapi.v1.json`](../schemas/openapi.v1.json) (OpenAPI 3.1); see [api.md](api.md).
- Workers convert through `ariad-host` under each plan's limits, with separate `core`, `office` and `docling` queues and container images built from `infra/docker/`.
- Storage is PostgreSQL plus an S3-compatible object store. `api` and `worker` refuse to start with unapplied or unknown migrations.
- Isolation: a root `tini` supervisor drops to UID 10001 with `no_new_privs`, workers are non-dumpable, helper engines cannot open non-UNIX sockets, and runtime database roles are limited. Uploads, storage and event streams are bounded per tenant.

No hosted AriadShift cloud service is part of this release. See [server.md](server.md).

### Engines and packs

- **Built-in PDF reader** for digital PDF text, and a **Typst PDF writer** with embedded Vietnamese fonts. Typst covers headings, lists and tasks, tables with spans, code, quotes, figures and footnotes; TeX math is kept as text with a `math_not_converted` warning.
- **LibreOffice** `>=24.2` (optional, system-installed) for DOC/RTF/ODT → DOCX, XLS/ODS → XLSX, PPT/ODP → PPTX and Office/ODF → PDF. Each job uses a fresh profile with macros disabled.
- **Docling engine pack** (`ashift engines install docling`) for OCR and layout-aware PDF, DOCX, PPTX, XLSX and image extraction. Its output maps to IR through a bounded, fuzzed Rust mapper. OCR uses the system Tesseract 5 (`vie+eng`), or RapidOCR when every language hint is a non-Vietnamese language it covers. The pack works offline and never downloads at conversion time. See [docling.md](docling.md).
- Pack builds are reproducible, hash-pinned and gated by a licence check. x86_64 packs exclude proprietary Intel IPP, MKL and Intel OpenMP: OpenCV is rebuilt without IPP, CPU PyTorch is built from source with OpenBLAS and oneDNN, and Windows uses LLVM OpenMP. These toolchain wheels are built in this repository's CI and pinned by size and SHA-256. ARM packs keep the audited official wheels. (#51)
- **Contracts frozen:** `ariad-engine/1` and `ariad-ir/1`. Shared conformance tests cover Pandoc, PDF, Typst, LibreOffice and the real Docling engine. (#49)
- **Measured routes:** route scores for PDF, OCR and Office come from English and Vietnamese benchmark inputs instead of unmeasured defaults; see [bench-report.md](bench-report.md). Typst remains the default PDF writer. Automatic PDF reader selection is unchanged: the built-in reader for digital text, Docling for scans. (#49)

### Libraries and contracts

- `ariad-core` adds an in-memory conversion pipeline and inspection, the PDF reader, Typst markup output, the DoclingDocument mapper and warm-session fields in the engine protocol. IR JSON reads now apply the input byte cap, and empty containers and tables count toward the nesting limit.
- `ariad-host` adds the engine registry, session pool, Linux sandbox, process-tree memory watchdog, caller-supplied limits and signed pack installation.
- New `ariad-typst` crate: the in-memory Typst compiler shared by native builds and the browser. It is published to crates.io with the other library crates. (#48)
- New `ariad-wasm` browser bindings: `version`, `capabilities`, `inspect`, `plan` and `convert`. Unknown limit names in convert options are rejected.

### Known limitations

- **Unsigned desktop builds.** Desktop installers are not code-signed for macOS or Windows until platform signing certificates are configured. macOS Gatekeeper blocks the first launch of the downloaded app until the user allows it. Updater artifacts are still signed and verified.
- **No Intel macOS desktop build.** macOS installers and in-app updates support Apple silicon only.
- **Docling pack platforms:** `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `aarch64-apple-darwin` (macOS 14 or later) and `x86_64-pc-windows-msvc`. There is no Intel macOS or musl Linux pack. CI builds the `aarch64` Linux pack but does not run it. Windows needs the latest x64 Microsoft Visual C++ Redistributable. Every platform needs Tesseract 5 on `PATH` for OCR; digital documents convert without it. The pack requires `ashift` `>=0.2.0, <0.3.0`.
- **Sandbox coverage.** The Landlock and seccomp sandbox is Linux-only. macOS and Windows keep process groups and Job Objects, and `ASHIFT_SANDBOX=required` fails there.
- **Pandoc stays external** for DOCX and EPUB in the CLI, desktop app and server (`>=3.12, <4`). The browser converter downloads about 61 MB for `pandoc.wasm` on first DOCX or EPUB use. The static web export does not fit the Cloudflare Pages 25 MiB file limit; Netlify, GitHub Pages without cross-origin isolation, and Node `next start` are supported.
- **Browser limits.** Inputs over 16 MB, OCR, legacy Office and LibreOffice routes need the CLI or the desktop app.
- **Cloud server.** It is self-hosted only. Docker images are built from the repository, and the Docling worker image needs the published signed pack.
- **Package channels.** 0.1.0 was not published to Homebrew, crates.io or WinGet. Each channel is confirmed for 0.2.0 only after the release smoke workflow passes.

## 0.1.0 (2026-10-21)

Roadmap 1a: Markdown, HTML, DOCX and EPUB conversion through `ariad-ir+json`, the `convert`, `inspect`, `plan`, `engines` and `doctor` commands, and the `ashift mcp` server. See the [v0.1.0 release](https://github.com/bavanchun/AriadShift/releases/tag/v0.1.0).
