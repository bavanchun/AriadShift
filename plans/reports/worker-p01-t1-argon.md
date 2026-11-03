# Worker Report: Phase 1 T1 — Architecture Contracts, AGENTS.md, and Unused `pdfium-render` Cleanup

- **Date:** 2026-11-01
- **Branch / Worktree:** `docs/next-stage-contracts` (`/Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift-wt-p01-contracts`)
- **Model:** Gemini 4 Argon (High)
- **Status:** DONE

## Changes Made

1. **`ARCHITECTURE.md`**
   - **§1 Architectural Principles:** Added `crates/ariad-pdf` (pure Rust on `lopdf`, `#![forbid(unsafe_code)]`, bounded by `Limits`, fuzzed) to the Principle 3 in-process exemption list for parser-light structural PDF operations and invisible OCR text-layer writing across all runtimes including `wasm32` (AD-2).
   - **§2.3 Rust Crates:** Updated the `lopdf` row so it no longer restricts PDFium to the Docling pack and bench; added workspace crate rows for `ariad-pdf`, `ariad-engine-io`, `ariad-pdfium-engine`, and `ariad-ffi`.
   - **§2.4 Engines and Infrastructure:** Updated the `Tesseract + tessdata_best` (`tesseract`) row and added rows for the `pdfium`, `qpdf`, `sign` (pyHanko), `verapdf` (veraPDF), `ai`, and `inpaint` engine packs.
   - **§4 Product Surfaces, §5 Monorepo Structure, §6.1 Crate Responsibilities:** Added the native `apps/macos` surface (Xcode project, Swift 6, SwiftUI + AppKit over `crates/ariad-ffi` UniFFI bindings, notarized Developer ID + Sparkle) and marked `apps/desktop` (Tauri 2) as maintenance-only (security/bug fixes, no new tools; macOS build retired at `0.7.0`, Windows/Linux retained until native WinUI 3 and GTK apps replace it; AD-7, AD-8). Added `ariad-pdf`, `ariad-engine-io`, `ariad-pdfium-engine`, and `ariad-ffi` plus the new engine pack directories to the monorepo layout and crate responsibilities table.
   - **§7.1 Engine Packs:** Added pack table rows for `pdfium`, `qpdf`, `tesseract`, `sign`, `verapdf`, `ai`, and `inpaint`, and documented generic `ariad_host::engines::resolve_pack(id)` resolution (pack name = engine id), `engine_protocol = "ariad-engine/2"` manifest metadata, and manifest/component SHA-256 verification on `resolve_installed` (AD-14).
   - **§7.3 Engine Protocol v2:** Added the normative `ariad-engine/2` specification (`op = "run_tool"`, `describe`, `capabilities.tools[]`, multi-`artifact` emission, closed error codes reusing `unsupported_route` by default, static `EngineSpec.protocol`, generalised runner `JobRequest` methods, `params_file` for parameters `> 256 KiB` and `x-ariad-secret` secrets under the 1 MiB line cap, no sessions for tools, stateless `edit.apply`, and zero engine network access with SSRF-safe host fetches) along with the canonical request and event JSON examples (AD-3, AD-14..AD-17).
   - **§11.1 Threat Model:** Updated the structural bomb, embedded code, SSRF/LFI, and parser vulnerability rows for `ariad-pdf`, `pdfium`, `qpdf`, `tesseract`, `sign`, and `verapdf` (confirming embedded PDF JavaScript is never executed and forms/annotations/encryption/OCR parsing run out of process), and added a row covering `x-ariad-secret` handling and untrusted recipes (AD-15, AD-16, AD-18).
   - **§15 Licensing:** Split `lopdf` and `PDFium / pdfium-render` (moving PDFium from Docling/bench-only to the out-of-process `pdfium` engine pack, Docling pack, bench, and separately loaded browser WASM with bundled third-party notices), added rows for `qpdf`, `pyHanko + RustCrypto`, `veraPDF` (MPL-2.0 option), and `Florence-2 / OpenCV`, updated `Tesseract + tessdata_best` and `llama.cpp`, and explicitly added `Ghostscript`, `MuPDF`, `OCRmyPDF-with-Ghostscript`, `unpaper`, and `BentoPDF` code to the Excluded list.
   - **§18 Decision Log:** Transcribed `AD-1` through `AD-18` verbatim at the end of the Decision Log table, preserving exact `Chosen`, `Rejected`, and `Rationale` wording.
   - **§20 Open Questions:** Added items 9–11 covering the owner-approved Pro tool list (`ToolAccess`, AD-9), the `big-lama` weights and Places365 dataset license verification gate for the `inpaint` pack, and the minimum macOS baseline (`macOS 15+` with Apple Foundation Models gated to `macOS 26+` and runtime Vietnamese Vision/Foundation Models verification).

2. **`AGENTS.md`**
   - **Current state:** Added a paragraph describing the next-stage PDF and document suite (`ariad-core::tools`, `crates/ariad-pdf`, `ariad-engine/2`, signed engine packs, Web and native macOS `apps/macos` over `crates/ariad-ffi`, and Tauri maintenance-only status) with links to `plans/261031-1422-next-stage-pdf-suite/plan.md`, `ARCHITECTURE.md`, `docs/tools.md`, `docs/parity-matrix.md`, and `docs/i18n.md`.
   - **Architecture changes:** Updated the rules to cover `ariad-pdf`, typed tools in `ariad-core::tools` (which stay out of the conversion capability graph), `ariad-engine/2` (`op = "run_tool"` declared statically in `EngineSpec.protocol`), and zero network access for engines.

3. **`Cargo.toml`**
   - Removed the unused `pdfium-render = "0.9"` entry from `[workspace.dependencies]` after verifying via `rg -n "pdfium" crates apps fuzz` that no crate in the workspace references it (Phase 4 re-adds it to `crates/ariad-pdfium-engine`).

## Commands Run and Results

| Command | Result |
|---|---|
| `source /Volumes/AriadWork/02-Ariadnev-Eco/.cache/env.sh && rg -n "pdfium" crates apps fuzz Cargo.toml` | Before edit: matched only `Cargo.toml:50:pdfium-render = "0.9"`. After edit: 0 matches in `Cargo.toml`, `crates`, `apps`, `fuzz`. |
| `source /Volumes/AriadWork/02-Ariadnev-Eco/.cache/env.sh && rg -n -i "pdfium" ARCHITECTURE.md AGENTS.md` | Confirmed no remaining sentence restricts PDFium to the Docling pack and bench only. |
| `source /Volumes/AriadWork/02-Ariadnev-Eco/.cache/env.sh && just spell` | Passed (`typos` exit code 0, including `plans/reports/worker-p01-t1-argon.md`). |
| `source /Volumes/AriadWork/02-Ariadnev-Eco/.cache/env.sh && just fmt-check` | Passed (`cargo fmt --all --check` exit code 0). |

## Open Questions

- None blocking T1. The stage-level open questions (owner sign-off on the `pro` tool list in `docs/parity-matrix.md` at the Phase 1 gate, written verification of the `big-lama` weights and Places365 dataset terms before enabling LaMa in the `inpaint` pack, and runtime verification of Vietnamese support in Apple Vision accurate mode and Foundation Models on macOS 26+) are recorded in `ARCHITECTURE.md` §20.
