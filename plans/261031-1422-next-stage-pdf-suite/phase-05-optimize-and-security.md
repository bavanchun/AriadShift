---
phase: 5
title: "Optimize and security: compress (incl. target size), qpdf pack, encrypt/decrypt/permissions, repair, linearize, sanitize"
status: pending
priority: P1
effort: "5 tasks, ~8 worker-days"
dependencies: [3, 4]
branch: feat/optimize-security
milestone: B (engine breadth)
---

# Phase 5: Optimize and security

> Deep-mode note: scout-and-detail pass by the Mac PM before dispatch.

## Stack (verified 2026-10-31, stack report §2.3-2.4)

- qpdf 12.4.2 (Apache-2.0) as engine pack `qpdf` (pack name = engine id): the pack ships the pinned qpdf binary (upstream SHA-256 verified, macOS codesigned); the adapter is `ashift __engine qpdf` in `crates/ariad-host/src/engines/qpdf.rs` (the `pandoc_bin` pattern, AD-14), `EngineSpec.protocol = V2`, `sandbox_required: true`.
- In-process codecs in `ariad-pdf` (feature `codecs`, not in wasm unless size allows): `image` 0.25.10, `jpeg-encoder` 0.7.1 (`(MIT OR Apache-2.0) AND IJG` — IJG notice), `oxipng` 10.2.1 (MIT), `subsetter` 0.2.6 (MIT OR Apache-2.0). mozjpeg (IJG + BSD-3) only if jpeg-encoder quality/size is insufficient (decide by measurement in T1).
- Excluded: Ghostscript (AGPL), imagequant/pngquant (GPL — never linked; not shipped this phase).
- JBIG2 (jbig2enc, Apache-2.0 with patent note): **not shipped** in this stage (owner, 2026-10-31); revisited in phase 20 with a patent/legal check.

## Tools

| Tool id | Notes |
|---|---|
| `pdf.compress` | `{level: 1..9 \| preset: "screen" \| "ebook" \| "print" \| "lossless", target_bytes?: u64, grayscale?: bool, max_dpi?: u32, jpeg_quality?: u8}`. Pipeline: dedupe streams → recompress Flate → downsample/re-encode images (in-process) → subset/unembed unused fonts → qpdf object streams + unreferenced-resource removal. `target_bytes`: bounded binary search over (max_dpi, jpeg_quality) ≤ 8 iterations, stops at the best result ≥ target; reports achieved size and visual similarity (render diff of 3 sample pages via PDFium) — never returns a result below a similarity floor without a warning. |
| `pdf.grayscale` | convert images and colour operators to gray |
| `pdf.encrypt` | `{user_password?, owner_password, algorithm: "aes-256" = default \| "aes-128", permissions: {print, modify, copy, annotate, fill_forms, extract_accessibility, assemble, print_high}}` via qpdf; passwords only via params, never logged; params redacted in logs/reports |
| `pdf.decrypt` | `{password}` via qpdf |
| `pdf.permissions` | change permissions (needs owner password) |
| `pdf.repair` | qpdf `--qdf`/rebuild xref; report what was repaired |
| `pdf.linearize` | qpdf `--linearize` |
| `pdf.sanitize` | `{javascript, embedded_files, links, metadata, xmp, thumbnails, annotations_of_type: [..], forms: "keep" \| "flatten" \| "remove"}` in `ariad-pdf` (+ PDFium flatten when requested) |
| `pdf.unlock_forms` | remove read-only flags from fields |

## Tasks

| Task | Owner (model) | Files owned |
|---|---|---|
| T1 | Gemini 4 Argon high | `crates/ariad-pdf/src/tools/{compress,grayscale}.rs` + image pipeline `src/images/**` + measurement report (size/quality vs Stirling levels on 10 corpus files) |
| T2 | Gemini 4 Argon high | `crates/ariad-host/src/engines/qpdf.rs` (adapter) + pack `engines/qpdf/pack/**`, `scripts/pack-qpdf.sh` (built on the shared pack builder from phase 4 T4 — extract it there if not yet shared), `just pack-qpdf`, workflow; tools encrypt/decrypt/permissions/repair/linearize |
| T3 | Gemini 4 Argon high | `crates/ariad-pdf/src/tools/{sanitize,unlock_forms}.rs` |
| T4 | Gemini 4 Argon high | Apply the phase-2 secret contract (AD-16) to every password field here (`secret_params` on each spec), end-to-end tests that passwords never appear in argv, logs, stderr, `--json` output, MCP results, recipe files or panic messages |
| T5 | Gemini 4 Argon high | Host/server registration, sandbox, docs, ARCHITECTURE rows; end-to-end tests incl. `encrypted-aes256.pdf` round-trip and `broken-xref.pdf` repair |

## Acceptance

- [ ] `pdf.compress` beats or matches Stirling's level 6 output size on ≥ 7 of 10 corpus files with visual similarity ≥ 0.98 (report table).
- [ ] `target_bytes` hits within 5 % or reports the closest achievable with similarity.
- [ ] Passwords never appear in logs, reports, MCP outputs, or panic messages (test greps captured stderr).
- [ ] Sanitized `javascript-action.pdf` has no `/JS`, `/JavaScript`, `/OpenAction` JS.
