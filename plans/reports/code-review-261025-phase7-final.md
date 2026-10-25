# Phase 7 final pre-merge security review

Review of `git diff origin/feat/cloud-product...HEAD` on `fix/v1-security-audit`, after merging the audit, CSP, dependency, MCP image and server fix branches. Reviewed 2026-10-25 (Asia/Saigon), with a security focus, by reading source only.

**Result:** the two High findings from this review are fixed in focused commits with regression tests, and the three audit findings are fixed in code. **No critical or high finding remains open in the code.** None of these fixes has been compiled or tested yet, because the brief forbade builds and the coordinator then paused all heavy commands. The merge gate stays closed until the coordinator's build and test queue passes (see "Verification").

## Scope

| Source | Content | How it entered |
|---|---|---|
| `--audit` | Threat model and audit report (SEC-01 High, SEC-02 and SEC-03 Medium) | merge `bd41cea` |
| `--csp` | Strict CSP and security headers from `apps/web/route-headers.mjs`, generated nginx config | merge `d81fbe8`; conflict in `infra/docker/web.Dockerfile` resolved |
| `--deps` | `js-yaml` and `body-parser` overrides, pillow 13.0.0, dated advisory exceptions | merge `4e6eb2c` |
| `--mcp-assets` | SEC-02 fix (`7836169`), plus the v1 completion plan files from `dev` | merge `484457b` |
| `--server` | SEC-01 and SEC-03 fixes (`f9deea5`, `0136cd9`) | merge `7ebad35`, see below |

**`--server` was not already present.** Its fixes were merged into `feat/cloud-foundation` (`9c4fb6d`), as the brief says. However, `feat/cloud-product` was cut before that merge (`git merge-base --is-ancestor 9c4fb6d origin/feat/cloud-product` fails), so this branch did not contain them. Phase 5 has not touched `crates/ariad-server` since the server branch's base (`c27c506e`), and the merge was clean. I merged the same commits instead of skipping them, because SEC-01 is High and would otherwise stay open on the phase 7 branch. When `feat/cloud-foundation` meets this branch later, the commits are identical and will not conflict.

**Dockerfile conflict.** Phase 5 added the Turnstile and operator build arguments; the CSP branch replaced the inline nginx config with the generated one and added `ARIADSHIFT_WEB_STORAGE_ORIGINS`. The resolution keeps all four build arguments and the generated nginx config.

## Findings from this review

| # | Severity | Finding | Status |
|---|---|---|---|
| R1 | High | The CSP blocks the account pages' calls to the cloud services | Fixed in `be8da76` |
| R2 | High | Four slow connections could hold every local blob transfer slot | Fixed in `90787ce` |
| R3 | Medium | Stale OpenAPI document and SDK lock after the new blob responses | Fixed in `f8b0347` |
| R4 | Medium | Transfer deadlines cut off large legitimate local transfers | Open |
| R5 | Low | Upload completion hashing has no in-flight guard | Open |
| R6 | Low | Windows-only gaps in the MCP image confinement | Open |
| R7 | Low | `convert_confined` still resolves images by ambient path | Open |
| R8 | Info | Turnstile inside the cross-origin-isolated `/app` needs a browser check | Open, needs runtime evidence |
| R9 | Info | `docs/mcp.md` does not describe the new image rule | Open, file outside this task's ownership |

### R1 (High, fixed): the CSP blocked the account pages

The CSP branch predates phase 5's account pages. It gave the cloud origins (API, auth service, object store) to the converter only, so every other page got `connect-src 'self'`. `/sign-in`, `/account`, `/api-keys` and `/jobs` call the auth service through Better Auth's client (`apps/web/src/components/account/auth-api.ts`), call `/v1` through the SDK, and fetch presigned output URLs to download them (`job-row.tsx` → `jobs-api.ts`). In a cloud build, the browser would have refused every one of these requests, so nobody could sign in, manage API keys or list jobs.

The fix (`be8da76`) adds an `account` profile to `apps/web/route-headers.mjs`. It is the site policy plus the cloud origins in `connect-src`. Account pages get no `'wasm-unsafe-eval'`, no workers and no Turnstile. `next start` and the static export (Netlify `_headers` and nginx) both apply it. A header test fails when `accountPaths` drifts from the pages under `src/app/(account)`. Regression tests cover the profile, both header tables, and every path form (`/jobs`, `/jobs.html`, `/jobs/`, `/jobs/index.html`). `docs/web.md` describes the third profile. `pnpm biome check` passes on both changed files.

