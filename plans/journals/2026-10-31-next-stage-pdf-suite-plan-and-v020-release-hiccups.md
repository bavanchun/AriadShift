---
title: Next-stage PDF suite plan and v0.2.0 release hiccups
date: 2026-10-31
summary: "Planned the PDF engine → web → native macOS stage (20 phases, red-teamed), fixed a repo setting that would have deleted dev on promotion, and unblocked a hung v0.2.0 release prep."
---

# Next-stage PDF suite plan and v0.2.0 release hiccups

## What happened

- Owner chose to cut v0.2.0, then start a new stage: full Stirling-PDF + BentoPDF parity plus an "edit PDF like Word" gem, built Engine → Web → native macOS (SwiftUI + UniFFI), Windows/Linux native later.
- Research (Haiku researchers): Stirling teardown, BentoPDF teardown (AGPL, unusable), native macOS + market, UI tech, permissive PDF engine stack, watermark removal. Key fact: AriadShift has no PDF manipulation at all today.
- Plan `plans/261031-1422-next-stage-pdf-suite/`: 20 phases, coordinator protocol, durable mailbox `.run/coord/`.
- Red team (4 Sonnet reviewers): 39 findings → 26 clusters, all applied. Biggest: runner only special-cases `Request::Convert` (v2 jobs would lose limits/sandbox roots), secrets contract too late, redaction scrub incomplete, registry crate cycle + non-deserializable ToolId, tool output bypassing confinement, engine network allowlist impossible (SSRF), lopdf silently stripping owner-password encryption.
- Release v0.2.0: a Gemini Flash worker ran full `just ci`; `cargo nextest` hung 30+ min on three deadlocked Apple `ld` processes, and the worker disassembled Xcode's `ld` with otool for ~45 min. Killed the hung tree; split the rest into 3 parallel workers with a narrowed local gate.
- GitHub: `delete_branch_on_merge` was true (contrary to docs/git-workflow.md) — merging the dev→main promotion would have deleted `dev`. Set it false and added ruleset `protect-main-dev` (no deletion, no force push, no required checks).

## Decision

- Architecture decisions AD-1..AD-18 (tool layer, `ariad-pdf` in-process, `ariad-engine/2` with static protocol per engine, PDFium out of process, stateless edit API, no engine network, secret channels, untrusted recipes).
- Owner: apply all red-team fixes; macOS 15 floor (AI on 26+); Tauri maintenance-only (Mac build retired at 0.7.0); JBIG2 deferred to phase 20; hosted Weblate; web foundation parallel from phase 4; keep menu-bar extra + recipe Quick Look, drop Share/Action extension; URL→PDF via host fetch + offline render.
- Workers must stop and report after 10 min or any hang; the coordinator checks worker tabs every 10 min.

## Next steps

- Finish v0.2.0 (promotion gate, tag, publish, smoke), then close v1-completion and the old roadmap plan.
- Git hygiene queue: stale branch/worktree cleanup, Dependabot #55-#60, docs/git-workflow.md update.
- Start phase 1 of the next-stage plan after owner approval.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
