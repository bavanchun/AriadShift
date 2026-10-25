# Security threat model

This model describes AriadShift's document-processing trust boundaries. It is based on the cloud-product foundation snapshot and the named hardening branches reviewed on 2026-10-25. The [audit record](../../plans/reports/security-audit-261025-v1.md) pins their exact commits, findings and verification limits. A branch-level control described below still needs integrated runtime verification before release.

[ARCHITECTURE.md](../../ARCHITECTURE.md), especially sections 9–12, owns the design. This document explains assets, attackers, assumptions and residual exposure; it does not change architecture or declare security acceptance complete.

## Assets and attackers

Protected assets are input/output document bytes and metadata, files outside an authorized conversion root, auth sessions and API keys, signing/storage/database credentials, tenant isolation, conversion quotas, service availability and the integrity of distributed engines/toolchains.

Attackers include unauthenticated Internet clients, authenticated users targeting other workspaces or service resources, producers of malicious documents/archives/IR, hostile MCP tool input, a less-trusted actor able to modify a shared input directory, and suppliers or PR authors attempting to influence release artifacts. A document's text is also untrusted downstream model input; successful conversion does not authenticate instructions contained in it.

The trusted computing base includes the AriadShift binaries and configured policy, the auth issuer and configured JWKS route, deployment operators, OS sandbox/kernel/runtime, reviewed signing keys and release refs, DB/object-store access policy, and the web origin that serves executable code. Compromise of the API DB login, auth signing private key, release signing key, browser origin or kernel exceeds ordinary malformed-document isolation. Its consequences and recovery still matter, particularly signing-key revocation.

## Boundary map

| Boundary | Untrusted material | Enforcement and limits |
|---|---|---|
| Browser to auth service | Credentials, OAuth callbacks, sessions and Origin | Better Auth plugins, explicit trusted origins, host-only HttpOnly/Lax cookies, Secure on HTTPS, 64 KiB body limit and database-backed throttling |
| Client to REST API | Bearer/API-key headers, JSON requests, resource IDs | JWT signature/claims checks or hashed API-key lookup; workspace visibility; bounded JSON; DB rate counters and entitlement admission |
| Client to object store | Upload body and replayed signed URL | Tenant/owner-scoped key, signed method/expiry/checksum; completion and worker recheck size/hash; sweeper removes invalid and expired/replayed objects |
| REST API to worker | Job row, queue message and staged bytes | Transactional admission, idempotent metering, per-job lock, finite cloud limits and isolated conversion helper |
| Host to engine | Documents and engine output/protocol | Temporary workspace, engine protocol framing, path validation, Linux Landlock/seccomp or macOS Seatbelt, process-tree deadline/RSS checks |
| Local MCP to host filesystem | Paths, roots, overwrite intent and resource URIs | Allowed-root capabilities, main-input isolated copy, parent capability for output, hidden-component restrictions, session-produced resource registry |
| Remote MCP to cloud | Authenticated HTTP JSON and inline/upload IDs | Separate MCP JWT audience or API key; Host/Origin validation; no anonymous callers; shared cloud admission; bounded inline data; no arbitrary local paths or remote URL fetch |
| Pack source to installed executable | Manifest, compressed components, wheel/source caches | Compiled Minisign trust, signed identity, pinned size/hash, staged extraction with path/link and resource checks, source-build provenance and reviewed wheel pins |
| PR/build to release | Actions code, caches and artifacts | Commit-pinned actions, restricted permissions, no persisted checkout token, same-run artifact transfer, release-scoped secret steps and trusted publication refs |

## Identity and tenant invariants

The auth service owns its separate Postgres schema. API/worker runtime roles do not own schema objects. The worker cannot read identity/signing tables; the API may read the API-key table. Runtime DB credentials are separate from migration-owner credentials. See [self-hosting](../self-hosting.md) and [server operation](../server.md).

JWT verification selects a JWKS key by `kid`, rejects symmetric algorithms, matches any declared key algorithm, verifies signature, and requires nonempty subject plus expiry/issuer/audience. The configured service signs Ed25519 tokens for 15 minutes. A missing JWK algorithm declaration is not rejected by the current verifier; an issuer configuration change must retain an explicit algorithm policy. JWKS fetches have timeouts, caching and unknown-kid retry spacing. A failed refresh currently allows a matching stale key indefinitely; the audit records the resulting revocation risk.

API keys are hashed before lookup and checked for configured owner type, disabled state, expiry and exhausted snapshots. The current service disables Better Auth's API-key rate counters because the server performs no counter writes. Per-workspace quotas own API consumption. Do not offer a key-specific usage cap as enforced without changing that contract.

