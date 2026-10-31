---
title: Repair optional server database test modes
date: 2026-10-30
summary: Keep Verify database-optional and require real PostgreSQL in Server gates.
---

# Repair optional server database test modes

## What happened

The inline MCP recovery regression panicked without a database. The old combined gate exported a real URL and hid the failure. Read-only reproduction found the same unconditional requirement in queue fairness; a database-free server run then exposed global admission and worker startup tests with the same defect.

## Decision

Use the existing optional shared harness. Treat ARIAD_TEST_REQUIRE_DATABASE=1 as a hard requirement for a nonblank database URL, and set it in the Server job and local test-server recipe. Retain every recovery, isolation, abuse-accounting, admission, fairness and startup assertion. Stop the temporary OAuth issuer when its test skips.

## Verification and next steps

Source commit 7bc656e1010a4bee9c31f2ae4ceb01c42168c22e passed all 234 server tests without database configuration and all 234 against throwaway PostgreSQL/S3. Missing and blank URLs fail in required mode. Clippy and the CI, workflow, formatting, spelling and commit guards passed. Both owned containers were removed. Exact commands and logs are recorded in the [result report](../reports/ws-3b-integrate.md). No fuzz or JavaScript suite rerun was needed for the test-harness-only change. Lead acceptance is required before any push. AgentWiki publish skipped.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
