---
phase: 20
title: "Hardening, benchmarks and 1.0 launch (web + macOS)"
status: pending
priority: P1
effort: "6 tasks + coordinator, ~10 worker-days"
dependencies: [19]
branch: release/1.0
milestone: F (1.0)
---

# Phase 20: Hardening, benchmarks and 1.0 launch

> Deep-mode note: scout-and-detail pass by the Mac PM before dispatch. Folds in the on-hold v1-completion phases 7 (security audit) and 8 (distribution) from `plans/261025-1651-ariadshift-v1-completion/`.

## Goal

Prove 1.0 quality with evidence — security, correctness, performance, accessibility, translations, licences — then launch web and macOS 1.0.

## Inputs

- the remote branch `fix/v1-security-audit` (kept on origin, not merged) holds the earlier security-audit material this phase consumes: docs/security/threat-model.md, docs/security/dependency-policy.md, plans/reports/security-audit-261025-v1.md, plans/reports/code-review-261025-phase7-final.md, plans/reports/worker-7-*.md, apps/web/test/headers.test.ts and apps/web/src/components/account/cloud-config.ts; recover with `git checkout origin/fix/v1-security-audit -- <path>` and re-validate against dev before use.

## Requirements

1. **Security audit** (threat model refresh for PDFium/qpdf/Tesseract/AI/inpaint engines, Page-mode edits, signing keys in memory, web OPFS data, Sparkle updates): findings fixed or accepted in ARCHITECTURE §11 residual risks.
2. **Fuzzing**: every new parser/tool has a target; 24 h cumulative clean on CI nightly; corpus minimised and committed.
2c. **JBIG2 decision**: legal/patent check for jbig2enc; ship JBIG2 compression in `pdf.compress` only with the owner's GO.
2b. **Secret scanning**: add a gitleaks-style check to CI (none exists today) and scan history once; no private keys in the repo (test keys are generated at test time).
3. **Correctness proofs** (competitor gaps, automated): redaction leaves no extractable text (text extraction + content stream scan); signatures validate after save in Adobe-compatible validator (pyHanko validate, out of process, test-only); diacritics round-trip; table survival on PDF→DOCX/XLSX.
4. **Footprint & speed benchmark vs Stirling-PDF desktop**: installer size, idle RAM, cold start, merge 1,000 pages, compress 200 MB, OCR 50 pages — published in `docs/bench-report.md`.
5. **Accessibility**: WCAG 2.2 AA web audit; macOS VoiceOver pass; PDF/UA output for tagged exports.
6. **i18n**: string freeze; all 40+ locales complete or marked community; RTL review.
7. **Protocols**: `ariad-engine/2` and `tools.v1`/`ir.v2` schemas flipped to `"x-status": "final"`; `/v1` API additions frozen.
8. **Launch**: web 1.0 production deploy; macOS 1.0 notarized + Sparkle; CLI/crates/Homebrew; docs site pages per tool (SEO, all locales); release notes; WinGet for CLI (gate).

## Tasks

| Task | Owner (model) | Files owned |
|---|---|---|
| T1 | Gemini 4 Argon high | Security audit report `plans/reports/security-audit-1-0.md` + fixes |
| T2 | Gemini 4 Argon high | Fuzz completion + corpus `fuzz/**` |
| T3 | Gemini 4 Argon high | Correctness proof suite `tests/proofs/**` (or crate-local tests) |
| T4 | Gemini 4 Argon high | Benchmarks vs Stirling `bench/competitors/**`, `docs/bench-report.md` |
| T5 | Claude Sonnet 5.5 high | Accessibility + i18n completion (web + macOS) |
| T6 | Claude Sonnet 5.5 high | Docs: `docs/tools/<id>.md` per tool (generated from registry + examples), README, launch pages |
| C1 | Coordinator + Mac PM | Release per `docs/releasing.md` (owner GO for every outward step) |

## Acceptance

- [ ] No open critical/high security findings; residual risks documented.
- [ ] All proofs green; benchmark published with reproducible commands.
- [ ] Parity matrix 100 % implemented on web and macOS (every row checked).
- [ ] Owner GO → v1.0.0 tags, web prod, notarized macOS 1.0.
