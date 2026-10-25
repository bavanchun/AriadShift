# AriadShift v1 security audit: early source review

| ID | Severity | Finding | Owning area | Status |
|---|---|---|---|---|
| SEC-01 | High | Replayed local upload URLs permit unbounded concurrent buffered bodies | Server storage and blob routes | Fixed in `f9deea5` and `90787ce`; tests not yet run |
| SEC-02 | Medium | MCP image resolution reopens ambient paths and bypasses hidden-file policy | Host assets and local MCP | Fixed in `7836169`; tests not yet run |
| SEC-03 | Medium | Failed JWKS refresh extends trust in a removed signing key without a deadline | Server authentication | Fixed in `0136cd9`; tests not yet run |

Status updated 2026-10-25 after the branches were integrated on `fix/v1-security-audit`. The final review of the integrated branch is in [code-review-261025-phase7-final.md](code-review-261025-phase7-final.md).

There are **zero confirmed critical, one high and two medium findings** in this source snapshot. This is an early audit, not release approval. The high finding must be repaired and verified before the security acceptance gate can pass. No source fixes, builds, tests, dependency-audit commands or exploit traffic were run, as required by the ASUS brief. Findings below are verified control-flow failures; proposed reproductions still need the coordinator's isolated test queue.

## Scope and immutable references

Reviewed on 2026-10-25, Asia/Saigon, from `fix/v1-security-audit--audit`. The checkout is based on `origin/feat/cloud-product-base`. Other task branches were inspected using `git show` and `git diff`; they were not checked out, modified or merged.

| Reference | Audited commit | Contribution reviewed |
|---|---|---|
| `origin/dev` | `3b8528149bc6a647bf50fbb992559cb4e2095b7c` | Completion plan and security acceptance requirements |
| `origin/feat/cloud-product-base` | `67a06fb59673e90ef7f72271073a47cc70c70afa` | Auth service, REST API, uploads, storage, web, compose and baseline workflows |
| `origin/feat/heavy-route-bench-and-freeze` | `8c9172ff53257af8ed968652484b812467a062a6` | Frozen IR/protocol, version-aware JSON reads, heavy routes and workflow changes |
| `origin/fix/docling-drop-intel-ipp` | `7293be4ab65848cb243623685bb2e1ee4b9c6b2b` | Pack builder, toolchain verification, pins, torch/toolchain/pack/release workflows |
| `origin/feat/v1-hardening` | `3b0ae7d98f013e15e330ea429594bd90a2e28dc4` | Seatbelt implementation, OS file-open handling, parser fuzz targets |
| `origin/feat/v1-hardening--api` | `c610e352f02ce293d3c2e5db01188c1654cf51f2` | Entitlement endpoint, upload deletion and API freeze |
| `origin/feat/v1-hardening--mcp` | `1d5b8c33b3402b17ec354a85a9531264bcf027a3` | Authenticated remote MCP transport and tools |

The completion plan is absent in this checkout and was read from `origin/dev`, including phase 7 and the original roadmap's phase 11. Prior foundation reports under `/home/vchun/Codes/ariadshift-ops/reports/` were supporting context; source code takes precedence. The refreshed [threat model](../../docs/security/threat-model.md) records trust boundaries and deployment assumptions.

## Confirmed findings

### SEC-01: concurrent local upload replay can exhaust the API's memory

**Severity: High.** Affects deployments using `ARIAD_STORAGE_URL=file://...`; the supplied compose stack uses S3 and does not execute this local PUT path. An ordinary or anonymous caller who can obtain one upload URL can deny service to all API users. Possession of a legitimate short-lived upload URL is the only prerequisite; no signing secret, hash collision, completed upload or conversion is needed.

**Affected source:** on `origin/feat/cloud-product-base`, `crates/ariad-server/src/api/mod.rs:274-283`, `crates/ariad-server/src/api/blobs.rs:52-90`, and `crates/ariad-server/src/storage.rs:392-413`. The blob/storage implementations remain unchanged on the audited API and remote MCP branches; the API branch adds routes but leaves blob isolation unchanged.

