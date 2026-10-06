---
title: Engine protocol host implementation
date: 2026-10-06
summary: "Completed phase 05 engine protocol, process host, confined assets, Pandoc conversion, and DOCX metadata support; local just ci passes."
---

# Engine protocol host implementation

## What happened
Implemented the AriadShift engine protocol schema and drift check, workspace/promotion behavior, bounded process runner, confined image assets, deep IR JSON reads, Pandoc lookup/engine, hidden `ashift __engine pandoc` entry, and DOCX core-property timestamp stamping. All local `just ci` gates passed on Linux. Pandoc 3.12 accepted every Markdown fixture AST, the 64-level IR converted end to end, and missing-tool/memory-limit cases returned typed errors.

A repeated runner flake was traced to `BrokenPipe` during request writes masking already-emitted child outcomes. The runner now ignores only that write error and continues reading protocol output; focused runner and full gates then passed.

## Decision
The host remains responsible for workspace boundaries, process-tree cleanup and eventual promotion. The engine keeps Pandoc out of process, discards its stdout, drains bounded stderr, uses the workspace temp/log paths, and reports generic errors without resource paths or document content. DOCX stamping preserves raw compressed bytes for all entries except `docProps/core.xml`.

## Next steps
Coordinator should verify the pushed commit set and Linux/macOS/Windows CI, then proceed with phase 06. Windows and macOS process cleanup and DOCX replacement behavior were not executable on this Linux host.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
