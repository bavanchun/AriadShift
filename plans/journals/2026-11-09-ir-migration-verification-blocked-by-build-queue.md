---
title: Versioned IR migration and verification
date: 2026-11-09
summary: Implemented and verified the versioned IR migration after an explicitly resumed build-lock timeout.
---

# Versioned IR migration and verification

The p11/t1 worktree started clean at immutable base 4b51bdf9b786be2d06553eee49f4d72891dbbfab. The approved IR contract was implemented across core types, readers, visitors, version boundaries, CLI/WASM options, schema support and compatibility tests. The frozen v0/v1 schemas and all 158 existing golden snapshots remain byte-identical; 40 separate v2 snapshots were generated from real readers.

Earlier revisions passed the v2 schema generation test, eleven IR style/identity tests and all three reader snapshot suites. Broader validation exposed a stack overflow while deserializing deeply nested upgraded blocks before semantic bounds ran. GDB located the repeated current Block deserialization frames. A JSON-level bounds pass was added, but its final regression check waited behind Phase 10 for 600 seconds and exited 124 before running. Final native byte-cap and geometry compatibility edits are also unverified.

The dispatch stop rule ended dependent verification. No commit, push, merge or ownership handoff occurred. Resume with the exact deep-IR regression, then rerun the IR suites and all remaining host/CLI/workspace/minimal/WASM/JS/fuzz/fmt/clippy/spelling gates. The absolute PM result mailbox records commands, outputs, hashes and pending work. No background process remains from this run.

## Resumption and verified implementation

The coordinator explicitly resumed repairs and delivery. All requested code/runtime checks then passed, including real Pandoc integration, canonical Python without prerequisite skips, a complete WASM build and JavaScript execution. Frozen legacy schema/snapshot bytes and the separate v2 corpus remain unchanged after repairs.

The broader checks found stale construction-ID assertions, a remaining default-version assertion, a missing Asset field and an old Pandoc route-count assertion. Content assertions now use the frozen legacy boundary where appropriate; the dedicated v2 tests cover identities and styles. A large HTML table exposed import-ID allocation overhead, fixed without changing its time bound or canonical hash payload. A deep Typst writer test exposed recursive subtree cloning; an iterative block normalizer restored the existing flatten behavior and preserved identities and opaque content.

The final runtime/static results and exact command logs are in the execution report and absolute PM mailbox. The excluded fuzz lock was synchronized with dependencies already present in the base manifest and root lock, with no dependency choice changed. Final spelling and authorized commit/push receipts are recorded by the delivery handoff. The coordinator still owns independent acceptance before the next wave.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