**Evidence:** blob routes disable the normal body limit and sit outside the request timeout. `put_blob` has no `Caller`, transfer lease, semaphore or per-key admission. It checks a signed method/size/hash and then enters `Storage::put_stream`. Token expiry is checked only before reading the body. That method creates a separate `object_store::buffered::BufWriter` for each request and waits for body chunks without an idle or total deadline. The locked `object_store` 0.14.2 implementation (`src/buffered.rs`, `BufWriter::new` and `poll_write`) buffers up to 10 MiB per writer before initiating multipart storage. Size and digest validation at EOF do not limit the number of simultaneous buffers.

**Concrete failure scenario:** obtain a URL for a declared 10 MiB input; the anonymous seed permits that size (`migrations/20261022000001_cloud_schema.sql:45-48`). Reuse exactly that URL on roughly 128 parallel PUTs. Send 9 MiB per request, declare the expected full `Content-Length`, then leave each body incomplete. Each request retains its own buffer while waiting. This pins over 1 GiB before HTTP/runtime overhead. Upload admission counted one upload, and neither workspace request limits nor concurrent-job limits run on these PUTs. Established bodies can remain open past the URL's 15-minute expiry. A bounded container can be OOM-killed; an unbounded process can pressure the host. The final SHA-256 never needs to match because EOF is withheld.

**Recommendation and owner:** server/storage owner should add bounded global and per-upload concurrent transfer admission before creating the writer, plus an idle body-read timeout and a finite transfer deadline. A PUT token already identifies the storage key, so admission need not require a second bearer credential. Reject excess readers without buffering and release permits on cancellation/error. Keep legitimate large uploads streaming. A route-level response timeout alone is insufficient unless it covers body consumption and cleans up partial storage. Review GET and upload-completion hashing for the same aggregate-resource boundary.

**Required regression evidence:** with a deliberately small transfer budget, hold one permitted request body open and verify the next replay is refused before accepting its body; verify idle requests and over-deadline transfers terminate, release admission, and remove partial state. Repeat after token expiry with a request accepted just before expiry. Test valid upload completion and digest mismatch afterward. Run the bounded memory scenario only in a disposable process/container, not against a shared service. These regressions do not exist yet.

### SEC-02: MCP ancillary images do not retain the input's confinement

**Severity: Medium.** This is a local confused-deputy disclosure, requiring either a crafted document referencing a hidden image or a directory writable by another actor. It does not affect direct REST uploads, whose conversion inputs live in per-job workspaces. It is not an arbitrary-file read: the asset resolver accepts supported raster-image signatures only.

**Affected source:** on `origin/feat/cloud-product-base`, `crates/ariad-cli/src/mcp/tools.rs:383-404`, `crates/ariad-host/src/convert.rs:892-915`, and `crates/ariad-host/src/assets.rs:37-39,241-283,316-373`. The corresponding local MCP/asset code is unchanged on the audited hardening branches. Compare the intended hidden-component check in `crates/ariad-host/src/confine.rs:365-375`.

**Evidence:** the main input is opened through `AllowedDirs` capability handles and copied to an isolated temporary file. Its parent is subsequently retained as a plain `PathBuf` in `asset_base_dir`. The native Markdown/HTML reader passes that path to `assets::resolve`, which canonicalizes and opens it with ambient authority. This is a new filesystem lookup after the original confinement check; it is not tied to the original allowed directory handle. The image lexical validator rejects `..`, absolute paths, URL schemes and several Windows forms, but permits hidden components such as `.private/photo.png`.

**Concrete disclosure scenarios:**

1. The user authorizes `/shared/inbox` and converts `/shared/inbox/sub/note.md`, containing `![image](photo.png)`. After input isolation, an actor with write permission on the shared parent renames `sub` and replaces it with a symlink to a private image directory readable only by the AriadShift user. The later ambient canonicalization follows the new parent, opens that private directory, and embeds `photo.png` in the returned output. The attacker needs permission to replace the shared directory, not permission to read the victim's image directory.
2. A document in an authorized root contains `![image](.private/photo.png)`. Direct MCP `inspect`/`convert` access to that hidden path is refused by `confine_input`, but conversion of the visible document reads and embeds it through `assets::resolve`. The returned HTML/IR makes those image bytes available to the MCP client.

**Recommendation and owner:** host/MCP owner should carry an already-open capability for the authorized image base into confined conversion, resolve image references relative to that handle, and apply the MCP hidden-component rule to ancillary reads. Preserve the ordinary CLI image behavior through an explicit policy or existing conversion boundary; do not globally remove supported sidecar images. Retaining the capability is necessary: rechecking a string and then reopening it leaves the race.

