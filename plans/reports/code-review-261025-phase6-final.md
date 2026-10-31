# Final review of v1 hardening

The branch is integrated, but the pre-merge verdict is **FIX_REQUIRED**. One high
acceptance gap remains outside this worker's ownership: the shipped auth service
cannot complete remote MCP OAuth. No critical finding was identified. Compilation,
runtime tests and platform acceptance remain the coordinator's responsibility.

Date: 2026-10-25, Asia/Saigon. Worktree:
`/home/vchun/Codes/01-AriadShift-wt-p6-int`. Branch: `feat/v1-hardening--int`.
Review followed the installed `ak-code-review` skill's base and API checklists,
with source-backed triage against `origin/feat/cloud-foundation...HEAD`.

## Integration

Started at `c39a6bb`. All requested branches were merged without rebase, history
rewrites or textual conflicts, in this order:

| Source revision | Merge commit |
|---|---|
| `origin/feat/cloud-foundation` (`504a212`) | `8526c5f` |
| `origin/feat/v1-hardening--api` (`c610e35`) | `873b3d1` |
| `origin/feat/v1-hardening--mcp` (`1d5b8c3`) | `5b21e49` |
| `origin/feat/v1-hardening--jobs` (`222c53c`) | `0c5f037` |

Both methods on `/v1/jobs` survive the merge. Limits and upload deletion remain
registered, and `serve_api` mounts remote MCP. The phase-4 baseline was reviewed
only where it supplies context for the changed trust boundaries.

## Earlier findings

The earlier review is at
`origin/feat/v1-hardening--review:plans/reports/code-review-261025-phase6.md`.

| Finding | Final disposition and evidence |
|---|---|
| H1: OS-selected files never reach the window | Resolved by `58e6046`, retained through desktop polish. The command table, backend, generated wrapper and ACL expose `opened_files`. The frontend subscribes before its initial drain and drains on later events. Rust inbox tests and frontend cold/second-launch tests cover the handoff. Runtime verification is pending. |
| H2: fragment whitespace crashes the fuzz oracle | Resolved by `f3a5134`. The trim assertion applies only to non-fragments, matching `allowed_link`; the trailing-whitespace fragment seed remains intentional. |
| H3: nightly ASan exceeds its combined timeout | Resolved by `2b0c65a`. Nine independent matrix jobs each retain a 600-second run and a 35-minute job budget. Actual cold-build duration remains unmeasured. |
| M1: regular CI fuzz budget is tight | Mitigated by `c0b602a`: the timeout is 45 minutes. Measure cold-cache duration in CI. |
| M2: Seatbelt grants broad Homebrew/system read scope | Open, medium; see below. |
| MCP deployment note: exact Host matching | Partially documented in `docs/mcp.md`: preserve the public Host. HTTP/2 authority-only requests and explicit default ports remain unsupported. |
| Earlier API/MCP no-high verdicts | API authorization and isolation conclusions still hold on source inspection. The MCP verdict does not establish the OAuth acceptance criterion; the auth-service gap below is confirmed. |

## High findings and integration fixes

### Open: interactive remote MCP OAuth cannot complete

Evidence: `services/auth/src/auth.ts:81` configures JWT, API keys, passkeys and
2FA, with JWT audience fixed to `config.baseUrl` at line 86. It provides neither
an OAuth authorization server nor resource-bound MCP access tokens.
`crates/ariad-server/src/mcp/mod.rs:123` advertises that issuer, while the MCP
verifier requires the public `/mcp` audience. `docs/mcp.md:198` explicitly records
the missing provider. The remote integration test signs tokens with `TestIssuer`;
it never obtains one through the shipped Better Auth service.

Scenario: MCP Inspector discovers the advertised issuer and attempts authorization
code plus PKCE. The shipped service cannot complete discovery/authorization/token
exchange for that resource, and its ordinary `/token` JWT is rejected at `/mcp`.
API-key access and verification of separately supplied valid MCP-audience JWTs are
implemented; they do not fulfill the requested interactive OAuth acceptance.

Required repair: the auth-service owner must implement discovery, client
registration or pre-registration, code plus PKCE S256, and signed resource-bound
access JWT issuance compatible with the current JWKS verifier, then test the real
flow through MCP Inspector. `services/auth/**` is outside this brief's editable
files, so this worker has not changed that service or relaxed audience checks.
The current [MCP authorization specification](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization)
requires authorization-server discovery and resource-bound access tokens.

### Fixed: remote MCP manifest and Cargo lock disagreed

`crates/ariad-server/Cargo.toml` enabled rmcp HTTP features, but the lock omitted
the server dependency edge and the HTTP feature closure. Locked builds would need
a lockfile update. Fixed in **`a858dc1`** by reconciling `Cargo.lock` with the cached
rmcp 3.5.1 manifest and the crates.io sparse registry. Existing versions were kept;
`sse-stream 0.2.6` is the latest stable version compatible with rmcp's `^0.2.4`
requirement. Its checksum and `MIT OR Apache-2.0` license were verified against the
registry. Static parsing finds no dangling lock dependency edges. Cargo resolution
and compilation were not run; the coordinator must confirm with `--locked`.

### Fixed: SDK generator lock described the old API

`generator.lock.json` still hashed the phase-4 schema, so SDK drift checks would
reject the merged limits, deletion and listing operations. Fixed in **`166db04`**.
The normalized schema SHA-256 is now
`0cafcc247064de49ad653562e1958ab36251e9c07d2df30da1104e3db618ceda`.
Generator version and configuration hash are unchanged.

## Medium and low findings

