---
title: Repair joined hardening integration
date: 2026-10-30
summary: "Join current dev and repair fuzz validation, reusable inline input and signup challenge wiring."
---

# Repair joined hardening integration

## What happened

Merged current dev without rebasing and retained the digest admission, transfer
retry budget, upload cleanup and SDK account/job helpers from both parents.
The joined versioned fuzz target failed compilation on the semantic validation
error added by dev. Real PostgreSQL reproduced an inline conversion refusal
that hid its completed upload and consumed the daily allowance on retries.
Rendered Compose reproduced the missing auth challenge secret.

## Decision

Keep the IR normalization oracle and validate the typed semantic errors.
Return the reusable upload id on inline quota refusal; callers retry with the
reference, while other automatic job-input failures retire their uploads.
Keep retired/abandoned upload accounting unchanged. Map auth's secret from the
shared Turnstile setting, and document the unchanged fleet ceiling of 64.

## Verification and next steps

The focused fuzz, PostgreSQL admission/recovery, configured and local signup,
rendered Compose and SDK checks passed. Full exact-head validation and the
local commit identities are recorded in the integration result report.
The first full gate found a stale frozen OpenAPI description inherited from
dev. Its guarded refresh passed all 35 compatibility tests with and without
update mode; the final full gate restarts on the refreshed snapshot.
The next run found two dev semantic IR seeds absent from the hardening
manifest; registering their original hashes and provenance passed the fixture
validator. Web notice checks also required regenerating the old ignored
Pandoc source notice and build outputs after merging dev.
The continuation then passed web tests/build/export, static licences and
workflow guards, Clippy, WASM, all 75 benchmarks, packages, desktop and
regeneration. The final full gate will validate the committed delivery head,
including all default fuzz campaigns and real PostgreSQL suites.
No push or pull request is authorized before lead review.

## MCP response timing follow-up

The full gate exposed a body-write race in the existing oversized MCP test.
At the lead's request, isolated trials passed 20/20 on the previous integration
and 20/20 on the merged head; the tested function and middleware were unchanged.
The server legitimately rejects an oversized Content-Length before reading
the body, while the eager client can lose that response to its write error.
The test now sends the declared headers and observes 413 before sending a body;
the chunked case still requires 413 after actual overflow. The corrected test
passed 20/20 trials. Full gate evidence remains in the result report.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