**Required regression evidence:** deterministically swap the original parent after the input copy but before asset resolution and verify the private image is never embedded. Verify hidden-directory and hidden-file image references are refused in MCP, while a normal authorized sibling PNG remains embedded. Retain existing main-input and output-directory swap tests. No new tests were written because this task owns documentation only.

### SEC-03: JWKS refresh failures have no maximum stale trust period

**Severity: Medium.** This is a signing-key revocation failure during an identity-service outage. It is not a signature bypass, algorithm-confusion attack or proof that any key is compromised. Exploitation requires a previously trusted private signing key plus continued inability to refresh the JWKS.

**Affected source:** on `origin/feat/cloud-product-base`, `crates/ariad-server/src/auth/jwt.rs:111-133,136-174`. This verifier is also used by the remote MCP branch and is unchanged in the audited sibling branches.

**Evidence:** the normal JWKS TTL is 300 seconds. `find` captures a matching stale key, attempts `refresh`, and returns that stale key on any refresh error regardless of its age. The cache's timestamp is never checked against a maximum failure grace. JWT validation requires `exp`, `iss`, `aud` and `sub`, but does not require issuance time or a maximum token lifetime. The service issues 15-minute tokens, yet a holder of a compromised signing key can choose a later `exp`.

**Concrete failure scenario:** an API replica caches key K. The auth operator removes K after compromise, but that replica cannot reach JWKS because the service or its network route is unavailable. Tokens signed with K continue to validate indefinitely on that replica, provided their `exp` is in the future. Removing K on the issuer does not bound this replica's trust. A healthy successful JWKS refresh does remove K; the issue is specifically repeated refresh failure.

**Recommendation and owner:** auth owner should define a finite maximum stale grace and reject or return service unavailable once it expires. If preserving outage availability is required, make that grace an explicit operational policy and document the revocation delay; do not describe the five-minute refresh TTL as a revocation bound. Add a maximum accepted token lifetime/age if the 15-minute lifetime is intended to constrain incident recovery. This report does not change the existing availability policy.

**Required regression evidence:** inject time and a controllable JWKS fetcher; cache K, advance past TTL, fail refreshes, and verify bounded grace followed by rejection. Separately verify successful rotation removes K, unknown-kid requests remain throttled, and ordinary EdDSA/issuer/audience/expiry tests still pass. Do not wait through real five-minute timers in a unit test.

## Coverage and verified controls