Authenticated objects belong to a workspace. Anonymous objects use a shared workspace and random UUIDs as bearer capabilities; any holder may read/use/delete them. Anonymous quotas are keyed by the client address, with IPv6 grouped by /64. Anonymous IDs and presigned URLs must be kept out of public logs and analytics.

Auth cookie scope and API CORS are separate controls. API clients authenticate with bearer/API-key headers; the API does not use browser session cookies as ambient authorization. Trusted forwarding headers are safe only when the upstream proxy overwrites them and direct access cannot bypass that proxy. Local loopback HTTP is a development route; public use requires TLS.

Self-hosted password signup deliberately does not prove ownership of an email address. Email is not an independent authorization assertion. GitHub/Google linking and account-recovery behavior require integration coverage when those providers are enabled. The audited remote MCP endpoint requires a resource-specific audience; auth-origin REST JWTs must not become acceptable there as an interoperability shortcut. OAuth authorization-server discovery and token issuance need separate end-to-end evidence.

## Uploads, streaming and lifecycle

Storage keys include tenant, owner UUID, direction and content hash. Upload filenames are cleaned metadata rather than storage paths. Content type supplied by a caller is not proof of document format. Completion checks actual size/hash before format detection, and a worker rechecks downloaded bytes so a replay cannot replace trusted content silently.

PUT URLs expire after 15 minutes and GET URLs after five minutes. S3 checksum signing does not enforce per-plan content-length before storage ingress; completion is authoritative. Sweepers recheck pending/completed/retired inputs around PUT validity and retention to remove mismatches and late replays. Operators must enforce an ingress body-size ceiling and aggregate bandwidth/concurrency limits on public object storage. A sweep does not prevent temporary resource use.

The local store uses HMAC-signed method/key/expiry/size/hash claims and checks streamed size/digest. Its PUT route currently lacks transfer admission and body deadlines. Reusing one URL for many incomplete bodies can retain independent buffers and exhaust the API; this is the open high finding in the audit. Content-length and final hash checks do not supply aggregate-resource isolation.

Upload admission counts abandoned and deleted upload reservations against the daily allowance. Pending uploads reserve concurrency slots; job submission separately uses the ledger and active-job limits under the same subject lock. Upload DELETE on the API hardening branch locks against job creation and refuses deletion while referenced by a job. Deletion is retriable, with the sweeper responsible for late PUT cleanup.

SSE checks ownership at connection creation and leases no more than eight streams per subject and two per job across replicas. Leases expire and renew, and terminal jobs end streams. Authentication is not reevaluated continuously during an established stream; job deadlines and retention limit its useful lifetime. Long-lived connections and hashing must still be considered in aggregate API resource sizing.

## Parser and engine isolation

Native Markdown/HTML and mappings from bounded JSON are explicit in-process exceptions. Markdown has nesting/workload checks; YAML front matter has a byte cap and rejects anchors/aliases; HTML uses bounded incremental DOM construction. Native IR JSON uses a streaming byte/depth scanner and grown-stack deserialization. Pandoc/Docling mappings validate depth, blocks and expansion. The PDF reader limits objects, pages, form recursion, glyph/operator work and decompression/amplification; native PDF parsing runs out of process, with browser parsing in a WASM worker.

Archive preflight bounds ZIP central-directory size, entry names/count, duplicates and expanded data, and walks legacy CFB directories with finite/cycle checks. Package metadata rewriting caps XML entry bytes and in-memory aggregate bytes. XML handling does not resolve remote entities. Fuzz targets cover these native parser families, protocol messages, capabilities, URIs/assets and pack formats on the audited hardening branch. Coverage and target presence are not evidence of a completed fuzz campaign.

Pandoc, LibreOffice, Docling/PDFium/OCR and Typst execute as separate engines. Pandoc has its own sandbox/memory parameters. LibreOffice uses private profiles with macro/link controls. Docling disallows external plugins and uses staged local models. Generated Typst source escapes document text. Conversion failures must remain typed failures; parser crashes must not acquire API credentials or another job's files.

Linux required mode needs working Landlock and seccomp. Rules restrict filesystem access, block non-UNIX socket creation and io_uring, and use private temporary mounts for Office. Helper environment clearing and worker non-dumpability protect runtime credentials; outer watchdogs bound process-tree time/RSS. Read-only container roots, limited tmpfs, memory/PID caps and internal worker networking add independent boundaries.

