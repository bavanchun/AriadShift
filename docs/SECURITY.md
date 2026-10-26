# Security

## Reporting a vulnerability

Report vulnerabilities through [GitHub private vulnerability reporting](https://github.com/bavanchun/AriadShift/security/advisories) ([Report a vulnerability](https://github.com/bavanchun/AriadShift/security/advisories/new)).
Do not open a public issue for a security report. Do not attach documents that
contain private data.

Include in a report:

- the affected component (`ashift` CLI, desktop app, web app, `ariad-server`, or engine pack), version, and operating system or deployment mode;
- minimal reproduction steps or a synthetic test input (never private or user documents);
- the observed behavior and security impact.

## Known limitations

The following known limitations and operator prerequisites are documented in `docs/release-notes.md`, [ARCHITECTURE.md §11.3](../ARCHITECTURE.md#113-isolation-layers), [server.md](server.md), and [install.md](install.md):

- the fixed transfer deadline on local `file://` blob storage (`min(15 min, signed URL lifetime: 15 min upload, 5 min download)` before `408 transfer_timeout`; use S3-compatible storage such as SeaweedFS for large files; see [server.md](server.md#object-storage-api-and-worker) and [#68](https://github.com/bavanchun/AriadShift/issues/68));
- unsigned v1 desktop installers, which rely on `SHA256SUMS` and Minisign `.minisig` signatures rather than Apple Developer ID or Windows Authenticode certificates ([install.md](install.md#2-checksum-and-minisign-verification); automated publication and code signing tracked in [#77](https://github.com/bavanchun/AriadShift/issues/77));
- platform or kernel sandbox gaps in `best-effort` mode, including Windows (Job Objects only, no filesystem or network sandbox), Linux kernels without Landlock ABI 9 for UNIX-socket confinement or user/mount namespaces for LibreOffice, gVisor lacking Landlock for `required` mode, and macOS Seatbelt's broad read rules under `/usr` and `/opt/homebrew` ([ARCHITECTURE.md §11.3](../ARCHITECTURE.md#113-isolation-layers), [server.md](server.md#container-runtime-support));
- missing reverse-proxy body-size limits in front of a public S3 upload endpoint, because S3 presigned `PUT` URLs bind `x-amz-checksum-sha256` rather than `Content-Length` ([server.md](server.md#data-lifecycle)).

Reports that show impact beyond a documented limitation (for example, an exploit that goes further than the documented behavior or a bypass of a stated mitigation) are welcome through the private reporting flow.

## Out of scope

The following generic categories are out of scope:

- denial of service that requires local administrator or root access to the machine running AriadShift;
- vulnerabilities in third-party engines or libraries themselves (Pandoc, Typst, LibreOffice, Docling, `rustc`, and so on), which should be reported to the upstream project, while a report about how AriadShift sandboxes, invokes, or fails to contain such an engine is in scope;
- findings that need an already compromised host or stolen credentials without a flaw in AriadShift;
- automated scanner output without a demonstrated impact.

## Supported versions

AriadShift follows SemVer. Releases are cut from `main`, and defects are fixed forward in the next release ([releasing.md](releasing.md), [git-workflow.md](git-workflow.md)):

| Version | Supported | Policy |
|---|---|---|
| latest 1.x | Yes (when `1.0.0` ships) | Security fixes ship in the next `1.x` release on `main` |
| latest 0.x | Until `1.0.0` ships | Active release line until `1.0.0` is published |
| older minor or patch lines | No | Fix-forward only; [releasing.md](releasing.md#failure-responses) defines no backports to older lines |
| engine packs (`docling`) | Latest compatible pack | Versioned separately (`pack-<name>-v<version>`); fixes ship as a new signed pack release ([packs.md](packs.md), [releasing.md](releasing.md#assets-owned-by-other-workflows)) |

## Surfaces and trust boundaries

The security model is defined in [ARCHITECTURE.md §11](../ARCHITECTURE.md#11-security-and-sandboxing):

- **Out-of-process engines and sandboxing.** Untrusted parsers run in isolated child processes over `ariad-engine/1` (`Pandoc`, built-in `PDF` and `Typst`, `LibreOffice`, and `Docling`), sandboxed with Landlock and seccomp on Linux (`best-effort` locally, `required` on Linux cloud workers) and Seatbelt (`sandbox-exec`) on macOS, with process-tree memory watchdogs and finite resource limits ([ARCHITECTURE.md §7](../ARCHITECTURE.md#7-engine-protocol), [cli.md](cli.md)). Native Markdown, HTML, IR JSON, and `DoclingDocument` JSON readers in `ariad-core` (plus the browser WASM PDF text reader) are pure Rust (`#![forbid(unsafe_code)]`), bounded, and fuzzed ([ARCHITECTURE.md §1](../ARCHITECTURE.md#1-architectural-principles), [§14](../ARCHITECTURE.md#14-quality-testing-and-benchmarking)).
- **Signed engine packs and release artifacts.** On-demand packs (`docling`) verify a detached prehashed Minisign signature (`pack.toml.minisig`, bound to `pack:<name>` and `version:<version>`) against compiled-in public keys and check each archive's size, SHA-256, and extraction bounds before unpacking ([packs.md](packs.md), [docling.md](docling.md)). Release verification is covered in [install.md](install.md) and [desktop.md](desktop.md).
- **Local MCP and browser isolation.** `ashift mcp` confines filesystem access and capability-based output promotion to `--allow-dir` and client roots, rejecting symlink escapes and hidden path components ([mcp.md](mcp.md#security-and-path-confinement)). The browser converter runs in a Web Worker over per-tab OPFS storage and verifies `pandoc.wasm` by SHA-256 ([web.md](web.md)).
- **Cloud API, remote MCP, and self-hosting.** `ariad-server` authenticates `/v1` via Better Auth EdDSA JWTs (JWKS), read-only API keys (`x-api-key`), or rate-limited anonymous quotas; binds S3 presigned uploads to SHA-256 or signs local `/v1/blobs/{token}` URLs with HMAC-SHA256; and serves remote MCP at `/mcp` over Streamable HTTP authenticated with OAuth 2.1 (dynamic client registration, S256 PKCE, `/mcp` audience) or API keys ([ARCHITECTURE.md §4](../ARCHITECTURE.md#4-product-surfaces), [api.md](api.md), [server.md](server.md)). Self-hosted worker, database-role, and network isolation guidance is in [server.md](server.md) and `docs/self-hosting.md`.
- **Internal v1 security review.** No external certification is claimed. The internal v1 source review (`plans/reports/security-audit-261025-v1.md` and `plans/reports/code-review-261025-phase7-final.md` on `fix/v1-security-audit`) audited the threat model, authentication/OAuth, uploads and signed URLs, local and remote MCP confinement, engine packs, sandboxes, parsers, dependencies, and web CSP headers. One high and two medium audit findings (concurrent local upload body buffering, local MCP sidecar image path resolution, and unbounded JWKS cache trust on refresh failure) plus two high and one medium pre-merge review findings (account-page CSP `connect-src`, slow-connection exhaustion of local blob transfer slots, and OpenAPI/SDK sync) were fixed in code, leaving zero open critical or high code findings and documented medium/low/informational follow-ups.

## Coordinated disclosure

Reports are handled privately through the GitHub security advisory flow while the issue is investigated and a fix is prepared. When a fix is ready, it ships in a release (or engine pack release) and the advisory is published with the release when appropriate.