| Surface | Source reviewed and conclusion |
|---|---|
| Better Auth and OAuth | `services/auth/src/{auth,config,app,main,migrate}.ts`: explicit origin allowlist, host-only HttpOnly/Lax cookies, Secure on HTTPS, private client-address header overwritten from socket/proxy policy, 64 KiB body cap, DB rate counters, Ed25519 JWTs, API-key session impersonation disabled, separate auth schema. GitHub/Google credentials are optional paired settings. OAuth callback internals are library-owned; no live provider flow was exercised. |
| JWT algorithms and API keys | `auth/jwt.rs` excludes HMAC, selects keys by `kid`, checks declared key algorithm, sets validation to the token algorithm and requires issuer/audience/expiry/subject. Missing JWK `alg` is permitted by this verifier; current configured issuer emits EdDSA keys. This is not evidence of an exploitable asymmetric/HMAC confusion. `auth/api_key.rs` hashes before parameterized lookup, enforces configured owner type, disabled/expiry and exhaustion snapshots. The configured service intentionally disables Better Auth's key rate counters; workspace quotas own usage. |
| Upload ownership and keys | `api/uploads.rs`, `api/jobs.rs`, `storage.rs`, `entitlements.rs`, `db.rs`, `sweeper.rs`: UUID resource identities, workspace visibility, tenant/owner/direction/hash storage keys, transactional subject locks, declared-size admission and stream/hash verification at completion and worker download. Unknown or cross-workspace IDs do not grant a signed URL. Anonymous random IDs are intentionally bearer capabilities. |
| Upload replay and content type | PUT lifetime is 15 minutes; GET is five minutes. S3 signs the checksum, not content-length; completion and workers recheck actual bytes. No content-type claim is treated as trusted format detection. Local GET uses octet-stream and no-store. Pending/completed/deleted-input sweeps address late PUT replay. Those controls do not repair SEC-01's in-flight memory exhaustion. |
| Upload DELETE and plan limits | API branch `api/uploads.rs:262-305` locks the upload row against job creation, checks ownership and job references, retires before deleting, and permits deletion retries. `api/limits.rs` returns the caller's entitlement, not a request-controlled plan. The frozen OpenAPI snapshot is compatibility evidence, not a security test. |
| SSE | `api/events.rs`: ownership check before subscription; eight streams per subject and two per job through DB leases; 90-second expiry, 30-second renewal, drop cleanup, bounded broadcast channel and paged event queries. `Last-Event-ID` is parsed as a nonnegative integer. Streams close on terminal events. Authentication is at stream creation; bearer expiry/revocation is not continuously checked. |
| Local MCP | `confine.rs`, CLI `mcp/{mod,tools,resources}.rs`: empty roots fail closed, main reads use capabilities and regular-file checks, output parent handles survive swaps, overwrite requires both server and tool consent, produced-resource registry is session-scoped and capped. SEC-02 concerns the separate image path. Resource reads perform a 32 MiB metadata check but then read to EOF; a concurrent writer can exceed that nominal cap. This latter local co-writer limitation is recorded for regression coverage, not promoted to an independent high finding. |
| Remote MCP | Remote branch `mcp/mod.rs:43-151,165-210`, `mcp/tools.rs`: anonymous auth disabled; exact public Host and Origin checks; separate `/mcp` audience; `Caller` quota/throttle reuse; 1 MiB HTTP body cap; stateless JSON responses; 256 KiB inline UTF-8 cap; no filesystem path/URL fetch tool; upload/job handlers reused; inline requests consume upload admission. No cross-tenant path or arbitrary URL input was found. |
| Pack installer | `packs/{install,signature,fetch,manifest,extract,zstd}.rs`: bounded manifest/signature reads, compiled trust keys, prehashed Minisign and signed pack/version identity, component size/SHA-256 verification including cached blobs, staging and per-pack locks. Extraction rejects traversal, unsafe component names, special files, writes through links and escaping link targets, with streamed byte and entry budgets. Signed size/entry declarations are trust inputs, not arbitrary document metadata. |
| Toolchain wheels | Docling branch `pack/{fetch,build,native_build,torch_build,licenses}.py`, `pins.toml`, `torch-build.toml`: pinned archive hashes/sizes, checked cached blobs, frozen dependency export with hashes, source/build-key provenance and CPU ABI/license probes; source-build bootstrap and pinned published-wheel paths are distinct. Python extraction uses the data filter. A checksum generated from the same artifact establishes consistency, not independent producer trust; the trusted source refs, reviewed pins and cache scopes remain part of the boundary. |
| Linux sandbox | Host `sandbox{,/linux,/linux/private_tmp}.rs`, engine path declarations, server `sandbox.rs`, runner/worker: required-mode preflight, Landlock rights plus seccomp denial of non-UNIX socket creation and io_uring including x32 encodings, private Office temporary mounts, cleared helper environment, worker non-dumpability and process-tree watchdogs. ABI-dependent local IPC gaps and native-reader exemptions are already documented; see assumptions below. |
| macOS Seatbelt | Hardening branch `sandbox/{macos,macos_profile}.rs`: deny-default policy, no network grant under deny-network, policy paths passed as parameters, no `/System` blanket grant, writable workspace distinct from executable roots, enforcement probe, fixed shell source/quoted positional arguments, environment hooks removed, rlimits before exec. Missing Seatbelt is an explicit best-effort warning or required-mode failure. Linux cannot validate Apple's runtime semantics. |
| Desktop deep links and OS files | `deep_link.rs` accepts route/profile preferences only and ignores auth callback; it does not accept document paths or credentials. Hardening `file_open.rs` receives OS file selections, filters formats and regular files, and caps the pending list. Generated OS integration passes arguments without evaluating document paths as shell code. |
| Web conversion and headers | `next.config.ts`, `route-headers.mjs`, converter components/client/worker/pandoc/cache/offline scripts and core HTML writer: no converted document is mounted with innerHTML/srcdoc; outputs are Blob downloads. Core escapes generated attributes/text and sanitizes raw HTML. COOP/COEP are limited to `/app` and compatible worker chunks. A CSP and general frame/nosniff/referrer policy are absent in this snapshot; T3 already owns that work. Absence alone is not counted as a demonstrated XSS. |
| Service worker | `service-worker.js`, `offline.ts`, engine cache: runtime fetches are GET and same-origin only, converter-only navigation fallback, engine resources are versioned, normal cache messages originate from app resource collection. The message handler itself accepts arbitrary same-origin URL arrays and does not check no-store; it must not be reused for authenticated API caching. Current compose keeps auth/API on separate origins and this web snapshot has no cloud-auth UI; no authenticated-cache leak was verified. |
| Compose and DB | `infra/compose/**`, `infra/server-roles.sql`, container entrypoints: required secrets have no embedded fallback values; ports bind loopback by default; DB has no published port; separate auth/API/worker logins, no runtime schema ownership, no worker identity/JWKS-table grants, bucket-scoped storage identity; workers have internal-only networking, read-only roots, memory/PID/tmpfs limits. Office lifts AppArmor only for its namespace needs. No live role inspection or container startup was performed. |
| GitHub workflows | All baseline workflow files plus named-branch workflow diffs: no `pull_request_target` or cross-run `workflow_run` artifact promotion found. Actions use commit pins; checkout disables persisted credentials; PR jobs have read permissions; publish permissions/secrets are scoped to release jobs/steps; pack and toolchain artifacts are downloaded from their own run. Source/compiled caches are not an independent authenticity root. Fork PR merge-ref caches cannot be restored by release refs under GitHub's documented isolation. |
| Secret exposure | Tracked-file and high-confidence private-key/token pattern checks over source/config/scripts found no matching committed credential. Only `.env.example` matched the tracked dotenv filename check. This is a bounded source scan, not a guarantee about repository history, provider secrets or live deployment values. |

