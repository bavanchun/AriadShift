---
phase: 4
title: "PDFium engine pack: render, thumbnails, images, text with positions, blank/whitespace detection"
status: pending
priority: P1
effort: "6 tasks, ~8-10 worker-days"
dependencies: [2]
branch: feat/pdfium-engine
milestone: B (engine breadth)
---

# Phase 4: PDFium engine

> Deep-mode note: the Mac PM re-verifies the PDFium build and pdfium-render compatibility (spike result of T1) before dispatching T2-T6.

## Goal

Add PDFium as the first `ariad-engine/2` out-of-process engine (AD-4), shipped as a signed pack, and deliver every tool that needs rendering or precise text geometry. This engine is reused by phases 6, 8, 9, 10 and 12.

## Stack (verified 2026-10-31, `plans/reports/researcher-261031-2113-permissive-pdf-engine-stack.md` §2.2)

- PDFium `chromium/8125` (PDFium 158.0.8125.0, 2026-10-26) from `bblanchon/pdfium-binaries` (wrapper MIT, engine BSD-3-Clause; bundled third-party notices in `licenses/` — FreeType FTL, ICU, libjpeg-turbo, OpenJPEG, LCMS, libpng, zlib, HarfBuzz, Abseil, AGG — must ship).
- Binding: `pdfium-render 0.9.4` (MIT OR Apache-2.0). Its `pdfium_latest` feature targets 7881; T1 proves 8125 works with the feature we pick (or pins a binary build that matches).
- Targets: mac-arm64, mac-x64 (or mac-univ), linux-x64, linux-arm64, linux-musl-x64/arm64. Windows packs stay deferred (ARCHITECTURE §7.1 defers Windows packs; Windows engines have no filesystem/network sandbox).
- Browser: not this phase (phase 14 uses `@embedpdf/pdfium`; note the bblanchon WASM heap is non-growable).

## Design

- Engine id `pdfium` (pack name `pdfium` = engine id), implemented as a Rust binary target in `crates/ariad-pdfium-engine` (`[[bin]] ariad-pdfium-engine`) built on `crates/ariad-engine-io` (phase 2), with `pdfium-render` dynamically loading `libpdfium` from the pack directory (`Pdfium::bind_to_library(path)`). Speaks `ariad-engine/2`, `supports_session: false` (stateless per AD-17), `EngineSpec.protocol = V2`, `sandbox_required: true`.
- `unsafe` stays inside `pdfium-render`; our crate keeps `#![forbid(unsafe_code)]`.
- Pack `pdfium` (manifest `ariad-pack/1` + new `engine_protocol = "ariad-engine/2"` field checked at install and resolve; minisign-signed with the existing key flow in `docs/packs.md`): components = engine binary + `libpdfium` + licences; the pack script verifies the upstream bblanchon tarball SHA-256 (pinned per target) and ad-hoc/Developer-ID codesigns the macOS binaries; index entry in `crates/ariad-host/data/packs.toml` (compiled into `ashift`, so a new pack version ships with an `ashift` release); release tag `pack-pdfium-v<ver>`. Packs are versioned `0.x` and declared unstable until `/2` is final in phase 20.
- Host registration: `EngineSpec` in `crates/ariad-host/src/engines/mod.rs`, `ENGINE_PATHS` in `crates/ariad-host/src/engines/sandbox.rs` (read-only pack dir, private tmp), resolution through `engines::resolve_pack` (phase 2). Server: change `Pack::for_engine` (`crates/ariad-server/src/packs.rs:56-62`) to fail closed for unknown engines; the cloud mapping for `pdfium` itself lands with the phase-13 server tool-jobs task (tools stay `[Wasm, Local]`/`[Local]` until then).
- Never execute PDF JavaScript: do not call `FORM_DoDocumentJSAction`/`FORM_DoPageAAction`; initialise form-fill environment without a JS platform.

## Tools

