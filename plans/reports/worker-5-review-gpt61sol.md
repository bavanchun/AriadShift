# Cloud product review result

The review is complete. The web and account/pages diffs need fixes; the compose E2E and API documentation diffs are mergeable within the requested critical/high review scope. The [detailed review](code-review-261025-phase5.md) records six high findings, exact revisions, file locations, concrete scenarios and required fixes. No critical finding was found.

## Files changed

- `plans/reports/code-review-261025-phase5.md` contains the review evidence and one verdict per diff.
- `plans/reports/worker-5-review-gpt61sol.md` contains this handoff.

No code, tests, configuration, dependencies or other branches were changed. No background process was started.

## Commits and delivery

- `fdb0cb3` — `docs(review): record cloud product merge blockers`.
- This result is committed separately as `docs(plans): record cloud product review handoff`. Its hash can be read with `git log -1 --format='%h %s' -- plans/reports/worker-5-review-gpt61sol.md`.
- Delivery branch: `feat/cloud-product--review`. The authorized publication command is `git push -u origin feat/cloud-product--review`; no other branch is a push target.

## What was done

Reviewed both UI diffs against `origin/feat/cloud-product-base`, and the E2E/docs diffs against `origin/feat/cloud-foundation`, after fetching origin. Read the completion plan and cloud-product phase from `origin/dev` because this worktree lacks those files. Followed the `ak-code-review` skill, project architecture and Git workflow, and the ASUS prohibition on builds/tests. Checked current Uppy and Better Auth documentation for the external API/security assumptions used in the review.

Compared the upload, auth, job, output, deletion and quota calls with the SDK, auth service, server implementation, OpenAPI document and compose wiring. Inspected the added tests for assumptions that could conceal integration failures. Git whitespace checks passed for the report changes; no build, test, browser verification, package installation or compose startup was performed.

## Coordinator verification

Run these on the integrated delivery branch after repairing the findings. Keep the global build queue sequential. The frozen installation first verifies that the dependency repairs and lockfile agree.

```sh
pnpm install --frozen-lockfile
just sdk-generate
just web-typecheck
pnpm exec vitest run \
  apps/web/test/cloud-auth.test.ts \
  apps/web/test/cloud-lib.test.ts \
  apps/web/test/cloud-job.test.ts \
  apps/web/test/cloud-panel.test.tsx \
  apps/web/test/cloud-converter-app.test.tsx \
  apps/web/test/account-auth-api.test.ts \
  apps/web/test/account-helpers.test.ts \
  apps/web/test/account-pages.test.tsx \
  apps/web/test/account-jobs.test.tsx
just web-ci
just web-e2e

# Exercise the cloud browser suites against a matching configured build.
NEXT_PUBLIC_ARIAD_API_URL=http://localhost:3902 \
  NEXT_PUBLIC_ARIAD_AUTH_URL=http://localhost:3901 just web-build
NEXT_PUBLIC_ARIAD_API_URL=http://localhost:3902 \
  NEXT_PUBLIC_ARIAD_AUTH_URL=http://localhost:3901 \
  pnpm --filter @ariadshift/web exec playwright test \
  e2e/cloud.spec.ts e2e/account.spec.ts

# Verify local-only static export, with no cloud variables inherited.
env -u NEXT_PUBLIC_ARIAD_API_URL -u NEXT_PUBLIC_ARIAD_AUTH_URL \
  -u NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY just web-export

# The fresh image build also verifies SDK generation in an isolated checkout.
bash infra/test/e2e/run.sh
just lint-workflows
just test-ci-scope
just web-lighthouse
```

Also execute the detailed report's same-origin Cache Storage scenario, deletion-failure/retry scenarios, and fresh-browser converter-to-jobs journey. Add regression coverage for those scenarios before the coordinator reruns the affected suites. The existing mocked tests alone do not establish those properties. Check real Turnstile on the cloud build when a site key is configured; the mock replaces its iframe and cannot establish compatibility with `/app` isolation headers.

## Open concerns

- The web manifest lacks a matching lockfile, and standalone web export/image builds do not generate the SDK client.
- The account routes import undeclared Better Auth dependencies, and actual converter jobs never enter the history consumed by `/jobs`.
- A same-origin auth deployment can persist JWT-bearing responses through the offline cache. Failed server deletion is silently abandoned by start-over.
- Runtime behavior remains unexecuted by this read-only worker. The compose E2E also depends on the existing published signed Docling pack and the documented Linux runtime requirements.

No unresolved question requires user input. Implementation fixes belong to the owners/coordinator; this worker's authorized scope is the two reports only.

Status: DONE_WITH_CONCERNS