### Parser inventory

Every identified product parser family was included in the source review. This is not a claim that every parser implementation or dependency is vulnerability-free.

| Family | Boundary and reviewed controls | Fuzz coverage in audited hardening branch |
|---|---|---|
| Markdown, YAML front matter and HTML | Native readers with input admission, Markdown nesting scan, iterative AST/DOM handling, HTML chunk/node/depth budgets; front matter byte cap and alias/anchor rejection. Dependency `saphyr-parser` 0.2.2 also limits flow nesting via a checked u8 counter; a supposed unlimited nested-flow drop attack was rejected after inspecting that source. | `markdown_reader`, `front_matter`, `html_reader` |
| IR JSON, Pandoc AST and DoclingDocument JSON | Host bounded byte/depth scanner plus serde_stacker for native IR; core in-memory bounded readers and traversal budgets; protocol/IR version checks reviewed on the freeze branch; Docling reference expansion and graph work limits. | `ir_json`, `ir_json_versioned`, `pandoc_ast_to_ir`, `docling_to_ir` |
| PDF | `reader/pdf/**`: object, page/form recursion, operator/glyph, retained-font and decompressed-stream/amplification budgets; native execution in the PDF child; browser execution inside WASM worker. | `pdf_reader` |
| ZIP/Zip64, OLE/CFB and package XML | Host archive preflight bounds central directory, names, duplicates, entries and expanded data; CFB directory walk is finite/cycle-bounded. `core/package_meta.rs` limits entries to 64 MiB and in-memory total to 256 MiB; quick-xml rewriting does not fetch external entities. | `archive_preflight`, `docx_meta` |
| External document/image engines | Pandoc sandbox and memory cap, isolated LibreOffice with macro/link policy and per-process profile, Docling/PDFium/OCR with input and page/image checks plus outer watchdog. Engine-private parsing can fail without becoming API-process parsing. Typst source is generated from escaped IR, not accepted directly from an API field. | `typst_writer`; direct third-party parser campaigns remain upstream/engine work |
| Engine protocol and capabilities | Bounded NDJSON framing, typed messages, protocol version validation, request/path separation, bounded JSON request sizes. Python Docling protocol is line-capped; host is its trusted request producer. | `engine_protocol`, `capabilities_json` |
| Filesystem/URI/assets and pack formats | Capability confinement and URI syntax validators; SEC-02 identifies the ancillary-read exception. TOML manifest validation, signed artifacts, tar/ZIP path/link checks, bounded concatenated-zstd processing. | `uri_parsers`, `asset_resolve`, `pack_manifest`, `pack_archive` |

