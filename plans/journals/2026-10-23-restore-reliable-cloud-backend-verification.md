---
title: Restore reliable cloud backend verification
date: 2026-10-23
summary: "Restore cloud verification, integrate host isolation, and verify Linux workers against PostgreSQL and S3."
---

# Restore reliable cloud backend verification

## What happened

The takeover found twelve committed steps, unfinished authentication throttling, uncommitted Dockerfiles, and a leftover test PostgreSQL container. Git had no interrupted operation. The test database was backed up before that container was removed.

Database tests reproduced two SSE lifecycle failures: replay after the final cursor never ended, and an open event stream prevented graceful API shutdown. Both now have regression coverage. Rejected credentials are checked against a shared client budget before verification after thirty failures. The PostgreSQL and SeaweedFS suite passed all 62 tests.

An initial workspace test run used a production CLI binary after a concurrent Cargo command rebuilt the same executable without the test feature. Two snapshots correctly detected that their fixture registry was ignored. Cargo verification now runs sequentially; snapshots were preserved.

PR fuzzing exposed a Markdown depth escape: an empty quote chain inside a list could produce IR deeper than the configured limit. The reader now counts each container itself, including tables. The minimized regression and all ten Markdown reader tests passed. The original crash artifact replayed successfully, followed by 226,344 fuzz executions over 61 seconds without a failure.

## Decisions

The Docling worker image extends the Office image so mixed routes have every engine they need. At this stage engine sandbox policy remained a fail-closed seam while its host implementation was pending. Product authority stays in ARCHITECTURE.md and docs/server.md.

## Resumed verification

After the quota reset, the branch rebased onto the signed engine packs in `c24cf97`. Cargo regenerated the combined lockfile and cargo-about regenerated the license inventory. Package verification exposed the already published siblings at the same development version; the recipe now verifies archives against current sibling sources while retaining extraction and compilation.

Three independent Sonnet review passes found three initial medium issues and then a warning-metadata issue. Native conversions now use a killable shared-host helper, with conservative native input admission and deadline/cancellation grace. Planning uses executable host edges. Upload admission accounts for abandoned requests and has a short pending TTL. A 20,000-warning document now succeeds with bounded warning metadata.

The final local quality gate passed 615 workspace tests and 64 benchmark tests. PostgreSQL/pgmq and SeaweedFS integration passed all 73 tests. Duplicate-delivery coverage uses two worker slots with a one-connection query pool. The final review found no remaining high or medium issue beyond the explicitly accepted host RSS dependency.

That source was pushed as `f618f02`; [Source CI](https://github.com/bavanchun/AriadShift/actions/runs/39851372117) passed on all three OSes, with fuzzing and server integration. Refreshed server and Office image builds and non-root smoke checks passed; the image's OpenAPI matched the committed document. Required sandbox mode still failed closed because the host API was unmerged at that point. Engine-pack installation was available, but the signed Docling assets were unpublished: the refreshed Docling build reached the installer and returned HTTP 404 for its manifest.

## Host isolation integration and Linux recovery

The Mac continuation rebased onto `dev@7e2f389`, integrated the real host sandbox and process-tree RSS watchdog, and pushed `622a13e`. [Code CI](https://github.com/bavanchun/AriadShift/actions/runs/39894036713) passed, including Server integration. The worker passes its selected mode explicitly through the cleared helper environment; ambient `ASHIFT_SANDBOX=off` cannot weaken required mode. Native input admission is now `min(64 MiB, max_input_bytes)`. Descendant cleanup snapshots engines while their helper is alive; container lifecycle remains the backstop if the helper dies first.

The Linux takeover found an older interrupted rebase at the duplicate Markdown regression. Recovery branches and an external conflict snapshot preserve that state. The worktree was restored to `origin/feat/cloud-backend`; `git rebase origin/dev` confirmed it was current, and `cargo update --workspace` regenerated resolution without changing `Cargo.lock`. The remaining Mac patch clarified cleanup guarantees and test coverage, and corrected an outdated sandbox API comment.

The Docling build was retried using the existing real Office image (`ariadshift-worker-office:dev`, image `942a2411abbc`) and the signed installer. It failed with HTTP 404 for `pack-docling-v0.2.0/pack.toml`, then the Dockerfile's explicit signed-pack requirement. No substitute pack or successful Docling image was produced; PR #38 was still open. This attempt verifies the pack-installation gate, not a rebuild of the current server and Office images.

The first Linux Docker suite exposed contention in the nested-engine cleanup regression: its 17 MiB native input hit the 20-second conversion deadline before the engine started. Running it alone took about 16 seconds; running it alongside other parser tests reproduced `Err(Convert(LimitExceeded))` with no engine PID. A 2 MiB input reaches the engine in about two seconds under contention while preserving its 230 MiB allocation, 256 MiB aggregate RSS budget, deadline and heartbeat assertions. The three contending tests then passed together; large native-input admission remains covered separately. Temporarily removing descendant cleanup made the revised regression fail again: the engine's heartbeat grew from 0 to 60 bytes after the helper returned. The production cleanup was restored before the final suites.

While verification ran, `dev` advanced with the web foundation. The cloud branch rebased onto `3a2d5ea`, retaining the WASM dependencies and web CI job alongside Server integration. Cargo regenerated the combined lockfile, and cargo-about regenerated the license inventory; neither generated file was hand-merged.

The October 23 Linux `ARIAD_TEST_REQUIRE_SANDBOX=1 just test-server` run passed all 78 tests with PostgreSQL/pgmq and SeaweedFS available, including the required-mode Pandoc worker job. The services were throwaway containers owned by this worktree and removed by the recipe. The unavailable-confinement helper test also passed with `ARIAD_TEST_SANDBOX_SUPPORT=none ASHIFT_SANDBOX=off`, confirming that ambient settings cannot weaken required mode.

## Serialized verification on October 24

The coordinator stopped the overloaded ASUS run at 22:48 on October 23 and killed its child processes. The interrupted full-CI output was not used as success evidence. On resumption, the branch rebased onto `dev@7f05fde`, and every heavy command ran under the shared `.run/build.lock` with three Cargo jobs and three test threads.

Before the stop, a pre-existing HTML reconstruction timing test took 20.27 seconds against its 20-second bound while three workers were building. The isolated rerun under the shared lock passed in 7.86 seconds without changing its input, limit or assertions. The Docling image was also retried under the lock and still failed at the signed manifest's HTTP 404, with no substitute pack.

Fresh `ARIAD_TEST_REQUIRE_SANDBOX=1 just test-server` verification passed 78/78 tests in 38.62 seconds with real PostgreSQL/pgmq and SeaweedFS. The required-mode Pandoc job passed, and the recipe removed both containers. The separate unavailable-confinement helper test passed with ambient `ASHIFT_SANDBOX=off`. These are new results from the serialized run, rather than reused output from the stopped session.

The serialized `just ci` run then completed successfully: 712 workspace test entries, doctests, 64 benchmark tests, capabilities validation, four crate archive builds, TypeScript checks, 57 JavaScript tests and a production web build, in addition to static checks, Clippy and WASM. Database cases skip internally in the workspace run without service configuration; the real-service suite above covers them. Two PowerShell mutation checks skipped because `pwsh` was unavailable locally. The unchanged HTML reconstruction timing test passed in the full workspace suite in 9.76 seconds. The phase acceptance criteria and plan row were completed, with signed Docling publication retained as an explicit external dependency.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
