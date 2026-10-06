---
title: Windows Clippy lint repair
date: 2026-10-06
summary: "Fixed the Windows-only Clippy denial; Linux CI passes, while local target cross-clippy is blocked by missing platform toolchains."
---

# Windows Clippy lint repair

## What happened

CI run 37402122033 failed on Windows because `trim_end_matches` used a manual character comparison in the Windows-only device-name validator. Replaced it with the character-array pattern required by Clippy. Source review found no additional concrete defect in runner cleanup, CRLF handling, DOCX replacement, or workspace promotion.

## Decision

Keep the repair scoped to the reported lint finding. The Linux `just ci` gate passed with all 89 tests. Windows MSVC cross-clippy stopped in `stacker` because this Linux environment lacks the Windows SDK; macOS cross-clippy stopped in `psm` because the Apple toolchain/SDK is unavailable.

## Next steps

Coordinator should push commit `4c49dfa940e1c2ed12e05a4ca1cf37712d362ba3` and verify the three-OS GitHub CI matrix, especially Windows runner tree-kill, cancellation, cleanup, and DOCX replacement tests. AgentWiki publish skipped.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