| Tool id | Params | Output |
|---|---|---|
| `pdf.render` | `{pages, dpi: 36..600 = 150, format: png \| jpeg \| webp, quality?, background: "white" \| "transparent", annotations: bool = true}` | one image per page (`<stem>.p001.png`) or zip when > 50 pages |
| `pdf.thumbnails` | `{pages, max_px: 64..512 = 256}` | images + JSON index |
| `pdf.to_images` | alias of render with defaults per Stirling "PDF to image" incl. single combined tall image option | images |
| `image.to_pdf` | inputs many images (png/jpeg/webp/tiff/heic? — heic only if a permissive decoder is verified); `{page: "fit-image" \| paper, margin_pt, orientation: auto}` | PDF (JPEG passthrough without re-encode) — implemented in `ariad-pdf` if no rendering needed; PDFium only for formats it decodes |
| `pdf.extract_images` | `{pages, format: original \| png, min_px}` | images with original encoding where possible |
| `pdf.text_layout` | `{pages}` | JSON: chars/words/lines with bbox, font name, size, colour |
| `pdf.remove_blank` | `{threshold: 0..1 = 0.99 white ratio, dpi = 50}` | PDF without blank pages (uses render + `pdf.delete_pages`) |
| `pdf.crop_whitespace` | `{pages, padding_pt}` | PDF with CropBox to content bbox |
| `pdf.split` mode `blank-separator` (pixel) | extends phase-3 split | parts |
| `pdf.detect_scanned` | `{}` | JSON per page: has text layer, image coverage ratio |

## Tasks

| Task | Owner (model) | Files owned |
|---|---|---|
| T1 | Gemini 4 Argon high | Spike + report `plans/reports/spike-pdfium-8125.md`: bind 8125 via pdfium-render 0.9.4 on macOS arm64 and Linux x64 (dynamic load; also test linux-musl `dlopen`), render a page, extract text boxes, load a font from bytes and draw Vietnamese text, **`FPDFText_SetText` on an existing subset-font text object with Vietnamese replacement text**, and **incremental save over a signed PDF keeping the earlier signature valid** (validate with pyHanko locally); decide the pin. No product code. Stop and report after 1 day. |
| T2 | Gemini 4 Argon high | `crates/ariad-pdfium-engine/**` (engine skeleton, session cache, describe/capabilities, render/thumbnails/text_layout/extract_images/detect_scanned) |
| T3 | Gemini 4 Argon high | `pdf.remove_blank`, `pdf.crop_whitespace`, blank split, `image.to_pdf` (split between `ariad-pdf` and engine as designed) |
| T4 | Gemini 4 Argon high | Shared pack builder first (`scripts/pack/build-pack.sh` + manifest templating, signing, `engine_protocol` field, upstream SHA-256 pins, macOS codesign) reused by later packs; then `engines/pdfium/pack/**` (licence bundling incl. third-party notices), `just pack-pdfium`, `.github/workflows/pack-pdfium.yml` (matrix of targets; signing only on `pack-pdfium-v*` tags under the protected `release` environment) |
| T5 | Gemini 4 Argon high | Host/server registration + sandbox + `doctor` line + docs (`docs/packs.md`, `docs/cli.md`), ARCHITECTURE §2.4/§7.1 rows |
| T6 | Gemini 4 Argon high | Tests: engine conformance v2, golden renders (pixel-diff with tolerance via `image-compare` 0.5.0 MIT — verify), fuzz target `pdfium_engine_request` (protocol-level), malicious corpus (JS, huge pages) never crashes the host |

## Verification

```sh
source /Volumes/AriadWork/02-Ariadnev-Eco/.cache/env.sh
just pack-pdfium && export ASHIFT_PACK_PDFIUM=$PWD/target/packs/pdfium
cargo nextest run -p ariad-pdfium-engine -p ariad-host -p ariad-cli --features ariad-host/test-probe
cargo run -p ariad-cli -- tool pdf.render fixtures/pdf-tools/multipage-20.pdf --param pages=1-2 --param dpi=150 -o /tmp/r
cargo run -p ariad-cli -- doctor
just deny && just licenses-check
cargo clippy -p ariad-pdfium-engine -p ariad-host -p ariad-cli --all-targets -- -D warnings   # narrowed gate; no full just ci locally
```

## Acceptance

- [ ] Spike report merged; pin decided with evidence.
- [ ] All tools above run via CLI/MCP with the pack installed and fail with exit 5 + install hint without it.
- [ ] Sandbox denies writes outside the job dir and any network (Linux + macOS tests).
- [ ] `javascript-action.pdf` renders without executing JS (assert no JS callback invoked).
- [ ] Pack builds for every listed target in CI; notices complete (`licenses/` + `THIRD_PARTY_LICENSES` entry).
