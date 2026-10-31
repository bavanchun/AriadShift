# Worker p6-jobs (Opus 5.5): GET /v1/jobs lists the caller's jobs

Worktree `~/Codes/01-AriadShift-wt-p6-jobs`, branch `feat/v1-hardening--jobs` (based on `origin/feat/v1-hardening--api`). Following the ASUS rules, I did not build, test, check or lint anything with cargo or pnpm. I ran only `cargo fmt -p ariad-server` and `typos`, and both pass. I wrote the OpenAPI document and the snapshot additions by hand from the utoipa 6.0 / utoipa-gen 6.0.1 sources, so they need the coordinator's regeneration pass (see Verify).

## Outcome

- `GET /v1/jobs` (`operationId: list_jobs`, tag `jobs`, security `bearer` or `api_key`) returns the jobs of the caller's account workspace, newest first. Order is `created_at DESC, id DESC`.
- **Anonymous callers get `401 unauthorized`.** This is checked before the query is parsed, so anonymous jobs (`ANONYMOUS_WORKSPACE`) are never listed. Rejected credentials also get `401`, as on every other route.
- **Query parameters:**
  - `limit`: 1 to 100, default 50. Out-of-range or non-numeric values get `422 invalid_request`; the server does not clamp them.
  - `cursor`: opaque. URL-safe base64 of the last row's `created_at` (microseconds) and `id`. A malformed cursor gets `422`.
  - `status`: an optional `JobStatus` filter. An unknown value gets `422`.
- **Response:** `JobList { jobs: JobListItem[] (maxItems 100), next_cursor: string | null }`. `next_cursor` is always present, and `null` on the last page.
  - `JobListItem` is the `GET /v1/jobs/{id}` `Job`, flattened (`allOf: [Job, {filename}]`), plus `filename`.
  - **Stability:** paging is keyset paging, so jobs created while a client pages through the list never shift or repeat later pages. Ties on `created_at` are broken by `id`.
  - **Expired and deleted jobs:** listed until the 30-day metadata prune.
