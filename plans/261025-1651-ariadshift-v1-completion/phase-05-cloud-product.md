---
phase: 5
title: "Cloud product: web cloud mode, jobs, compose E2E"
status: pending
depends_on: [4]
branch: feat/cloud-product
---

# Phase 5: Cloud product

The second half of roadmap phase 10.

## Tasks

| Task | Owner (model) | Files owned | Work |
|---|---|---|---|
| T1 | Opus 5.5 High (first; read-only) | `plans/reports/rea-261025-cloud-converter-ux.md` | REA study (`reverse-engineer-anything`, `mcp__rea__capture_browser_scenario`, `inspect_web_page`) of CloudConvert's job UX: upload, destination disclosure, progress, download/delete, errors. It produces UX requirements for T2/T3. |
| T2 | Opus 5.5 High | `apps/web/src/components/cloud/**`, `apps/web/src/lib/cloud/**`, the cloud entry point in `apps/web/src/app/app/**` | Cloud mode in `/app`: an explicit destination choice, Uppy `@uppy/aws-s3` presigned PUT, jobs through the SDK, SSE progress, download, "Delete now" and "Delete after download", anonymous small-file mode with Turnstile, and the phase 7 "Process in Cloud" hand-off. |
| T3 | Opus 5.5 High | `apps/web/src/app/(account)/jobs/**` | Jobs page: list, status, retry, download and delete through the SDK. |
| T4 | Opus 5.5 High | `.github/workflows/cloud-e2e.yml` (new), `infra/test/e2e/**` (new), cloud paths in `scripts/ci-scope.sh` and `scripts/test-ci-scope.sh` | Compose E2E with all three profiles: sign up through the API, upload a scanned PDF, convert to DOCX on the docling worker, download and check, then a quota refusal. Triggered by path, by label, and nightly. |
| T5 | Opus 5.5 High | `docs/api.md` (new), the cloud section of `docs/web.md` | Auth, API keys, SDK quick start, job lifecycle, and the cloud-mode privacy statement. |

## Build and test pass (coordinator, sequential)

```sh
source /Volumes/AriadWork/02-Ariadnev-Eco/.cache/env.sh
just web-ci && just web-e2e
bash infra/test/e2e/run.sh        # the same script CI runs, against local compose (core, office, docling)
just web-lighthouse               # / and /app ≥ 90
just lint-workflows && just test-ci-scope
```

## Acceptance

- [ ] Locally, scanned PDF → DOCX succeeds through both the web cloud mode and the SDK against compose; the quota refusal shows in the UI.
- [ ] The E2E script passes locally and the workflow is committed.
- [ ] Lighthouse is at least 90 on `/` and `/app`. Roadmap phase 10 row is Done.