### R2 (High, fixed): slow transfers could exhaust the blob budget

`f9deea5` fixed SEC-01's memory exhaustion, but it introduced a cheaper denial of service on `file://` storage:

- Uploads and downloads shared a fixed limit of four transfers per process (`BlobTransferLimits::default`), and `app.rs` never configured another value.
- The 15-second idle limit reset on every chunk.
- GET had no per-key or per-tenant limit.

As a result, four connections each trickling one byte every 14 seconds, or one replayed GET URL opened four times, held every slot until the URL expired (up to 15 minutes for PUT, 5 for GET). Every other upload and download on that process got `429`.

The fix (`90787ce`) does three things:

1. **Configurable, higher limits.** The default is now 64 transfers per process (`ARIAD_BLOB_MAX_TRANSFERS`), read by `ApiConfig::from_env` and applied in `app.rs`.
2. **A per-workspace share.** One workspace may hold at most 8 transfers at once (`ARIAD_BLOB_MAX_TRANSFERS_PER_WORKSPACE`). The workspace is read from the `t/{tenant}/…` storage key, and uploads and downloads count together.
3. **A minimum average rate.** After a 30-second grace period, a transfer must average at least 16 KiB/s (`ARIAD_BLOB_MIN_RATE_BPS`) or it ends like an idle one. Holding a slot now costs real bandwidth.

None of the three settings may be zero. Regression tests:

- `one_workspace_cannot_take_the_whole_transfer_budget`
- `trickled_upload_falls_below_the_rate_floor_and_releases_admission`
- `slowly_read_download_falls_below_the_rate_floor_and_releases_admission`
- `rate_floor_and_workspace_budget_cannot_be_zero`
- `blob_transfer_limits_read_positive_overrides` (config)
- `transfer_budgets_find_the_workspace_in_object_keys` (key parsing)

`docs/server.md` documents the variables.

The fix does not cover every case:

- Every anonymous caller shares one workspace, so anonymous callers together can still use up the anonymous share (8 slots). They cannot touch signed-in users' slots.
- Free sign-up has no email verification, so an attacker with many accounts still needs about 16 KiB/s per held slot.
- Deployments with several replicas still need ingress-wide limits, which `docs/server.md` already requires.

### R3 (Medium, fixed): OpenAPI drift

`f9deea5` added `408` and `429` responses to `/v1/blobs/{token}`, but it did not regenerate `schemas/openapi.v1.json` or update `packages/sdk/generator.lock.json`. That would fail `openapi_drift` and `just sdk-check`.

`f8b0347` adds the four responses to the committed document by hand. The file is serde_json pretty output, which Python's `json.dumps(indent=2)` reproduces byte for byte, and the response maps are sorted. The commit also updates the lock's `spec_sha256` and names the per-workspace share in the `429` descriptions.

If the drift test still disagrees, run `just openapi` followed by `pnpm --filter @ariadshift/sdk lock`.

### R4 (Medium, open): fixed transfer deadlines

A transfer must finish within the shorter of 15 minutes from admission and the signed URL's remaining lifetime. That lifetime is 15 minutes from issue for PUT and 5 minutes for GET. On `file://` storage, the free plan's 2 GiB upload therefore needs about 2.4 MB/s from the moment the URL is issued. Before `f9deea5`, expiry only gated the start of a transfer.

This is a product regression, not a security hole, and it is not High. The audit explicitly asked for transfers to stop after URL expiry. Possible fixes:

- Size-aware URL lifetimes.
- A deadline of a base time plus size divided by the minimum rate.

Either fix needs a product decision, because it changes the URL lifetime contract. The compose stack uses S3 and is unaffected.

### R5–R7 (Low, open)

- **R5:** `complete_upload` re-hashes the stored object without taking a transfer slot (`Storage::digest`). Parallel completions of one pending upload therefore each read up to 2 GiB. Memory stays bounded; only CPU and I/O are spent. Fix: a per-upload in-flight guard.
- **R6:** two Windows-only gaps in the MCP image confinement, both reaching only files inside the authorized directory:
  - `same_inode` always returns true on Windows, so a junction swap between open and check is not detected (`crates/ariad-host/src/confine.rs`).
  - 8.3 short names such as `PRIVAT~1` bypass the hidden-component check (`crates/ariad-host/src/assets.rs`).
