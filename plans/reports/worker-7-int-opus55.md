# Worker result: phase 7 integration and final security review

Branch `fix/v1-security-audit`, cut from `origin/feat/cloud-product` (`a882c23`). I merged all phase 7 branches without rebasing and reviewed the integrated diff with `ak-code-review` (security focus). I fixed the two High findings that review found, and the Medium OpenAPI drift, each with regression tests. No critical or high finding remains open in code. Nothing has been compiled or tested: the brief forbade builds, and the coordinator then paused all heavy commands. Full review: [code-review-261025-phase7-final.md](code-review-261025-phase7-final.md).

## Commits

| Commit | What |
|---|---|
| `bd41cea` | Merge `--audit` (threat model, audit report) |
| `d81fbe8` | Merge `--csp`; resolved the `infra/docker/web.Dockerfile` conflict by keeping phase 5's Turnstile and operator build arguments and the CSP branch's generated nginx config and storage origins |
| `4e6eb2c` | Merge `--deps` |
| `484457b` | Merge `--mcp-assets` (SEC-02 fix, plus the completion plan files from `dev`) |
| `7ebad35` | Merge `--server` (SEC-01, SEC-03 fixes) |
| `be8da76` | `fix(web)`: account pages get the cloud origins in `connect-src` (High) |
| `90787ce` | `fix(server)`: configurable blob budget, per-workspace share and minimum transfer rate (High) |
| `f8b0347` | `fix(schemas)`: OpenAPI document and SDK lock for the new blob responses |
| last | `docs(plans)`: this file, the review report and the audit status table |

## Deviation from the brief: `--server` was merged, not skipped

The brief said to skip `--server` if it was already merged into `feat/cloud-foundation`. It is merged there (`9c4fb6d`), but `feat/cloud-product` was cut before that merge. Without it, SEC-01 (High) and SEC-03 would stay open on this branch. The merge was clean, because phase 5 never touched `crates/ariad-server` after the server branch's base. The commits are the same ones as on `feat/cloud-foundation`, so the two branches will not conflict when they meet.

## Files changed by my own commits

- `apps/web/route-headers.mjs`
- `apps/web/test/headers.test.ts`
- `docs/web.md`
- `crates/ariad-server/src/{app,config,storage}.rs`
- `crates/ariad-server/src/api/blobs.rs`
- `crates/ariad-server/tests/blob-transfers.rs`
- `docs/server.md`
- `schemas/openapi.v1.json`
- `packages/sdk/generator.lock.json`
- `plans/reports/security-audit-261025-v1.md` (status column only)
- the two report files

`crates/ariad-server/**`, `schemas/` and `packages/sdk/` lie outside the four named branches. I touched them because the High fix and the drift it caused live there.

## Checks run (none compile)

All of these ran on the merged tree before the pause:

- `cargo deny check`: ok.
- `pnpm audit --prod` and `pnpm audit`: clean.
- `uv audit --frozen`: clean.
- `cargo fmt --all --check`: clean; `cargo fmt -p ariad-server --check` was clean after my Rust edits.
- `pnpm biome check` on the two web files: clean.

`pnpm biome` triggered a `pnpm install` of the existing lockfile, which left `node_modules` in place. The lockfile is unchanged.

## Coordinator verification

Run these after RESUME. The full list is in the review report.

```sh
just pandoc
cargo nextest run -p ariad-server --lib --test blob-transfers --test openapi_drift
cargo nextest run -p ariad-host --test confine --test assets
cargo nextest run -p ariad-cli --test mcp_stdio
pnpm --filter @ariadshift/web test -- headers
just sdk-check
just test-server && just ci && just desktop-ci && just web-e2e && just web-lighthouse
just deny && pnpm audit --prod && uv audit --frozen
just lint-commits origin/feat/cloud-product..HEAD
```

If `openapi_drift` disagrees with the hand-edited document, run `just openapi && pnpm --filter @ariadshift/sdk lock` and commit both files.

## Open concerns

- **Untested fixes.** All fixes, mine and the merged ones, are uncompiled and untested. The Rust changes in `90787ce` were checked only by reading and `rustfmt`.
- **R4 (Medium, open):** a local transfer must finish within the shorter of 15 minutes and the signed URL's remaining lifetime, so large uploads on `file://` storage can time out. Fixing it changes the URL lifetime contract and needs a product decision. S3 deployments are unaffected.
- **Low items open:** R5 (completion hashing), R6 (Windows-only MCP image gaps), R7 (`convert_confined` footgun).
- **Turnstile check (R8):** Turnstile inside the cross-origin-isolated `/app` needs a check in Chromium.
- **`docs/mcp.md`:** it should describe the new MCP image rule. That file is outside this task's ownership.

Status: DONE_WITH_CONCERNS
