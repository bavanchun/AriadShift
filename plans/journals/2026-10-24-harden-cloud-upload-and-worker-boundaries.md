---
title: Harden cloud upload and worker boundaries
date: 2026-10-24
summary: "Close merged cloud review gaps with real storage, process and engine evidence"
---

# Harden cloud upload and worker boundaries

The cloud follow-up retained the single PUT API while adding independent stored-size sweeps, signed-URL replay cleanup, shared SSE leases and metadata erasure. Runtime role grants keep worker credentials away from identity signing tables.

A procfs review caught an ancestor leak beyond the worker itself: same-user tini retains the initial database/storage environment. The container now keeps the supervisor root-owned and drops the application to UID/GID 10001; Linux workers disable dumpability before helpers start. The container regression checks both boundaries with a cleared same-user process.

The locked Docling pack built locally and converted the repository PDF fixture. A 1,024 MiB budget failed at the engine allocation guard (1,569 MiB data plus 256 MiB headroom); 2,048 and 4,096 MiB succeeded. Planning and job creation now reject Docling below 2,048 MiB without changing seeded plan limits. Current operational decisions and measured limits live in docs/server.md and the linked measurement report.

Local and three-platform validation, the rebased PR and coordinator handoff are tracked in the cloud hardening plan. The coordinator retains merge and release authority.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