- **R7:** `convert_confined` passes no asset handle. No caller exists today, but a future caller that sets `asset_base_dir` would bring SEC-02 back. Make it take an `AssetBase`, or document the restriction.

### R8–R9 (Info)

- **R8:** `/app` sends `Cross-Origin-Embedder-Policy: credentialless`. Turnstile's cross-origin iframe may be blocked there unless Cloudflare sends a compatible policy. This predates phase 7. Check anonymous cloud mode with a Turnstile site key in Chromium before release.
- **R9:** `docs/mcp.md` should say that hidden or symlinked image references are now left out of MCP output (`image_not_embedded`). That file is outside this task's ownership.

## Audit findings

| ID | Severity | Verdict | Evidence |
|---|---|---|---|
| SEC-01 | High | Fixed, pending tests | See the list below this table. |
| SEC-02 | Medium | Fixed on Unix, pending tests | `7836169`: MCP keeps the confined input's parent directory handle and resolves images through it one component at a time without following links. Hidden components are refused before any lookup. Ordinary CLI resolution is unchanged. Tests: `tests/confine.rs` (parent swap after the input copy), `tests/assets.rs`, `mcp_stdio.rs` (hidden reference). Windows gaps are R6. |
| SEC-03 | Medium | Fixed, pending tests | `0136cd9`: a cached key is trusted for at most 15 minutes after the last successful JWKS fetch, then the verifier fails closed (503). A throttled refresh cannot renew trust, and a key removed by rotation cannot come back. `tests/jwks-cache` uses injected time. A cap on token lifetime or `iat` age remains an optional hardening item. |

SEC-01 evidence:

- `f9deea5`: an admission slot is taken before any writer is created, a replayed URL is refused without its body being read, staging writes are bounded, idle and total deadlines apply, and permits are released on every exit path.
- `90787ce`: closes the slot-exhaustion bypass described in R2.
- Tests: `tests/blob-transfers.rs`.

The audit's other recorded items stay as the audit classified them: deployment conditions and accepted decisions, not findings. They cover anonymous bearer IDs, the S3 signer not binding content-length, Linux sandbox ABI limits, no email verification, and OAuth readiness for remote MCP.

## Other surfaces reviewed

- **CSP and headers** (`route-headers.mjs`): no `'unsafe-eval'` outside `next dev`, and inline scripts are hashed per exported page. Cross-origin isolation applies to `/app` only. Worker chunks and `/sw.js` get their own policy. nginx values are checked against header injection, and build-time origins are validated so a malformed value fails the build. `frame-ancestors 'none'`, `base-uri 'none'` and `object-src 'none'` apply everywhere. Nothing was weakened.
- **Dependencies.** The overrides match only vulnerable ranges and pin exact fixed versions. The `deny.toml` exceptions carry review dates. I ran these on the merged tree before the pause:
  - `cargo deny check`: advisories, bans, licences and sources ok.
  - `pnpm audit --prod` and `pnpm audit`: no known vulnerabilities.
  - `uv audit --frozen`: none found in 98 packages.
  - `cargo fmt --all --check`: clean. My later Rust edits were checked with `cargo fmt -p ariad-server --check`.
- **Secrets.** The diff adds no credential or private key; test secrets are the existing fixed placeholders.

## Verification for the coordinator

Run in this order from the worktree, once the coordinator sends RESUME:

```sh
just pandoc
cargo nextest run -p ariad-server --lib --test blob-transfers --test openapi_drift
cargo nextest run -p ariad-host --test confine --test assets
cargo nextest run -p ariad-cli --test mcp_stdio
pnpm --filter @ariadshift/web test -- headers
just sdk-check
just test-server
just ci
just desktop-ci
just web-e2e
just web-lighthouse
just deny && pnpm audit --prod && uv audit --frozen
just lint-commits origin/feat/cloud-product..HEAD
```

If `openapi_drift` fails, run `just openapi && pnpm --filter @ariadshift/sdk lock` and commit both files. The JWKS cache tests run under `--lib`: `crates/ariad-server/src/auth/jwt.rs` includes `tests/jwks-cache/mod.rs` with `#[path]`.

## Unresolved questions

- R4 needs a product decision on transfer deadlines and URL lifetimes for large `file://` uploads.
- Should the coordinator accept R6 (Windows, within the authorized directory only) for v1, or schedule a fix?
