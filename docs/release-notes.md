# Release notes

Changes in each AriadShift release, newest first. Installation commands and download tables for every version are on [GitHub Releases](https://github.com/bavanchun/AriadShift/releases). Each entry cites the pull request that merged it into `dev`.

## 0.2.0 (unreleased)

Roadmap 1b. `ashift` adds PDF, Office, OCR and image routes, signed engine packs, and a Linux engine sandbox. This release also adds the desktop app and the in-browser converter. The `ariad-engine/1` engine protocol and the `ariad-ir/1` document schema are now frozen.

### Upgrade notes

- **IR JSON is now `ariad-ir/1`.** `--to json`, the MCP server and the browser write `"version": "ariad-ir/1"`, described by [`schemas/ir.v1.json`](../schemas/ir.v1.json). Readers still accept `ariad-ir/0` and upgrade its version on output, and `schemas/ir.v0.json` remains available. Tools that check the version string must accept `ariad-ir/1`. Unknown or missing versions are rejected. (#49)
- **EPUB identifiers change.** EPUB identifiers and the table-of-contents checksums that depend on them are derived from the IR bytes, so a given input now gets different identifiers than it did in 0.1.0. Repeated conversions are still byte-identical. (#49)
- **Typst is the default PDF route.** Markdown, HTML, EPUB and IR JSON always write PDF through the built-in Typst writer, in every profile, even when LibreOffice is installed. LibreOffice renders PDF only from the user's own Office or ODF file, as the first step of a route. (#34, #49)
- **Linux engines run sandboxed by default.** Out-of-process engines run under Landlock and seccomp in `best-effort` mode. Set `ASHIFT_SANDBOX` to `off`, `best-effort` or `required`; unknown values fail closed, and `required` works on Linux only. Without kernel support, `best-effort` prints `warning[sandbox_unavailable]` and `required` exits with code 5. (#33)
- **LibreOffice on Linux needs user and mount namespaces.** It runs with a private `/tmp`. If namespace setup is denied, its routes fail with `sandbox_unavailable` (exit 5) in every mode except `off`. On Ubuntu, either allow `userns` for `ashift` in AppArmor or run `sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0`. Default Docker and Kubernetes seccomp profiles also block the setup. See [the CLI reference](cli.md#ashift-convert). (#34)
- **Exit codes cover more cases.** Exit 5 now also reports unavailable required sandbox isolation, a missing Docling engine or Tesseract, and a pack built for a different `ashift` version. Exit 4 now also covers the engine process-tree memory limit and oversized pack archives. [cli.md](cli.md) lists how pack failures map to codes 1 to 5. (#33, #35, #38)
- **Engine packs are versioned with the host.** Docling packs for this release require `ashift` `>=0.2.0, <0.3.0`, and installs fail with exit 5 on any other version. Packs are verified against the signing keys compiled into `ashift`. (#35, #38, #40)
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
- **Library additions.** `ariad-core` adds an in-memory conversion pipeline with inspection, the PDF reader, Typst markup output, the DoclingDocument mapper, and warm-session fields in the engine protocol. `ariad-host` adds the engine registry, the session pool, the Linux sandbox, the memory watchdog, caller-supplied limits and signed pack installation. The new `ariad-typst` crate is the in-memory Typst compiler shared by native builds and the browser, and is published to crates.io with the other library crates. The new `ariad-wasm` crate provides browser bindings for `version`, `capabilities`, `inspect`, `plan` and `convert`. (#33, #34, #35, #36, #37, #38, #48)

### Changed

- **Frozen contracts.** `ariad-engine/1` and `ariad-ir/1` are frozen. Shared conformance tests cover Pandoc, PDF, Typst, LibreOffice and the real Docling engine. (#49)
- **Measured route scores.** Route scores for PDF, OCR and Office now come from English and Vietnamese benchmark inputs instead of unmeasured defaults; see [bench-report.md](bench-report.md). Automatic PDF reader selection still uses the built-in reader for digital text and Docling for scans. (#49)
- **Embedded Markdown images.** Markdown `data:` images now become embedded IR assets, so they survive conversion to DOCX and EPUB. No resource URLs are fetched. (#48)
- **`ashift engines` output.** Human output adds a pack section after the engine table. JSON rows backed by a pack carry a `pack` object, whose `status` can be `ready`, `missing`, `wrong version` or `not installed`. MCP `list_engines` returns the same `pack` object. (#35)
- **No Visual C++ Redistributable for `ashift`.** The Windows release binary now links the C runtime statically. (#46)
- **Updated Rust dependencies,** including comrak 0.56.0 and saphyr-parser 0.2.2. (#31)

### Fixed

- In-memory inspection now analyses only complete inputs. (#36)
- Each pack's staging directory now has its own name, so cleaning up one pack never deletes another pack's staged files. (#35)
- Windows Docling installs now print the Visual C++ runtime prerequisite, and Docling import failures on Windows now name their cause. (#43, #46)

### Security

- **Linux engine sandbox.** Out-of-process engines run under Landlock and seccomp. Incomplete network isolation and local socket limits are reported instead of being silently accepted. (#33)
- **Engine artifact reads are confined** to each engine's own output directory. (#33)
- **Pack verification.** Packs are verified as a whole against a pinned maintainer public key, and cached downloads are isolated from one another. (#35, #40)
- **Tesseract confinement.** Sandboxed engines can read Tesseract installation data and shared data only. (#43, #46)
- **IR JSON limits.** IR JSON reads now apply the input byte cap, and empty containers and tables count toward the nesting limit. Unknown limit names in `ariad-wasm` convert options are rejected. (#33, #36)
- **Desktop confinement.** Native desktop commands are limited to the main window capability, and per-job batch request storage is bounded. (#40)
- **Browser engine integrity.** `pandoc.wasm` is verified against its pinned SHA-256 before it is cached or compiled. The service worker serves only hashed assets cache-first. (#36, #48)

### Docling pack without Intel components

> **Coordinator: verify at merge time.** This entry describes #51, which is still open. Keep it only if #51 merges before the `v0.2.0` tag, and edit it to match the merged change. Otherwise delete this section and the matching sentence under Known limitations.

- x86_64 Docling packs no longer contain proprietary Intel IPP, MKL or Intel OpenMP. OpenCV is rebuilt without IPP, x86_64 CPU PyTorch is built from source with OpenBLAS and oneDNN, and Windows uses LLVM OpenMP. These toolchain wheels are built in this repository's CI and pinned by size and SHA-256. ARM packs keep the audited official wheels. (#51)

### Known limitations

- **Unsigned desktop builds.** Desktop installers are not code-signed for macOS or Windows. Gatekeeper blocks the first launch of the downloaded macOS app until the user allows it. Updater artifacts are still signed and verified. (#40)
- **No Intel macOS desktop build.** macOS installers and in-app updates support Apple silicon only. (#40)
- **Docling pack platforms:** `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `aarch64-apple-darwin` (macOS 14 or later) and `x86_64-pc-windows-msvc`. There is no Intel macOS pack and no musl Linux pack. CI builds the `aarch64` Linux pack but does not run it. On Windows, the pack needs the latest x64 Microsoft Visual C++ Redistributable. Every platform needs Tesseract 5 on `PATH` for OCR, but digital documents convert without it. (#38, #43, #46)
- **The sandbox is Linux-only.** macOS and Windows keep process groups and Job Objects, and `ASHIFT_SANDBOX=required` fails on them. (#33)
- **Pandoc stays external** (`>=3.12, <4`) for DOCX and EPUB in the CLI and the desktop app. (#34, #40)
- **Browser limits.** The first DOCX or EPUB conversion downloads about 61 MB for `pandoc.wasm`. Inputs over 16 MB, OCR, legacy Office and LibreOffice routes need the CLI or the desktop app. The static web export does not fit the Cloudflare Pages 25 MiB file limit. Netlify, GitHub Pages (which serves it without cross-origin isolation) and Node `next start` are supported. (#36, #48)
- **No cloud service.** The web app has no cloud mode. The `ariad-server` crate (the `/v1` HTTP API and conversion workers) is in the source tree but is not packaged or supported in this release. The self-hosted cloud product is planned for 1.0.0. (#36, #37)
- **Package channels.** 0.1.0 was not published to Homebrew, crates.io or WinGet. A channel counts as confirmed for 0.2.0 only after the release smoke workflow passes on it.

## 0.1.0 (2026-10-21)

Roadmap 1a: Markdown, HTML, DOCX and EPUB conversion through `ariad-ir+json`, the `convert`, `inspect`, `plan`, `engines` and `doctor` commands, and the `ashift mcp` server. See the [v0.1.0 release](https://github.com/bavanchun/AriadShift/releases/tag/v0.1.0).
