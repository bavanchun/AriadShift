# Hardening integration repair

Outcome: join current `origin/dev` with the accepted hardening integration,
repair all four whole-integration findings, and verify the exact delivery head.
The worker brief is `/home/vchun/Codes/ariadshift-ops/briefs/ws-3b-integrate-4.md`.

Constraints: retain both parents' behavior, IR semantic validation, upload abuse
accounting, the global ceiling of 64 and pending production capacity approval.
Use local conventional commits, no rebase, push or PR. Do not touch other
workstreams, persistent databases or foreign services. All compiling gates run
detached through the shared gate queue with four Cargo jobs and nice 19;
`just test-server` runs without an outer queue wrapper.

Non-goals: capacity tuning, unrelated Compose/storage operations and deployment.

## Steps and acceptance

1. Merge fetched dev `3c7f058` without fast-forward. Resolve all four conflicts
   cumulatively and regenerate OpenAPI and the SDK input lock.
2. Prove the versioned fuzz compile error, inline MCP retry regression against
   disposable PostgreSQL, and rendered Compose challenge mismatch before fixes.
3. Handle typed IR validation errors with an invariant; return a reusable upload reference
   on inline job admission refusal; retire unusable automatic inputs; map auth's
   shared challenge secret and verify both configured/local signup policy.
4. Update owning server, API and self-hosting docs and retain executable
   regressions. Review the cumulative patch against both parent contracts.
   Follow the lead's additional request to sample the original oversized MCP
   test 20 times on each integration head, diagnose its early-response race,
   retain all size assertions and verify the corrected client 20 times.
5. Commit the report before the full combined gate, then verify that exact head:
   real PostgreSQL server/auth suites, workspace, all 18 default fuzz campaigns,
   static/licences/workflow guards, JS/web/desktop, regeneration, commit lint and
   typos. Copy the completed report with its exact SHA/log evidence to operations.

Merge, repairs, focused regressions and the postmerge continuation have passed.
The exact final committed head still requires the complete gate before delivery.
Progress and evidence are recorded in [the result](reports/ws-3b-integrate.md).
A failed gate stops delivery until repaired and rerun. Roll back an unaccepted
fix with a new scoped revert; preserve the merge history.
