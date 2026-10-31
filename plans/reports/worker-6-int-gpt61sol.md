# Integration worker result

The requested branches are merged on `feat/v1-hardening--int`, and the in-scope
integration repairs are committed. The final pre-merge verdict is **FIX_REQUIRED**:
the shipped Better Auth service lacks the OAuth provider required for remote MCP.
That service is outside this worker's file ownership. See
[the final review](code-review-261025-phase6-final.md) for findings, evidence,
earlier-finding dispositions and the full verification commands.

Worktree: `/home/vchun/Codes/01-AriadShift-wt-p6-int`.
Initial head: `c39a6bb`. Base reviewed: `origin/feat/cloud-foundation` (`504a212`).

## Commits and files

| Commit | Work |
|---|---|
| `8526c5f` | Merge cloud foundation. |
| `873b3d1` | Merge limits, upload deletion and API freeze. |
| `5b21e49` | Merge remote MCP. |
| `0c5f037` | Merge account job listing. |
| `a858dc1` | Repair `Cargo.lock` for the server's rmcp HTTP transport dependency closure. |
| `166db04` | Align `packages/sdk/generator.lock.json` with the merged OpenAPI schema. |
| `57bb971` | Format `apps/desktop/os-integration/generate.mjs`; emitted artifacts remain unchanged. |
| This report commit | Add this result and `plans/reports/code-review-261025-phase6-final.md`. |

The merges bring the task branches' server source/tests, API/MCP docs, SDK source
and job-list test, OpenAPI document and snapshot into the existing hardening branch.
They were clean; no manual conflict resolution was needed. No branch was rebased,
no pushed history was rewritten, and only this task branch is authorized for push.

## What was verified

Rust workspace formatting, formatting of the nine phase-6 fuzz targets, workflow
lint, Biome across the reviewed desktop/SDK files, OS integration artifact drift,
schema/snapshot equality, local schema references and generator-lock hashes pass.
The Cargo lock was reconciled by hand using the locked rmcp manifest and registry
metadata; no Cargo dependency resolution or compilation was run. The three repair
commits pass the repository's commit linter.

OpenAPI generation requires compiling the server, so the existing hand-maintained
schema was checked against source rather than regenerated. Its three new REST
operations and component references agree with the merged routes. The SDK lock
now records that document. Exact generator equality still needs the coordinator.

No builds or tests were run, and no database or background service was started.
The allowed pnpm lint wrapper automatically installed its pinned dependencies;
it did not change tracked manifests or lockfiles.

## Commands for the coordinator

Use the pinned tool environment and a disposable PostgreSQL/pgmq test service;
set `ARIAD_TEST_DATABASE_URL` or database-backed tests will skip. Run sequentially:

```sh
cd /home/vchun/Codes/01-AriadShift-wt-p6-int
cargo metadata --locked --format-version 1 > /tmp/ariadshift-p6-metadata.json
just pandoc
just openapi
git diff --exit-code schemas/openapi.v1.json
cargo nextest run -p ariad-server --lib --test openapi_drift --test openapi_v1_snapshot
just test-server
pnpm --filter @ariadshift/sdk generate
pnpm --filter @ariadshift/sdk check
pnpm --filter @ariadshift/sdk typecheck
pnpm --filter @ariadshift/sdk test
just desktop-bindings-check
just licenses-check
just ci
just fuzz uri_parsers 60
just fuzz archive_preflight 60
just fuzz docx_meta 60
```

The review also records native-platform and actual OAuth acceptance checks.
Regenerated schemas, baselines, locks and notices require review before committing
any differences. Do not overwrite the frozen baseline to hide a breaking change.

## Open concerns

- High: actual OAuth cannot complete with the shipped auth service. The remote
  tests use a test issuer, not a real provider flow. An auth-service worker must
  implement the provider and supply MCP Inspector acceptance evidence.
- Medium: broad Seatbelt read grants, exact Host compatibility, composite-schema
  freeze false positives, stale architecture sections, and unrun runtime/platform
  and license-inventory gates remain documented in the final review.
- Full fuzz-workspace formatting fails in unchanged `pdf_reader.rs:32`, outside
  this brief's ownership. The phase-6 commit range fails commit lint on the old
  91-character subject of `9734cfc`; broader merged history also has long body
  lines. None of those pushed commits was rewritten.
- Full-range whitespace inspection flags the intentional H2 regression seed's
  trailing space. It is retained to exercise the repaired oracle.

## Unresolved question

Which auth-service owner will supply the missing OAuth provider and real-client
acceptance evidence before the integrated branch is merged?

Status: BLOCKED
