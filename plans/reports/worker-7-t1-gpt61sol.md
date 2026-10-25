# Security audit worker result

The early read-only audit is complete. It found one high and two medium issues; none was fixed because this task owns documentation only. Release acceptance remains open.

## Files changed

- [Security audit](security-audit-261025-v1.md): snapshot hashes for every requested branch, severity-ranked findings, file/line evidence, concrete failure scenarios, owning areas, recommended fixes, coverage and coordinator verification.
- [Threat model](../../docs/security/threat-model.md): assets, actors, trust boundaries, verified controls, deployment assumptions and residual risks.
- This result file, `plans/reports/worker-7-t1-gpt61sol.md`.

No source code, manifests, lockfiles, workflow files or plan-status files were modified. No build, test, dependency audit, container or dev server was started.

## Findings and required follow-up

| Severity | Finding | Owner |
|---|---|---|
| High | One valid local-store PUT URL can be replayed into unlimited concurrent incomplete buffered uploads, exhausting API memory. The S3 compose path does not execute this local handler. | Server blob/storage owner |
| Medium | Local MCP sidecar image reads reopen an ambient parent path after confinement and permit hidden image paths. A shared-directory swap can disclose a private raster image. | Host assets/local MCP owner |
| Medium | A matching JWKS key remains trusted indefinitely during refresh failure, so emergency key removal does not bound acceptance on an isolated replica. This requires prior signing-key compromise and an identity-service outage. | Server auth owner |

The report distinguishes verified failures from existing decisions: anonymous IDs are shareable capabilities; S3 ingress limits are an operator requirement; older Linux ABIs have documented IPC gaps; email verification is deliberately absent. No speculative OAuth takeover or fork-cache-poisoning finding was added.

Remote MCP exists and was reviewed at `origin/feat/v1-hardening--mcp`. Its API-key path, audience separation, Host/Origin validation and bounded upload adapters were inspected. Real OAuth token issuance/discovery remains an integration requirement. The final cloud web UI is not in this checkout and needs a review after integration. T2 owns dependency advisories, and T3 owns CSP/header implementation and Lighthouse verification.

## Commits and delivery

- `85dcee0`: `docs(security): record threat model and source review`.
- This handoff is committed separately as `docs: record security review handoff`; retrieve its exact hash with `git log -1 --format='%H %s' -- plans/reports/worker-7-t1-gpt61sol.md`.
- Authorized push target: `origin/fix/v1-security-audit--audit` only. No merge or promotion is performed.

## Verification performed

Read the common ASUS brief, task brief, project instructions, Git workflow, relevant architecture sections, completion/security plans from `origin/dev`, source, tests and the requested branch diffs. Consulted current primary Better Auth and GitHub cache-isolation documentation. Checked pinned object_store buffering behavior against the installed registry source. Checked tracked dotenv names and high-confidence credential patterns without printing credentials. Reviewed the staged documentation diff and ran `git diff --cached --check` successfully before the audit commit.

One attempted dependency-directory inspection was blocked by the configured context ignore policy. No exemption was added. That did not block the audit: the relevant buffer behavior was read from the Rust registry source, while auth library guidance was checked against primary documentation. No skill script failed.

## Coordinator commands

Run these sequentially in the integrated worktree through the shared build queue. Source the coordinator machine's pinned environment first. Add the regression cases specified in the audit before treating these existing suites as closure evidence.

```sh
just pandoc
cargo nextest run -p ariad-host --test confine --test assets --test packs
cargo nextest run -p ariad-cli --test mcp_stdio
just test-server
pnpm --filter @ariadshift/auth test
pnpm --filter @ariadshift/auth typecheck
just ci
just desktop-ci
just web-e2e
just web-lighthouse
just deny
pnpm audit --prod
uv run --project engines/docling pip-audit
just lint-commits origin/feat/cloud-product-base..HEAD
```

Run `cargo nextest run -p ariad-host --test sandbox_macos` on the Mac after integrating the Seatbelt branch. Run required-mode sandbox/container checks on ASUS; a skipped Docker integration command is not a pass. Use a disposable bounded process for upload-exhaustion verification. No heavy verification was run by this worker.

## Open concerns

All three findings remain open. The high finding blocks security acceptance until repaired and verified. The coordinator still needs dependency-audit results, integrated cloud/OAuth coverage, real sandbox execution and the final integration audit. No clarification from the user is required for this completed read-only assignment.

Status: DONE_WITH_CONCERNS