- **Rate limits** apply through the shared `Caller` extractor (the plan's `requests_per_minute`).
- **No N+1 queries.** A page uses one jobs query plus one `job_steps` query (`job_id = ANY($1)`). `job_dto` was split so that both routes build the same `Job` through `job_from_row`.
- **No migration.** The existing `jobs_workspace_created_idx (workspace_id, created_at DESC)` serves both the workspace filter and the order. The `(created_at, id) < (...)` bound narrows that index on `created_at`.
- **SDK:** `listJobs({ limit?, cursor?, status?, signal? }): Promise<JobList>` in `packages/sdk/src/client.ts`, through the generated `listJobs` operation.
- **Docs:** a `docs/api.md` endpoints row and a "Listing jobs" subsection.

## Differences from the web worker's contract (`worker-5-fix-pages-opus55.md`, `jobs-api.ts`)

Everything else matches: path, operationId, tag, security, limit bounds and default, cursor semantics, order, the workspace scope, the inclusion of expired jobs, `maxItems 100`, `next_cursor: string | null`, `401`/`422`/`429`/`500`, and the SDK signature. The differences:

1. **`filename` is optional, not required.** It is present while the upload keeps the name. Retiring an upload (`DELETE`, retention) blanks `uploads.filename`, so for those jobs the property is omitted rather than sent as `""`.
   - An empty string would bypass the page's `job.filename ?? names.get(job.id) ?? "Job …"` fallback in `jobs-panel.tsx` (line 194) and show a blank name.
   - The client already types it as `filename?: string`, so no web change is needed.
2. **Additive `status` query parameter.** The web client does not send it.
3. **A documented `503` response** ("Identity verification is unavailable"), like every other authenticated route. The page's generic error alert already covers it.

## Commits

| Commit | Subject |
|---|---|
| `d7eba58` | `feat(server): list the caller's jobs with GET /v1/jobs` |
| `7cb7332` | `feat(sdk): wrap GET /v1/jobs as listJobs` |
| (this report) | `docs(plans): record the job list worker result` |

## Files changed

| File | Change |
|---|---|
| `crates/ariad-server/src/api/job_list.rs` (new) | `list_jobs` handler, `ListJobsParams` (IntoParams), `JobList`, `JobListItem`, the cursor codec, and unit tests (cursor round trip, malformed cursors, `filename` omission, OpenAPI shape) |
| `crates/ariad-server/src/api/jobs.rs` | `job_dto` now delegates to the pure `job_from_row(row, steps)` |
| `crates/ariad-server/src/api/mod.rs` | `pub mod job_list;` and `routes!(job_list::list_jobs, jobs::create_job)` |
| `crates/ariad-server/src/db.rs` | `ListedJobRow`, `JobListQuery`, `list_jobs` (two static keyset queries), `JobStepRow`, `steps_of_jobs` |
| `crates/ariad-server/tests/job_list.rs` (new) | Integration tests (below) |
| `schemas/openapi.v1.json`, `crates/ariad-server/tests/snapshots/openapi_v1.json` | `get` on `/v1/jobs`; `JobList` and `JobListItem` schemas (purely additive, and the two files stay identical) |
| `packages/sdk/src/client.ts` | `ListJobsOptions` and `listJobs` |
| `packages/sdk/test/list-jobs.test.ts` (new) | Request shape, credentials, cursor pass-through, and `401` as `AriadApiError`. **This file is outside my listed files.** It is a new test for the wrapper I own, so it cannot conflict with another branch; drop it if you prefer |
| `docs/api.md` | Endpoints row and the "Listing jobs" subsection |

### Integration tests (`tests/job_list.rs`, need `ARIAD_TEST_DATABASE_URL`)

- `job_lists_show_only_the_callers_jobs`:
  - **Isolation:** Alice (JWT and API key) sees exactly her jobs, newest first, with filenames. Bob sees only his.
  - **Representation:** each listed item equals `GET /v1/jobs/{id}` plus `filename`.
  - **Refusals:** anonymous callers get `401`, also with an invalid `limit`, and a bad bearer token gets `401`.
  - **Empty account:** a new account gets an empty list with a `null` cursor.
  - **Deleted jobs:** a deleted job stays listed as `expired` without `filename`.
- `job_list_pages_stay_stable_across_inserts`:
  - A job inserted between pages does not shift pages 2 and 3 and appears on a fresh first page.
  - With every `created_at` tied, paging at limits 1, 2, 4, 6 and 7 returns each job exactly once, in `id DESC` order.
- `job_list_limits_are_bounded`:
  - With 120 jobs, the default page has 50 items, `limit=100` returns 100, and following the cursor yields all 120.
  - `limit` values `0`, `101`, `-1` and `ten`, the empty, garbage and separator-less cursors, and an unknown `status` each get `422 invalid_request`.
- `job_lists_filter_by_status`: `failed`, `queued`, `succeeded` and `canceled` filters, the filter combined with cursor paging, and no leak of another account's failed job.
- `job_lists_are_rate_limited`: with the free plan at `requests_per_minute = 2`, the third list call gets `429 rate_limited` with `retry-after`.

## Verify (coordinator)

```sh
# Regenerate the hand-written OpenAPI document; it should not change.
# If it does, review the diff (it should stay additive), commit it, and copy it to the snapshot.
just openapi && git diff --exit-code schemas/openapi.v1.json
ARIAD_UPDATE_OPENAPI_BASELINE=1 cargo nextest run -p ariad-server --test openapi_v1_snapshot
cmp schemas/openapi.v1.json crates/ariad-server/tests/snapshots/openapi_v1.json

cargo clippy -p ariad-server --all-targets -- -D warnings
cargo nextest run -p ariad-server --lib
cargo nextest run -p ariad-server --test openapi_drift --test openapi_v1_snapshot
ARIAD_TEST_DATABASE_URL=postgres://… cargo nextest run -p ariad-server --test job_list --test auth --test jobs

# The SDK lock records the spec hash, so it must be refreshed (packages/sdk/generator.lock.json is not my file).
pnpm --filter @ariadshift/sdk lock
pnpm --filter @ariadshift/sdk generate
pnpm --filter @ariadshift/sdk typecheck
pnpm --filter @ariadshift/sdk exec vitest run test/list-jobs.test.ts test/client.test.ts
pnpm exec biome check packages/sdk
just sdk-check
```

Once this branch and `feat/cloud-product--fix-pages` are integrated, the web page's API path can be verified with `pnpm --filter @ariadshift/web exec vitest run test/account-jobs-api.test.ts test/account-jobs.test.tsx`.

## Open concerns

- **`packages/sdk/generator.lock.json` needs `pnpm --filter @ariadshift/sdk lock`.** `just sdk-check` fails until then, because the spec hash changed. The file is not mine.
- **The OpenAPI JSON is hand-written.** I matched utoipa 6's serializer field order, the `allOf` layout for `#[serde(flatten)]`, non-nullable `Option` query parameters, and the doc-comment line joins. Still, `openapi_drift` compares byte for byte, so the generated output may differ in some detail. `just openapi` settles it.
- **Not compiled or run.** These APIs were checked against the local crate sources but not exercised:
  - axum 0.8.9 `Query::try_from_uri`;
  - sqlx 0.9 `#[sqlx(flatten)]` and `ANY($1)` with a `&[Uuid]` binding;
  - utoipa-gen 6 `#[param(default = json!(50))]` and `RefOr<Parameter>`.

  The SDK's `query` omits keys the caller did not pass. A caller that passes an explicit `undefined` relies on hey-api's serializer skipping undefined values.
- **CORS** already allows `GET` with `authorization` and `x-api-key` from configured origins; nothing changed there.

Status: DONE_WITH_CONCERNS
