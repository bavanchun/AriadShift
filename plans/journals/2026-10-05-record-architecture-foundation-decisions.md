---
title: Record architecture foundation decisions
date: 2026-10-05
summary: Recorded the foundation contracts and implementation boundaries for the first delivery phase.
---

# Record architecture foundation decisions

## What happened
Updated the architecture and repository guidance for the foundations: renamed the user-facing binary to `ashift`, fixed OCR routing for Vietnamese, made the Markdown and front-matter contract implementable, recorded the exact IR and engine protocol shapes, documented local and cloud limits, and specified the initial fixture, CI, tenancy, and product identifiers. A repository-execution amendment required one commit per verified logical step. The Pandoc log path was reconciled with the detailed engine phase through the coordinator mailbox.

## Decision
Native Markdown parsing runs in `ariad-core` with bounded limits and fuzzing planned from 1a. Tesseract handles Vietnamese; RapidOCR covers other supported languages; PaddleOCR-VL remains opt-in. The Phase 0 CLI is `ashift`, and local input, page, asset, time, and memory defaults remain unlimited while structural limits are finite.

## Next steps
The coordinator owns the shared plan status. Later phases should implement the contracts recorded in `ARCHITECTURE.md`; `just ci` and product tests remain unavailable until the workspace is scaffolded.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
