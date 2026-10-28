# Worker p5-t5 (Opus 5.5 High): API, auth and SDK docs

`docs/api.md` is now the full developer guide for the cloud API. I checked every example against the server source, `schemas/openapi.v1.json`, `services/auth` and `packages/sdk/src`. Nothing was built or run, as the brief requires. The only check I ran was `typos docs/api.md`, which passed.

Worktree `~/Codes/01-AriadShift-wt-p5-docs`, branch `feat/cloud-product--docs`.

## Files changed

- `docs/api.md` (rewritten and extended, 510 lines)
- `plans/reports/worker-5-t5-opus55.md` (this report)

There is no `docs/README.md` or docs index. Root `README.md` already links `docs/api.md`, so no index change was needed.

## Commits

- `2addff8 docs(api): document auth, job lifecycle, rate limits and the SDK`
- the commit that adds this report

## What the page now covers

- **Authentication.** How the API picks a credential (bearer first, then `x-api-key`, else anonymous) and the fact that a rejected credential is never treated as anonymous. Auth-service sign-up and sign-in with curl (`/api/auth/sign-up/email`, `/api/auth/sign-in/email`, a cookie jar and a trusted `Origin`), and the two-factor note. JWTs: `GET /api/auth/token`, the `set-auth-jwt` header, the claims, JWKS verification rules (kid, declared algorithm, required claims, 30 s leeway, 5 min cache, refetch on an unknown kid at most every 30 s), and `503` when the key set cannot be fetched. API keys: create, list and delete, the `ashift_` prefix, the key shown only once, and which refusal maps to which status. Anonymous callers and Turnstile. The throttle after 30 rejected credentials.
- **Converting a file.** The full curl flow, from upload to delete. The old example passed the presigned headers through word-splitting, which broke them; it now builds a bash array and streams the file with `--upload-file`.
- **Endpoints.** A table that now includes the presigned PUT and GET (`/v1/blobs/{token}` with the local store), plus general rules: the 1 MiB JSON body cap, unknown fields rejected (`deny_unknown_fields`), non-UUID ids return 404, the `x-request-id` header, and CORS.
- **Uploads, planning and capabilities, jobs, events, outputs and delete.** JSON examples use only real DTO fields: `Upload` with `upload` present only while pending and `format: null`, `PlanResponse`, `Job` with nullable `error`, `started_at` and `finished_at`, `JobEvent`, and `Outputs`. Each section lists the status codes its handler returns.
- **Errors.** The existing table, now also covering the responses without a JSON body (the `413` body-limit layer and the `408` timeout layer), XML errors from S3, and retry guidance.
- **Rate limits.** Fixed one-minute windows in PostgreSQL. A JWT and an API key of the same workspace share one window. `Retry-After` is the seconds until the window ends, at least 1, and is 5 for the event-stream caps. `quota_exceeded` has no `Retry-After`. The uncounted public routes.
- **Limits and quotas.** A summary table of the seeded plans (full table linked in server.md), the order of the job admission checks with the real violation messages, and the upload-admission message.
- **TypeScript SDK.** The package is a private workspace package, so you run `generate` first. Quick start in Node, a cached `bearer` provider for the web app, a helper table, the `events` reconnect behaviour, `AriadApiError` handling, and calling generated operations through `ariad.raw`.
- **Versioning and deprecation.** A placeholder heading, as the brief asks.
- The statement that the contract is generated from code and checked by a drift test is still there.

## Verify

```sh
cd ~/Codes/01-AriadShift-wt-p5-docs
git log --oneline origin/feat/cloud-foundation..HEAD
typos docs/api.md
just ci   # includes the Markdown checks
```

## Open concerns

- **Plan files not in this worktree.** `plans/261025-1651-ariadshift-v1-completion/` (`plan.md` and `phase-05-cloud-product.md`) does not exist in this worktree or anywhere on this box, so I worked from the brief alone. Please check the page against the phase file's acceptance criteria.
- **Better Auth details from docs, not code.** The sign-up and sign-in endpoint paths and bodies, `api-key/list`, `api-key/delete {keyId}`, and the create response's `key` field come from the Better Auth documentation (checked through Context7), not from code in this repository. The `Origin` header in the curl examples is a precaution for Better Auth's origin check on cookie requests. An end-to-end run against the compose stack should confirm them.
- **SDK names not generated here.** The generated SDK function names `plan` and `capabilities` are assumed from the operation ids. `client.ts` imports the other operations by exactly those names, but I did not run the generator.
- **Event kind `warning`.** The OpenAPI `JobEvent.kind` description and the SDK's `JobEventKind` include `warning`, but no code emits it. The page tells clients to ignore kinds they do not know. A later change could remove `warning` from the schema text or start emitting it.
- **ARCHITECTURE.md wording.** ARCHITECTURE.md §9 says "Clients are never hand-written", but the SDK has a hand-written layer over the generated operations. The owner of ARCHITECTURE.md may want to say "generated, with a thin hand-written helper layer".

Status: DONE_WITH_CONCERNS