- **Medium, Seatbelt read scope:** `crates/ariad-host/src/sandbox/macos_profile.rs:30`
  permits all of `/usr` and `/opt/homebrew`, including service data beneath their
  `var` directories. A malicious engine can read such data into conversion output
  even though networking is denied. The shared policy also supplies broad paths,
  so narrowing only the profile constant is insufficient. The sandbox owner should
  narrow the effective runtime grants with real macOS engine coverage. This is the
  earlier M2; no unverified allow-list rewrite was made on Linux.
- **Medium, transport compatibility:** `mcp/mod.rs:165` requires exactly one Host
  equal to the canonical URL's authority. HTTP/2 authority-only requests and clients
  explicitly writing the default port are refused. Deploy with an HTTP/1.1 proxy
  that preserves the canonical Host, or extend and test both validation layers.
- **Medium, freeze checker false positives:**
  `tests/openapi_v1_snapshot.rs:660` compares composite schemas for full equality.
  Adding an optional property to the inline part of `JobListItem.allOf` fails the
  gate despite the additive-change policy. Current unchanged composites compare
  successfully. A future extension should compare their component shapes.
- **Medium, architecture documentation:** `ARCHITECTURE.md:714` still says macOS
  has no engine sandbox and Seatbelt is future work. Its MCP section also
  documents only stdio and omits the newly mounted remote boundary. Those sections and the decision log
  need the owning maintainer's update; this brief does not own that file.
- **Medium, remaining verification gates:** no compiler, runtime, fuzz duration,
  actual OAuth or native installer checks were run. The hand-maintained OpenAPI
  must pass the real generator drift test. The new dependency also requires the
  generated license inventory check, with regeneration by its owner if needed.
- **Low, inherited formatting:** `cargo fmt --manifest-path fuzz/Cargo.toml --check`
  fails in unchanged `fuzz/fuzz_targets/pdf_reader.rs:32`. The file is identical to
  the phase-4 baseline and outside this worker's editable scope. Formatting passes
  for all nine phase-6 targets.
- **Low, inherited commit policy:** the phase-6 commit lint range fails for the
  91-character subject of `9734cfc`. No pushed history was rewritten. A broader
  range starting at the initial branch head also includes long commit-body lines
  in inherited phase-4 commits `e0f68a8` and `afb5ea1`.
- **Low, generator formatting, fixed:** **`57bb971`** formats the desktop OS
  integration generator and documents the intentional literal NSIS placeholder
  for Biome. `node .../generate.mjs --check` confirms unchanged emitted files.

## Contract consistency and source review

`just openapi` invokes `cargo run --locked`; no non-compiling OpenAPI generation
script exists. The merged schema and frozen snapshot were therefore reconciled
by reading route annotations, DTOs and the JSON, without executing the generator.
They remain byte-identical and contain no unresolved local schema references.
GET `/v1/jobs`, GET `/v1/limits`, DELETE `/v1/uploads/{id}`, their operation ids,
security alternatives and response component references match the merged source.
The SDK's new wrappers refer to those operations; generated client validation is
still pending. No baseline was weakened or breaking change accepted.

The source review also checked row-lock ordering between upload deletion and job
creation, workspace filtering and bounded keyset pagination, omission of retired
filenames, shared entitlement counters, MCP audience separation and credential
stripping, body/inline limits, Host/Origin gates, inherited conversion boundaries,
Seatbelt parameterization and fail-closed setup, OS launch argument quoting and
the queue-drain handshake, desktop shortcuts/feedback, parser fuzz oracles and
workflow budgets. Existing tests cover the new behavior; this worker added no
runtime behavior that requires a new unit test.

## Verification performed

Passed:

- `cargo fmt --all --check`.
- `rustfmt --edition 2024 --check` on all nine new fuzz targets.
- `just lint-workflows` (actionlint and zizmor; zizmor uses its default offline mode).
- `pnpm exec biome check` on desktop sources, tests, OS integration generator,
  and changed SDK sources/tests/lock: 18 files after the formatting repair.
- `node apps/desktop/os-integration/generate.mjs --check`.
- Static JSON/TOML inspection of route references, schema/snapshot equality,
  SDK hashes and Cargo lock edges.
- Commit lint for the three integration repair commits (`0c5f037..HEAD`, before
  adding this report).

Failures are retained above: the full fuzz-workspace formatting check and inherited
commit lint. Full-range `git diff --check` also flags the deliberately trailing
space in `fuzz/seeds/uri_parsers/anchor-trailing-whitespace.txt`; removing it would
remove H2's regression input. New repair diffs pass whitespace checks.

No build, unit/integration test, container, database operation, dev server or fuzz
execution was started. One direct inspection of a dependency directory was denied
by the environment hook; it was not bypassed. The allowed pnpm Biome command's
wrapper bootstrapped pinned workspace dependencies automatically. No tracked
manifest or pnpm lock changes resulted.

## Coordinator verification

Run these sequentially in the coordinator's build queue, with its pinned tool
environment and a disposable PostgreSQL/pgmq test service configured through
`ARIAD_TEST_DATABASE_URL`. Integration tests otherwise skip.

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

If OpenAPI regeneration changes the file, review the actual change before
refreshing its frozen baseline and SDK lock. Never refresh away a breaking change.
If notices drift, the inventory owner must regenerate them with `just licenses`.
On macOS also run `cargo test -p ariad-host --features test-probe --test sandbox_macos`
and real Pandoc, Typst and Office conversions. On each native platform inspect the
installer and verify cold and warm file-manager launches populate Convert.
After the auth-service repair, verify actual OAuth through MCP Inspector, API-key
access, tool listing, upload conversion, status polling and output downloads.
Measure one complete nightly matrix run.

## Unresolved question

Which auth-service owner will deliver the OAuth provider and the real client
acceptance evidence before this integrated branch is merged into `dev`?

Status: BLOCKED
