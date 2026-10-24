---
phase: 1
title: "Cloud hardening"
status: completed
---

# Cloud hardening

## Implementation

- [x] Sweep pending S3 object sizes before completion, handle signed-URL replay and document a store-facing body cap.
- [x] Protect worker credentials from same-user procfs access without privileged container features; prove the boundary with real processes.
- [x] Cover cross-account and anonymous access for job delete, outputs, events, upload completion and upload-based planning.
- [x] Document and test migration-owner, API and worker database roles.
- [x] Warn on private peers without trusted proxy identity and reject ambiguous client-address lists.
- [x] Bound SSE streams by subject and job across replicas, releasing and recovering leases.
- [x] Document proxy limits on readiness and capabilities endpoints.
- [x] Redact configuration Debug; reclaim abandoned workspaces safely; detach sweep locks; sanitize errors; release idle queue permits.
- [x] Erase retired filenames and prune expired job/event metadata without modifying usage accounting.
- [x] Document container kernel/syscall/runtime requirements and add a real container smoke job to CI.
- [x] Retry missing Docling engines with a generic client error.
- [x] Test unit and HTTP PDF routing to the Docling queue.
- [x] Explain signed-pack/host-version gates; build and measure Docling on a real PDF or reject budgets below the supported engine minimum.

## Validation and delivery

- [x] Focused regressions, real PostgreSQL/SeaweedFS suite, container build and smoke.
- [x] `just ci`, `just test-server`, commit lint and full diff self-review.
- [x] Rebase onto current `origin/dev`, push, open PR into `dev`, and verify CI on three operating systems.

## Risk and rollback

Use an additive migration for upload checks and stream leases; retain the immutable usage ledger and at least 24 hours of upload quota rows. Runtime roles are opt-in, applied by the schema owner after all identity and server migrations. Roll back application commits only with a compatible migration-aware binary; retain the additive schema and role grants until no deployed binary uses them. Keep required isolation fail-closed; never downgrade to make a smoke test pass.

## Local verification evidence

- `just ci` passed with three Cargo jobs and three nextest threads under the shared lock: 759 workspace entries passed (nine existing ignored cases), doctests, 64 benchmark tests, capability validation, four crate archive checks, TypeScript, 59 JavaScript tests and the production web build. Two existing PowerShell mutation checks skipped because this Linux host has no `pwsh`; their absence was reported by the test harness.
- `ARIAD_TEST_REQUIRE_SANDBOX=1 ARIAD_TEST_CONTAINER_SMOKE=1 just test-server` passed 91/91 against real PostgreSQL/pgmq and SeaweedFS, with zero skipped tests, then built/checked the images and completed a real required-mode Markdown job. The smoke confirmed UID 10001, empty inherited/permitted/effective/ambient capabilities, `no_new_privs`, and denied access to worker/supervisor credentials. The final cleanup uses SIGTERM before removing the owned application containers; a focused container rerun passed that path.
- `ARIAD_TEST_SANDBOX_SUPPORT=none ASHIFT_SANDBOX=off cargo nextest run -p ariad-server --test isolated -E 'test(required_mode_reaches_the_engine_through_the_helper)'` passed the selected regression.
- The actual locked Docling pack built locally. [Memory evidence](../reports/261024-docling-cloud-memory.md) records a real PDF failure at 1,024 MiB and successes at 2,048/4,096 MiB; no substitute engine or model was used.
- Full diff self-review, formatting, spelling and relative Markdown link checks passed. The cook simplification pass removed a redundant metadata-pruning dispatch without changing SQL behavior. Runtime sandbox and publication limitations are explicit in the owning server documentation.

## Public verification evidence

[PR #44](https://github.com/bavanchun/AriadShift/pull/44) targets `dev`. The [CI run](https://github.com/bavanchun/AriadShift/actions/runs/40049542736) passed on revision `0e5c8e1`, including Verify on Ubuntu 26.04, macOS 26 and Windows 2025, static checks, Windows lint, Web, Fuzz and the aggregate gate. The server job ran 91/91 tests with zero skips, checked the unavailable-sandbox regression, built/checked the images and completed the required-mode container conversion. The [Docling pack run](https://github.com/bavanchun/AriadShift/actions/runs/40049542731) also passed real-engine conformance and its pack build.

The completion commit receives another rebase, local quality gate and public CI check before the PR is marked ready. Final revision, CI and readiness evidence belong in the coordinator handoff report; this delivery does not merge the PR or publish a release.