Local IPC coverage depends on Landlock ABI. Older kernels can leave pathname/abstract UNIX sockets or signals less restricted. Native orchestration helpers retain filesystem/UNIX IPC access. Never mount a host Docker/SSH-agent socket or unrelated sensitive volume into a worker; isolate workers from credential-bearing same-user processes and metadata services. gVisor does not satisfy the Landlock required-mode contract merely by being a container runtime. See the existing [server sandbox requirements](../server.md).

The macOS hardening branch adds a deny-default Seatbelt profile with parameterized paths, finite executable/read/write grants, no network grant under deny-network, an enforcement probe and rlimits. Best-effort mode reports reduced isolation when Seatbelt is unavailable; required mode refuses it. Windows process-tree management and resource checks do not imply Linux-equivalent filesystem/network confinement. Local users choose their mode; cloud execution targets isolated Linux workers.

## Local MCP filesystem authority

The MCP process's OS account is more powerful than the tool client's permitted roots. Capability handles must preserve this distinction even when another actor can rename a shared directory. Main input and output paths retain handles; resources/read permits only session-produced outputs and rechecks current roots. Overwrite requires both server authorization and explicit tool intent.

Ancillary image resolution is an exception that currently violates this invariant: it reopens the original parent through ambient authority after main-input isolation and permits hidden path components. A shared-directory swap can redirect the read to a private image directory, and a crafted relative image reference can expose a hidden image. Preserve an authorized directory capability and the MCP path policy through ancillary reads, as required by the audit.

Image signatures limit this particular disclosure to supported raster formats; it is not a demonstrated arbitrary plaintext-file read. Resource downloads also currently check the 32 MiB limit by metadata before an unbounded read; a concurrently growing output can exceed that nominal limit. Streamed read bounds are preferable when files may change.

## Browser and desktop trust

The browser converter reads user-selected data into workers, downloads outputs as Blobs, and does not mount converted HTML into the application's DOM. The HTML writer escapes generated content and sanitizes raw HTML; allowed link schemes remain restricted. Standalone output can still contain external HTTP images/links, so conversion does not make an untrusted document trusted or guarantee offline viewing privacy.

COOP/COEP apply to the converter and compatible worker chunks. They do not replace CSP, frame restrictions or XSS prevention. The current header source lacks a CSP and general frame/nosniff/referrer policy; the web-security task owns that hardening and must verify both server and static-export delivery. Do not relax the converter's WASM/worker isolation to make headers pass a superficial check.

The service worker answers same-origin GETs and uses a converter-navigation fallback. It caches executable assets and binaries, not conversion uploads in the current path. Its cache message handler accepts same-origin URL lists without an authenticated-route allowlist or no-store check. Keep sessions, API responses, signed blob URLs and document data out of that cache when cloud UI is integrated. Current compose places web, auth and API on separate origins; the merged cloud UI still requires review.

Desktop deep links accept only route/profile preferences. OS file-open events are user file selections, not authorization for arbitrary writes or immediate cloud uploads. The audited desktop path does not process tokens from `ariadshift://auth/callback`; a future OAuth callback implementation must introduce and verify PKCE/state/keychain handling explicitly.

## Distribution and infrastructure

Pack executables are trusted code after installation. A manifest signature authenticates its reviewed publisher and pack/version; component size/SHA-256 checks authenticate bytes. Extraction stages privately, enforces entry/byte bounds, rejects traversal/special files and escaping links, and publishes only after verification. A valid publisher signature does not make an engine safe to run without a sandbox.

Toolchain source/wheel pins and hashes must be reviewed and committed. Rehashing a wheel against a build record stored beside that wheel proves consistency, not provenance against a compromised producer. Release refs, trusted cache scope, source-build keys, frozen package hashes and subsequent committed wheel pins are part of the trust chain. PR-created merge-ref caches and same-run artifacts must never be deliberately promoted across a privileged workflow boundary.

The compose default publishes only loopback web/auth/API/S3 ports; PostgreSQL and filer/master interfaces are internal. Required secrets have no usable embedded defaults. Auth/API/worker DB roles and bucket-scoped storage rights reduce consequences of a component compromise. Operator-provided values, firewall/proxy policy, key rotation, backups before migration and real DB grants must be verified at deployment time.

## Security acceptance

This document does not certify a v1 release. The [audit](../../plans/reports/security-audit-261025-v1.md) contains one open high finding and two medium findings. Release review requires closure of high findings with regressions, an explicit JWKS outage/revocation policy, dependency advisory evidence, actual Linux/macOS sandbox verification and review of the final integrated cloud UI and OAuth flow.
