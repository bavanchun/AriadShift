---
title: "Cloud backend hardening"
status: in-progress
branch: fix/cloud-hardening
---

# Cloud backend hardening

Outcome: close all thirteen low-severity observations from both reviews of the merged cloud backend, preserving the `/v1` upload and conversion contracts.

Constraints: start from current `origin/dev`; keep conversion in the host; no private documents or new dependency versions; one heavy command at a time under the shared build lock. Rebase before pushing and preserve concurrent PDF/Office and Docling changes.

Non-goals: release publication, production deployment, changing plan quotas, or merging the delivery PR.

| Phase | Status | Dependencies | Execution |
|---|---|---|---|
| Cloud hardening | In progress | Merged cloud backend | [Implementation and verification](phase-01-cloud-hardening.md) |

Acceptance: regression tests cover upload size sweeping/replay, procfs isolation, all tenant-sensitive endpoints, runtime database roles, proxy identity, SSE capacity/release, cleanup and metadata pruning, Docling error/routing/memory behavior. Document store and runtime limits; build the server container and run a real Markdown job in required mode. Pass `just ci`, `just test-server` and three-OS PR CI, then mark the PR ready and write the coordinator report.
