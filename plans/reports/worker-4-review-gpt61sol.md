# Cloud foundation review result

The review is complete with two verified high-severity merge blockers and no
confirmed critical finding. The detailed review is
[code-review-261025-phase4.md](code-review-261025-phase4.md), ending with
`Verdict: FIX_REQUIRED`.

## Files changed

- `plans/reports/code-review-261025-phase4.md`: findings, concrete failure
  scenarios, source locations, remedies and review limits.
- `plans/reports/worker-4-review-gpt61sol.md`: this delivery record.

No product code or configuration was edited.

## Work and commits

Reviewed all 67 changed files between `origin/dev` at `3b8528149bc6` and
`origin/feat/cloud-foundation` at `c27c506e2dc2`, including integration edits.
Read the ASUS briefs, development rules, repository Git workflow, relevant
architecture sections, copied worker reports and the completion plan/Phase 4
file found in the sibling `01-AriadShift-wt-p3-notes` worktree.

Delivery commit subject: `docs(review): record cloud foundation merge blockers`.
It contains both reports on `feat/cloud-foundation--review`. Resolve its hash:

```sh
git log -1 --format='%H %s' -- plans/reports/worker-4-review-gpt61sol.md
```

## Verification performed

Only source inspection and read-only checks were performed. The diff whitespace
check passed. SHA-256 values for the committed OpenAPI document and generator
configuration exactly match the SDK lock. Pinned upstream source was consulted
for Better Auth, pnpm deployment, Hono middleware, SeaweedFS and actionlint.
No build, test, install, workflow dispatch or container process was started.

## Coordinator verification

Run these sequentially in the integrated checkout after the two fixes:

```sh
just lint-workflows
pnpm install --frozen-lockfile
just openapi
git diff --exit-code -- schemas/openapi.v1.json
cargo test -p ariad-server --test openapi_drift
cargo test -p ariad-server --lib api::tests
pnpm --filter @ariadshift/auth typecheck
pnpm --filter @ariadshift/auth test
pnpm --filter @ariadshift/sdk typecheck
pnpm --filter @ariadshift/sdk test
just sdk-check
docker build -f services/auth/Dockerfile -t ariadshift-auth:local .
docker build -f infra/docker/web.Dockerfile -t ariadshift-web:local .
docker compose -f infra/compose/docker-compose.yml --profile core config -q
docker compose -f infra/compose/docker-compose.yml --profile core up -d --wait
curl -fsS http://localhost:3001/api/auth/ok
curl -fsS http://localhost:3001/api/auth/jwks
curl -fsS http://localhost:8080/readyz
just ci
```

Use a disposable validation database and operator-generated secrets. For the
sign-up → JWT → API-key round trip, follow the exact commands under "Verify"
in `/home/vchun/Codes/ariadshift-ops/reports/worker-4-t3-opus55.md`, replacing its
final two GET requests to `/v1/jobs` with GET requests to `/v1/jobs/<job-id>` for
a job created by the same user; the server has no list-jobs endpoint.

After images are published, configure the real owner/version and corrected
image names in `infra/compose/.env`, then validate the separate binary route:

```sh
docker compose -f infra/compose/docker-compose.yml --profile core pull
docker compose -f infra/compose/docker-compose.yml --profile core up -d --wait --no-build
```

## Open concerns

- Add the arm64 runner exception to actionlint configuration. The current
  workflow fails the mandatory static gate.
- Align the advertised server image with the workflow's published API image.
  The current published-image instructions cannot start the stack.
- Builds, auth migrations/JWT/API-key round trips, Office/Docling execution,
  registry signatures and SBOM contents remain for the coordinator's build queue.

Status: DONE_WITH_CONCERNS