## Existing decisions and deployment conditions

- Anonymous UUIDs are deliberately shareable capabilities. Learning an anonymous ID permits access, including destructive operations. This matches `Principal::can_access` and architecture section 9.5; it is not reported as a tenant IDOR. Do not expose IDs or signed URLs in public logs.
- S3 PUT signatures deliberately cannot bind content-length through the current signer. Completed-object validation and replay sweeps were verified. The deployment-wide ingress size/rate/concurrency ceiling documented in `docs/server.md` and architecture section 9.4 is still required; direct public exposure of port 8333 without those limits can consume storage/bandwidth before the sweeper acts. The loopback compose development default is not itself an Internet-facing exploit. This audit does not reverse the accepted signer choice.
- Required Linux sandbox mode does not imply ABI 9 UNIX-socket mediation on older kernels. Native helper orchestration retains filesystem/UNIX IPC access. Use supported kernels and dedicated workers with no host sockets, credential-bearing same-user peers, metadata route or unnecessary mounts. Existing documentation explicitly states these limits; they are not newly discovered sandbox escapes.
- Local workload limits intentionally allow large inputs/assets. Do not turn unlimited local policy into a cloud exemption. Cloud input/memory/deadline limits and container limits remain mandatory.
- Better Auth does not require email verification because the self-hosted stack has no mail transport. This is an explicit product decision. Do not infer email ownership for authorization or silently change signup policy. Combined credential/OAuth account linking needs a real integration regression before release; no account-takeover claim is made from defaults alone.
- Current primary docs confirm that JWT-plugin tokens are separate from session cookies and describe implicit OAuth account linking. The shipped service JWT audience is the auth origin; remote MCP requires the `/mcp` resource audience. API-key MCP works by source inspection, but OAuth-capable token issuance/discovery is not demonstrated by the current service configuration. The remote MCP handoff also calls out the authorization-server requirement. Treat OAuth readiness as an integration gap, not a reason to weaken audience checks.

## Coordinator verification and closure

No build or test was run here. The coordinator should first integrate the named branches and resolve SEC-01/SEC-02 with focused regressions. Use the actual merged paths; branch snapshot line numbers above will move. Review SEC-03's outage/revocation trade-off and record the chosen finite policy or explicit risk acceptance.

Run these commands sequentially through the shared build queue, from the integrated worktree, after sourcing that machine's pinned environment:

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

Confirm `mcp_stdio` against the integrated crate's test targets before running it; add the new regression tests to their owning suites. `just test-server` must actually start its disposable services: its skip-on-missing-Docker path is not integration evidence. Run `sandbox_macos` on the Mac, and exercise required-mode Linux sandbox/container tests on ASUS. Exercise fuzz targets in a disposable checkout/nightly job; no long-running fuzz process was started here.

Dependency review belongs to T2. No claim of a clean RustSec/npm/Python advisory inventory is made. T3 must deliver and verify the CSP/header policy without losing converter isolation or the Lighthouse threshold. Recheck the merged web cloud UI for credential storage, authenticated service-worker caching, output rendering, OAuth linking and logout behavior; that UI is not present in this checkout.

**Open questions / remaining evidence:** no user answer is required to finish this read-only task. Release closure still requires the code fixes and regressions above, dependency audit evidence, real Linux/macOS sandbox execution, OAuth issuance/discovery evidence and a final audit of the integrated branch. All three findings remain open.

## Primary references consulted

- [Better Auth JWT plugin](https://better-auth.com/docs/plugins/jwt): session/JWT distinction and JWKS verification guidance. This report's maximum-stale concern follows the project verifier's control flow, not a claim of a library advisory.
- [Better Auth accounts and linking](https://better-auth.com/docs/concepts/users-accounts#account-linking): implicit linking behavior; cited as an integration test requirement, not a confirmed exploit.
- [GitHub dependency cache isolation](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching#restrictions-for-accessing-a-cache): merge-ref, branch and tag isolation, used to reject an unsupported fork-cache-poisoning allegation.
