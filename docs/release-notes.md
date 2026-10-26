# Release notes

Changes in each AriadShift release, newest first. Installation commands and download tables for every version are on [GitHub Releases](https://github.com/bavanchun/AriadShift/releases). Each entry cites the pull request that merged it into `dev`.

## 1.0.0 (unreleased)

> **Coordinator: complete at merge time.** This entry describes the phase 4 to 8 branches (`feat/cloud-foundation`, `feat/cloud-product`, `feat/v1-hardening`, `fix/v1-security-audit` and `feat/v1-distribution`) as they stand before integration. After they merge into `dev`, add the pull request number to each bullet, as the 0.2.0 entry does, and drop every sentence that no longer matches the merged code. The `docling` pack range under Upgrade notes also needs a published pack whose `requires_ashift` covers 1.0.0.

AriadShift 1.0.0 completes the product. The self-hosted cloud (the `/v1` API, workers, accounts, API keys and the web cloud mode), remote MCP with OAuth, a macOS engine sandbox, file-manager integration on the desktop and the distribution pipeline join the CLI, the browser converter and the desktop app from earlier releases. The `ariad-engine/1` protocol, the `ariad-ir/1` document schema and the `/v1` API are frozen for the whole 1.x line.

### Highlights

- **Self-hosted cloud.** `docker compose` starts the web app, the `/v1` API, the Better Auth service, conversion workers, PostgreSQL 18 with `pgmq` and SeaweedFS object storage on one Linux host, with no external account. Profiles `core`, `office` and `docling` choose the routes. See [self-hosting.md](self-hosting.md).
- **A frozen `/v1` API.** `schemas/openapi.v1.json` is generated from the server code, guarded by a drift test, and compared with a frozen snapshot that fails on any breaking change. [api.md](api.md) states the compatibility and deprecation policy.
- **Remote MCP with OAuth.** `ariad-server api` serves a stateless Streamable HTTP endpoint at `/mcp` (MCP 2026-07-28). Clients sign in through an OAuth 2.1 flow served by the auth service, or send an API key. See [mcp.md](mcp.md#remote-mcp-over-http).
- **A web cloud mode and accounts.** `/app` can send a file to a server the operator runs, after an explicit choice. Account pages cover sign-in with passkeys and two-factor codes, API keys and the job list.
- **A TypeScript SDK.** `@ariadshift/sdk` is generated from the OpenAPI document and used by the web app.
- **A macOS engine sandbox.** Engines run under Seatbelt on macOS, and `ashift doctor` reports it.
- **Desktop file-manager integration.** "Convert with AriadShift" is available in Finder, Explorer, Dolphin and Nautilus, and the desktop window gained drag feedback, keyboard shortcuts and queue progress.
- **A security audit and its fixes.** The audit found one High and two Medium issues, all fixed, together with a strict Content Security Policy for the web app and nine new fuzz targets.
- **Unsigned desktop builds you can verify.** Every desktop installer ships with a `SHA256SUMS` manifest and Minisign signatures from the project key. See [install.md](install.md).

### Breaking changes

No `ashift` option, exit code or JSON output, no MCP stdio tool schema and no `ariad-ir/1` document changes incompatibly compared with 0.2.0. This release makes these contracts final:

- **`/v1` is frozen.** Within `/v1` the API changes only in backward-compatible ways: new paths and operations, new optional request fields and new response properties, headers, enum values and error codes. Removing or renaming anything, requiring a new field, narrowing a request type, making a response property optional or nullable, and tightening who may call an operation never happen inside `/v1`. A breaking change goes to a new `/v2` path next to `/v1`. A deprecated operation keeps working for at least six months, and `/v1` stays available for at least twelve months after `/v2` becomes stable. A security fix may tighten validation, and the release notes will say so. Clients must ignore unknown response properties and treat unknown enum values and error codes as a generic case. Plan values, the contents of `GET /v1/capabilities` and the form of presigned URLs are not part of the contract.
- **`ariad-engine/1` and `ariad-ir/1` stay frozen.** Neither changes in 1.x. A change that breaks them needs a new major wire version, and readers keep accepting `ariad-ir/0`.
- **Behaviour change: hidden and linked images in `ashift mcp`.** The local MCP server no longer embeds an image that a document references through a hidden file or directory (for example `.private/photo.png`) or through a symbolic link. The conversion succeeds and reports `image_not_embedded` for each such reference. Ordinary sibling images are still embedded, and `ashift convert` is unchanged. (The remote `/mcp` endpoint accepts no filesystem paths.)

### Upgrade notes from 0.1.0 and 0.2.0

- **Upgrading from 0.1.0.** Review the [0.2.0 upgrade notes](#upgrade-notes) below as well: IR JSON moved from `ariad-ir/0` to `ariad-ir/1` (readers still accept `ariad-ir/0`), EPUB identifiers derive from IR bytes, Typst is the default PDF writer, Linux engines run under Landlock and seccomp in `best-effort` mode, and `pandoc.wasm` is verified by SHA-256 in the browser.
- **CLI, desktop and browser users need no action from 0.2.0.** Existing IR JSON, scripts, exit codes and settings keep working.
- **macOS engines now run under Seatbelt.** In the default `best-effort` mode, `ashift` and the desktop app launch engines through `/usr/bin/sandbox-exec` with a deny-by-default profile. An engine that needs a path outside that profile is denied and the job fails with `sandbox_unavailable`. `ASHIFT_SANDBOX=required` now works on macOS when `ashift doctor` reports Seatbelt as usable. `ASHIFT_SANDBOX=off` restores the 0.2.0 behaviour. See [the CLI reference](cli.md#macos-engine-sandbox).
- **Docling packs are versioned with the host.** Packs published for 0.2.x declare `ashift >=0.2.0, <0.3.0`, so a 1.0.0 host reports them as `wrong version` and refuses them (exit 5). After upgrading, run `ashift engines install docling` to fetch the pack built for 1.x. Pack verification keys are unchanged.
- **Self-hosters: the server is now a supported product.** In 0.2.0 `ariad-server` was only in the source tree. Start from the [self-hosting guide](self-hosting.md): copy `.env.example` to `infra/compose/.env`, replace every placeholder secret, and run the compose profiles you need. Database migrations are not reversible, so back up the database before every upgrade. Compose runs `migrate` and `db-grants` on each `up`, and the API and workers refuse to start against a database whose migrations do not match their binary.
- **The auth service needs the API's public URL and HTTPS.** `ARIAD_PUBLIC_URL` is required by `services/auth`, and `BETTER_AUTH_URL` and `ARIAD_PUBLIC_URL` must both use `https` unless their host is a loopback host. The service refuses to start otherwise. If `ARIAD_PUBLIC_URL` changes, OAuth clients registered earlier must register again.
- **Web builds are cloud-free unless configured.** Without `NEXT_PUBLIC_ARIAD_API_URL` at build time, `/app` stays the static, offline converter with no cloud code. Web hosts now need to send the security headers: Netlify reads `out/_headers`, and the web image serves a generated nginx configuration. Other servers must copy the per-page policies from `out/_headers` ([web.md](web.md#security-headers)). Cloud builds also need `ARIADSHIFT_WEB_STORAGE_ORIGINS` so the page may reach the object store.
- **The offline service worker no longer caches account or API traffic.** `/api/auth/`, `/v1/` and presigned document URLs are never stored, and on activation the worker deletes any such entry an earlier worker kept.
- **Desktop installers are unsigned.** The first launch needs the Gatekeeper or SmartScreen override described in [install.md](install.md#3-desktop-installation-and-first-run-guide). In-app updates are still verified with the updater key.

### Added

#### CLI

- **macOS engine sandbox.** Engines run under a generated Seatbelt profile that denies access by default. They read their installation and declared runtime paths, system libraries and fonts, and write only their job workspace and `/dev/null`. IP networking, including loopback, and UNIX sockets are denied. The profile is checked in a disposable child before an engine starts, and a profile error fails the job instead of retrying outside the sandbox. `ashift doctor --json` adds `sandbox.seatbelt_available`, `sandbox.seatbelt_usable` and `sandbox.seatbelt_enforced` on macOS.
- **Nine more fuzz targets** cover archive preflight, DOCX package metadata, the engine protocol, capabilities JSON, URI parsers, image asset resolution, pack manifests, pack archives and version-checked IR JSON. They run in CI, and the nightly AddressSanitizer run now uses one job per target.

#### Web

- **Cloud mode in `/app`.** Every file starts **On this device**. The **Where to convert** choice names the server, its retention period and what it stores, and nothing leaves the device until the user presses **Upload and convert**. The browser hashes the file with SHA-256 in a worker (a 2 GiB file is never held in memory), uploads it straight to the object store with one presigned `PUT`, follows the job over its event stream with reconnects, and checks the size and hash of the download. **Delete now**, **Delete after download** and **Cancel** remove the upload or job from the server. When a deletion fails, the file stays listed with its retention deadline and **Delete from server** retries.
- **Plan limits before upload.** When the API offers `GET /v1/limits`, the panel shows the plan's limits and refuses an oversized file or a spent quota, and a `429` shows a countdown from `Retry-After`. Anonymous uploads use a Cloudflare Turnstile widget when the build has a site key.
- **Account pages.** `/sign-in`, `/account`, `/api-keys` and `/jobs` need `NEXT_PUBLIC_ARIAD_API_URL` and `NEXT_PUBLIC_ARIAD_AUTH_URL`. Sign-in offers email and password, passkeys and the two-factor step. The account page changes the name and password, manages passkeys and two-factor codes, signs out and deletes the account. The API keys page shows a new key once. The jobs page lists the account's jobs from `GET /v1/jobs`, with Download, Delete now and Retry.
- **Security headers.** Every route gets a Content Security Policy in one of three profiles (site pages, account pages, converter) plus `Referrer-Policy`, `X-Content-Type-Options` and `Permissions-Policy`. See [Known limitations](#known-limitations) for the one open browser check.

#### Desktop

- **"Convert with AriadShift" in the file manager.** It covers every supported input format except JSON. macOS declares Open With document types and ships a Quick Action, Windows adds a per-user Explorer verb, and Linux installs desktop-entry MIME types, a Dolphin service menu and a Nautilus script. Files opened from the OS, on a cold launch or from a second launch, are added to the Convert selection, at most 1,000 per launch. The registrations are generated from one `formats.json` that a test keeps equal to the capability graph. See [desktop.md](desktop.md#os-integration).
- **Interaction polish.** Dragging files shows how many documents will be added and how many items are skipped. Keyboard shortcuts switch tabs and start or cancel work, and every control carries `aria-keyshortcuts`. The queue shows a live summary, batch progress, failed jobs first, status badges and **Retry failed**. Completion toasts report jobs that finish on another tab, empty states say what to do next, Settings tracks unsaved changes, and motion follows the reduced-motion preference.

#### Cloud and self-hosting

- **Compose stack.** `infra/compose/docker-compose.yml` runs PostgreSQL 18 with `pgmq`, SeaweedFS with its S3 gateway, a one-shot database and bucket setup, the migrations, the Better Auth service, the API, one worker per pack and the web app. Start order follows health checks, published ports bind to `127.0.0.1` by default, and `docker-compose.gvisor.yml` runs the workers under gVisor. The guide covers backups, restores, upgrades and troubleshooting. A compose end-to-end workflow (`cloud-e2e.yml`) exercises the stack.
- **`/v1` API.** Upload, complete, plan, create job, event stream, outputs and delete. New in this release: `GET /v1/jobs` (the account's jobs, newest first, with opaque keyset cursors; anonymous callers get `401`), `GET /v1/limits` (the caller's plan, limits, retention and quota use) and `DELETE /v1/uploads/{id}`. Callers use a 15-minute JWT from the auth service, an `x-api-key`, or no credential under the small `anonymous` plan.
- **Auth service.** `services/auth` is Better Auth on Hono with its own `auth` schema. It provides email and password accounts, GitHub and Google sign-in when configured, passkeys, two-factor codes, 15-minute EdDSA JWTs with the public keys at `/api/auth/jwks`, API keys and self-service account deletion.
- **Plans and quotas.** The seeded `anonymous` plan allows 10 MiB files, and the `free` plan 2 GiB. Both keep inputs and outputs for one hour, and jobs, bytes, concurrency and requests per minute are metered per workspace or client address. Edit `plan_entitlements` to change them.
- **Local blob transfers.** With `file://` storage the API serves its own signed transfer URLs under a per-process budget (`ARIAD_BLOB_MAX_TRANSFERS`, default 64), a per-workspace share (`ARIAD_BLOB_MAX_TRANSFERS_PER_WORKSPACE`, default 8) and a minimum average rate (`ARIAD_BLOB_MIN_RATE_BPS`, default 16 KiB/s). The compose stack uses S3 and is unaffected.
- **Published images.** A release tag pushes `ariadshift-api`, `ariadshift-worker-core`, `ariadshift-worker-office`, `ariadshift-worker-docling`, `ariadshift-auth` and `ariadshift-web` to GHCR for `linux/amd64` and `linux/arm64`, with SBOM and provenance attestations and a keyless Cosign signature.

#### MCP

- **Remote MCP at `/mcp`.** The endpoint is stateless Streamable HTTP for MCP 2026-07-28. It needs `ARIAD_PUBLIC_URL`, shares workers, retention and quotas with the REST API, and offers `list_engines`, `plan`, `inspect`, `convert` and `job_status`. Documents arrive as a completed `upload_id` or as inline Markdown or HTML of at most 256 KiB, and the endpoint accepts no filesystem paths or input URLs. Request bodies are limited to 1 MiB, and `Host` and `Origin` are checked exactly.
- **OAuth 2.1 sign-in.** The auth service is the authorization server for `/mcp`: RFC 9728 protected-resource metadata, RFC 8414 discovery, RFC 7591 dynamic client registration, authorization code with mandatory S256 PKCE, and RFC 8707 resource binding. It issues 15-minute Bearer JWTs whose audience is the `/mcp` resource, plus a 30-day refresh token for `offline_access`. It serves its own sign-in and consent pages. `ariad-server` accepts these tokens at `/mcp` only and the REST token at `/v1` only.
- **API keys for headless agents.** The same keys as `/v1`, sent in `x-api-key`. Anonymous REST access does not enable anonymous MCP access.
- **Local MCP.** The `ashift mcp` stdio server and its four tools are unchanged apart from the image rule under Breaking changes.

#### SDK

- **`@ariadshift/sdk`.** Operations and types are generated from `schemas/openapi.v1.json` by `@hey-api/openapi-ts`. A hand-written layer adds credentials, the upload flow with incremental SHA-256 hashing in 8 MiB slices, event streams that resume with `Last-Event-ID`, `listJobs`, and downloads that verify size and hash. `just sdk-check` fails when the generated client drifts from the document. The package lives in the workspace and is used by `apps/web`. See [api.md](api.md#typescript-sdk).

### Security

- **Audit result.** The v1 audit ([threat model](security/threat-model.md)) reported one High and two Medium findings, and the final review of the integrated branch found no open critical or high issue in code.
  - **High:** replaying one local upload URL could pin an unbounded number of buffered request bodies in the API. Local transfers now take an admission slot before any writer exists, stream into bounded staging, apply idle, rate and total-time limits, and release their slot on every exit path. A follow-up closed the cheaper attack in which slow connections held every slot, with the per-workspace share and the minimum rate.
  - **Medium:** `ashift mcp` resolved sidecar images through an ambient path, so a swapped directory or a hidden file could leak an image into the output. It now resolves them through the confined input's directory handle and refuses hidden components.
  - **Medium:** a failed JWKS refresh extended trust in a removed signing key without a deadline. A cached key is now trusted for at most 15 minutes after the last successful fetch, then the API fails closed with `503`.
- **Content Security Policy.** Pages run no `eval`, inline scripts are pinned by hash in the static export, and the pages cannot be framed. The converter alone may compile WebAssembly and start workers, and only cloud builds list the API, auth and object-store origins. The account pages were missing those origins in the first draft and now have their own profile.
- **Dependencies.** Overrides pin fixed versions of `js-yaml` and `body-parser`, Pillow moved to 13.0.0, and every `cargo-deny` advisory exception records its review date. [The dependency policy](security/dependency-policy.md) describes the process.
- **Compose isolation.** Each service has its own database login. The API and workers own no tables and cannot read Better Auth's users, sessions or signing keys. Workers sit on an internal network with no route out, run with a read-only root and dropped capabilities, and start the engine sandbox in `required` mode.
- **Remote MCP boundaries.** A token for `/v1` is rejected at `/mcp` and the reverse. The endpoint checks `Host` and `Origin`, rate-limits every request including discovery, and serves `Cache-Control: no-store`.
- **No client-credentials grant.** The authorization server offers only `authorization_code` and `refresh_token`, so every token belongs to a signed-in user and workspace.

### Distribution and install

[install.md](install.md) is the full guide. Channels:

- **CLI.** GitHub Release archives for five targets with `.sha256` sidecars and SBOMs, the shell and PowerShell installers, a Homebrew formula (`brew install bavanchun/tap/ashift`) generated and checked against the archive hashes on every release, and optional crates.io and WinGet channels. A release publishes to crates.io and the tap only when their tokens exist, and skips them with a notice otherwise.
- **Desktop.** `.dmg` (Apple silicon only), NSIS `.exe` and `.msi` for Windows x64, and `.AppImage`, `.deb` and `.rpm` for Linux x86_64. **v1 desktop builds are not code-signed or notarized.**
- **Self-hosted cloud.** The compose stack and the GHCR images above. Verify an image with `cosign verify --certificate-identity <workflow ref> --certificate-oidc-issuer https://token.actions.githubusercontent.com ghcr.io/bavanchun/ariadshift-api@<digest>`. The release workflow prints the exact command with the digest.

**Verify an unsigned desktop installer before you open it.** Each desktop release attaches `SHA256SUMS`, `SHA256SUMS.minisig` and a `.minisig` for every installer and updater artifact. The project key is `D87B0E33F55EA02B` (`RWQroF71Mw572E0d5K9ndPm5/Uf94TRU99qOHfm3ZCNmB7CebJkjmim/`), the same key that signs engine packs.

```sh
minisign -Vm SHA256SUMS -P RWQroF71Mw572E0d5K9ndPm5/Uf94TRU99qOHfm3ZCNmB7CebJkjmim/
sha256sum -c --ignore-missing SHA256SUMS      # macOS: shasum -a 256 -c --ignore-missing SHA256SUMS
```

The trusted comment of each signature reads `desktop:AriadShift release:v<version> file:<name>`, and `ariad-host` refuses it as an engine pack signature. After verifying, allow the first launch: on macOS use **System Settings › Privacy & Security › Open Anyway**, and on Windows use **More info › Run anyway** in SmartScreen. The guide explains both, and why `xattr -dr com.apple.quarantine` should wait until after verification. Files ending in `.sig` are Tauri updater signatures that the app checks itself. Apple and Windows signing stay wired in the workflow, turn on only when their whole secret group exists, and fail the release when a group is partial.

### Known limitations

- **Local `file://` storage transfers have a deadline (R4).** The API signs and serves `/v1/blobs/{token}` itself, and each transfer must finish within the shorter of 15 minutes after admission and the signed URL's remaining lifetime (15 minutes for an upload, 5 for a download). A 2 GiB upload needs about 2.4 MB/s from the moment its URL is issued, and a slower one fails with `408 transfer_timeout`. The API logs a warning at startup when it uses `file://`. Use S3-compatible storage, which the compose stack provides through SeaweedFS, for large files.
- **No code signing yet.** Desktop installers carry no Apple Developer ID, notarization or Authenticode signature, so Gatekeeper and SmartScreen warn on first launch, and Windows 11's top-level context menu cannot show the Explorer verb. Checksums and Minisign signatures are the substitute.
- **OAuth uses dynamic client registration only (DCR).** Client ID Metadata Documents, which MCP 2026-07-28 prefers, are planned (CIMD).
- **No OAuth sign-up or consent-revocation UI yet.** `/oauth/sign-in` signs in existing accounts only. Create accounts through the web app, `/api/auth/sign-up/email` or social sign-in. Users cannot list or revoke the OAuth clients they approved, although the provider has the endpoints.
- **Node.js stays on 24 LTS until Node 26 enters LTS.** `.node-version` (`24.21.0`) and `@types/node` (`24.19.2`) remain on Node 24 LTS for 1.0.0 because Node 26 enters Active LTS on 2026-10-28. Runtime and type definitions will move to Node 26 LTS together after that date.
- **Cloud web app session, account-deletion and download limits.** Transient token-refresh errors in `CloudAuth.bearer()` and the account adapter currently fall back to an anonymous request; the account deletion form always requires a password (`deleteUser({ password })`) and has no passwordless recent-session flow for social-only OAuth users; and web downloads buffer the complete output as a `Blob` in browser memory.
- **macOS desktop is Apple silicon only.** Intel Macs run the CLI and the browser converter.
- **The Seatbelt profile reads broadly.** It permits all of `/usr` and `/opt/homebrew`, so a hostile engine could read service data stored there into its output even though networking is denied. Apple deprecates `sandbox-exec`, so `doctor` checks that it works.
- **Turnstile under cross-origin isolation is unchecked.** `/app` sends `Cross-Origin-Embedder-Policy: credentialless`, and Turnstile's iframe may be blocked there. If so, anonymous cloud uploads cannot start, while signed-in uploads and on-device conversion still work.
- **Remote MCP expects HTTP/1.1 and the canonical `Host`.** HTTP/2 requests with only an authority and explicit default ports are refused, so keep a proxy that preserves the public `Host`. Only clients that implement MCP 2026-07-28 are supported.
- **Windows-only gaps in local MCP image confinement.** A junction swap between open and check is not detected, and 8.3 short names bypass the hidden-name check. Both reach only files inside an authorized directory.
- **Upload completion re-hashes without an in-flight guard.** Parallel completions of one pending upload each read up to 2 GiB. Memory stays bounded, but CPU and I/O do not.
- **Accounts need no email verification,** because the self-hosted stack has no mail transport. Anonymous upload and job ids are bearer capabilities. Event streams are authorized when they open, not continuously.
- **Deployments with several API replicas need ingress limits.** The S3 upload signer cannot bind the content length, so put a body cap on the route to object storage, as [self-hosting.md](self-hosting.md#going-beyond-localhost) describes.
- **The Linux sandbox depends on the kernel.** Landlock's UNIX-socket mediation needs ABI 9, and LibreOffice needs user and mount namespaces. gVisor cannot run `required` mode.
- **Uploads are one request per file,** so the cloud input cap stays at 2 GiB.
- **Carried over from 0.2.0:** Pandoc stays external for the CLI and desktop, the web export does not fit Cloudflare Pages, the `docling` pack has no Intel macOS or musl build, and the first DOCX or EPUB conversion in the browser downloads about 61 MB.

### Roadmap for 1.x

- Client ID Metadata Documents for MCP clients, once the auth service's outbound fetch policy is decided.
- An OAuth sign-up page and a screen to list and revoke approved clients.
- Size-aware transfer deadlines for `file://` storage, so large local uploads stop depending on a fixed 15 minutes (R4).
- Code-signing and notarization certificates, with the Windows 11 top-level Explorer menu that needs a signed package, and automated `crates.io` (`CARGO_REGISTRY_TOKEN`) and `bavanchun/homebrew-tap` (`HOMEBREW_TAP_TOKEN`) publication once tokens exist.
- An Intel macOS desktop build.
- A narrower Seatbelt read scope, verified against real Pandoc, Typst and LibreOffice runs on macOS.
- Security audit low and informational follow-ups: a per-upload in-flight guard on completion hashing (R5), fixes for the Windows MCP image confinement gaps (R6), an asset-handle parameter on `convert_confined` when callers need confined image extraction (R7), and a Chromium check of Cloudflare Turnstile inside cross-origin-isolated `/app` (R8).
- Cloud web app follow-ups: distinguish transient `5xx` token-refresh failures from unauthenticated state so signed-in sessions do not fall back to anonymous requests, add a recent-session passwordless account-deletion path for social-only OAuth users, and stream large cloud downloads instead of buffering the full `Blob` in browser memory.
- Web E2E and runner hardening: extend the `ENGINE_BOOT` (`device-indicator`) readiness wait in `apps/web/e2e/converter.spec.ts` to the remaining direct fresh-page file-pick and `device-indicator` assertions, and install native Playwright WebKit system dependencies on the self-hosted Linux CI runner.
- Migrate `.node-version`, CI/Docker Node pins and `@types/node` together to Node 26 LTS after 2026-10-28.
- Add a supported-versions statement to [`docs/SECURITY.md`](SECURITY.md) and split [`ARCHITECTURE.md`](../ARCHITECTURE.md) into focused topical documents once all v1 branches merge.
- Desktop notifications, a native menu bar, window-state restore, a recoverable **Clear finished** and Dock or taskbar progress.
- S3 multipart uploads, and a NATS queue adapter only if measured throughput exceeds PostgreSQL.
- Support for Cloudflare Pages through a different engine delivery.
- Out of scope for 1.0 and not scheduled: audio and video, an `ocr-vlm` pack, billing and a managed public deployment.

## 0.2.0 (unreleased)

Roadmap 1b. `ashift` adds PDF, Office, OCR and image routes, signed engine packs, and a Linux engine sandbox. This release also adds the desktop app and the in-browser converter. The `ariad-engine/1` engine protocol and the `ariad-ir/1` document schema are now frozen.

### Upgrade notes

- **IR JSON is now `ariad-ir/1`.** `--to json`, the MCP server and the browser write `"version": "ariad-ir/1"`, described by [`schemas/ir.v1.json`](../schemas/ir.v1.json). Readers still accept `ariad-ir/0` and upgrade its version on output, and `schemas/ir.v0.json` remains available. Tools that check the version string must accept `ariad-ir/1`. Unknown or missing versions are rejected. (#49)
- **EPUB identifiers change.** EPUB identifiers and the table-of-contents checksums that depend on them are derived from the IR bytes, so a given input now gets different identifiers than it did in 0.1.0. Repeated conversions are still byte-identical. (#49)
- **Typst is the default PDF route.** Markdown, HTML, EPUB and IR JSON always write PDF through the built-in Typst writer, in every profile, even when LibreOffice is installed. LibreOffice renders PDF only from the user's own Office or ODF file, as the first step of a route. (#34, #49)
- **Linux engines run sandboxed by default.** Out-of-process engines run under Landlock and seccomp in `best-effort` mode. Set `ASHIFT_SANDBOX` to `off`, `best-effort` or `required`; unknown values fail closed, and `required` works on Linux only. Without kernel support, `best-effort` prints `warning[sandbox_unavailable]` and `required` exits with code 5. (#33)
- **LibreOffice on Linux needs user and mount namespaces.** It runs with a private `/tmp`. If namespace setup is denied, its routes fail with `sandbox_unavailable` (exit 5) in every mode except `off`. On Ubuntu, either allow `userns` for `ashift` in AppArmor or run `sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0`. Default Docker and Kubernetes seccomp profiles also block the setup. See [the CLI reference](cli.md#ashift-convert). (#34)
- **Exit codes cover more cases.** Exit 5 now also reports unavailable required sandbox isolation, a missing Docling engine or Tesseract, and a pack built for a different `ashift` version. Exit 4 now also covers the engine process-tree memory limit and oversized pack archives. [cli.md](cli.md) lists how pack failures map to codes 1 to 5. (#33, #35, #38)
- **Engine packs are versioned with the host.** Docling packs for this release require `ashift` `>=0.2.0, <0.3.0`, and installs fail with exit 5 on any other version. Packs are verified against the signing keys compiled into `ashift`. (#35, #38, #40)
- **The browser checks `pandoc.wasm` against a pinned SHA-256.** Self-hosted web exports must serve the unmodified official Pandoc 3.12 `pandoc.wasm`. The sync script refuses any other binary. A cached or downloaded copy that does not match is evicted, and the conversion stops with an `engine_integrity` error. (#48)

### Added

- **Many inputs in one `convert` call.** With more than one input, `-o` names an existing output directory, and duplicate destinations are refused before anything converts (exit 2). `--json` prints a `results` array in input order. A failed input does not stop the later ones, and the exit code is that of the first failure. Single-input behaviour is unchanged. (#33)
- **New input formats:** PDF (built-in text reader), DOC, XLS, PPT, RTF, ODT, ODS, ODP, PPTX, XLSX, PNG, JPEG, TIFF, WebP and DoclingDocument JSON (`*.docling.json`). **New targets:** `pdf`, `json` (IR JSON), and `xlsx` and `pptx` where a route reaches them. (#34, #38)
- **Built-in PDF reader and Typst PDF writer.** The reader extracts text from digital PDFs. The writer embeds Vietnamese-capable fonts and supports headings, lists and task lists, tables with spans, code, quotes, figures and footnotes. TeX math is kept as text with a `math_not_converted` warning. (#34)
- **Automatic PDF reader selection.** `ashift` checks whether a PDF has a text layer. Digital PDFs use the built-in reader; PDFs that need OCR select Docling. Without Docling, a scanned PDF fails with exit 5 and an installation hint. (#34, #38)
- **`--engine <ENGINE>`** on `convert` and `plan` chooses the input reader, for example `pdf` or `docling`. The requested reader is never silently swapped for another one. (#34, #38)
- **Optional system LibreOffice `>=24.2`** for DOC/RTF/ODT → DOCX, XLS/ODS → XLSX, PPT/ODP → PPTX, and Office/ODF → PDF. Each job uses a fresh profile with macros disabled. (#34)
- **Docling engine pack** (`ashift engines install docling`) for OCR and layout-aware PDF, DOCX, PPTX, XLSX and image extraction. A bounded, fuzzed Rust mapper converts its output to IR. OCR uses the system Tesseract 5 (`vie+eng`), or RapidOCR when every language hint is a non-Vietnamese language that RapidOCR covers. The pack works offline and never downloads anything during a conversion. See [docling.md](docling.md). (#38, #43, #46)
- **Signed engine packs.** `ashift engines install`, `remove` and `verify` check a minisign signature, then each component's size and SHA-256. Interrupted downloads resume, and `--from <DIR>` installs offline. `engines`, `doctor` and MCP `list_engines` report installed packs. See [packs.md](packs.md). (#35)
- **More `inspect` output.** For PDFs it reports page count, text and image-only pages, `needs_ocr`, encryption and producer. It also detects OOXML, ODF, OLE2, RTF and image formats from their content. (#34)
- **Sandbox and pack checks in `doctor`.** It reports the engine sandbox, the pack data directory, trusted signing keys, every installed pack, LibreOffice and the Docling engine. (#33, #35, #38)
- **Warm engine sessions.** Engines that support sessions stay warm across the inputs of one batch, and MCP `convert` calls share one session pool. Idle sessions close after five minutes, and engine processes retire after 100 jobs. (#33)
- **Process-tree memory watchdog.** `max_memory_mb` now caps each engine's whole process tree, on Linux, macOS and Windows. (#33)
- **Clearer `plan` failures.** When an engine was found but failed to start, `plan` prints `startup_failure (<engine>): <cause>`. The plan JSON is unchanged. (#34)
- **Desktop app (new).** A Tauri 2 app for macOS, Windows and Linux that converts through `ariad-host` on the device, so documents are never uploaded. The GitHub Release carries the installers: `.dmg` for macOS, `.msi` and NSIS `.exe` for Windows, and AppImage, `.deb` and `.rpm` for Linux. See [desktop.md](desktop.md). (#40, #50)
  - The conversion queue runs 1 to 4 jobs at once, with cancel, retry, and an explanation of the route before converting.
  - Up to 16 watch folders, polled with a stable-file check. Subfolders are optional. Outputs must be outside the watched folder, which prevents conversion loops. Watches run only while the app is open.
  - Engine packs can be installed, verified and removed in the app, with progress and cancellation, under the same signature policy as the CLI. Diagnostics runs the host `doctor`.
  - Links of the form `ariadshift://open?route=<from>-<to>&profile=<profile>` preselect a route and a profile. They never carry file paths.
  - In-app updates come from GitHub Releases and are verified against the updater public key. An update is never installed without the user's confirmation.
- **Web app (new).** The website and the in-browser converter live in `apps/web`. Conversions run on the device through `ariad-wasm`. Files stay in per-tab private browser storage (OPFS), with a 64 MiB in-memory fallback where OPFS or Web Locks are unavailable. See [web.md](web.md). (#36, #41, #42, #48)
  - Markdown, HTML and IR JSON convert with the core module. (#36)
  - DOCX and EPUB are read and written through the official `pandoc.wasm` 3.12 reactor. Digital PDF text is read in the browser, and PDF output uses the Typst compiler with embedded fonts. (#48)
  - Heavy engines download on first use, with progress, and stay cached for offline use. When a new build activates, the service worker deletes retired engine downloads. The landing page loads no WebAssembly. (#48)
  - Inputs over 16 MB, and routes that need native engines such as OCR, offer **Open in Desktop**. The link carries the route and profile, never the file. (#36, #48)
- **Library additions.** `ariad-core` adds an in-memory conversion pipeline with inspection, the PDF reader, Typst markup output, the DoclingDocument mapper, and warm-session fields in the engine protocol. `ariad-host` adds the engine registry, the session pool, the Linux sandbox, the memory watchdog, caller-supplied limits and signed pack installation. The new `ariad-typst` crate is the in-memory Typst compiler shared by native builds and the browser, and is published to crates.io with the other library crates. The new `ariad-wasm` crate provides browser bindings for `version`, `capabilities`, `inspect`, `plan` and `convert`. (#33, #34, #35, #36, #37, #38, #48)

### Changed

- **Frozen contracts.** `ariad-engine/1` and `ariad-ir/1` are frozen. Shared conformance tests cover Pandoc, PDF, Typst, LibreOffice and the real Docling engine. (#49)
- **Measured route scores.** Route scores for PDF, OCR and Office now come from English and Vietnamese benchmark inputs instead of unmeasured defaults; see [bench-report.md](bench-report.md). Automatic PDF reader selection still uses the built-in reader for digital text and Docling for scans. (#49)
- **Embedded Markdown images.** Markdown `data:` images now become embedded IR assets, so they survive conversion to DOCX and EPUB. No resource URLs are fetched. (#48)
- **`ashift engines` output.** Human output adds a pack section after the engine table. JSON rows backed by a pack carry a `pack` object, whose `status` can be `ready`, `missing`, `wrong version` or `not installed`. MCP `list_engines` returns the same `pack` object. (#35)
- **No Visual C++ Redistributable for `ashift`.** The Windows release binary now links the C runtime statically. (#46)
- **Updated Rust dependencies,** including comrak 0.56.0 and saphyr-parser 0.2.2. (#31)

### Fixed

- In-memory inspection now analyses only complete inputs. (#36)
- Each pack's staging directory now has its own name, so cleaning up one pack never deletes another pack's staged files. (#35)
- Windows Docling installs now print the Visual C++ runtime prerequisite, and Docling import failures on Windows now name their cause. (#43, #46)

### Security

- **Linux engine sandbox.** Out-of-process engines run under Landlock and seccomp. Incomplete network isolation and local socket limits are reported instead of being silently accepted. (#33)
- **Engine artifact reads are confined** to each engine's own output directory. (#33)
- **Pack verification.** Packs are verified as a whole against a pinned maintainer public key, and cached downloads are isolated from one another. (#35, #40)
- **Tesseract confinement.** Sandboxed engines can read Tesseract installation data and shared data only. (#43, #46)
- **IR JSON limits.** IR JSON reads now apply the input byte cap, and empty containers and tables count toward the nesting limit. Unknown limit names in `ariad-wasm` convert options are rejected. (#33, #36)
- **Desktop confinement.** Native desktop commands are limited to the main window capability, and per-job batch request storage is bounded. (#40)
- **Browser engine integrity.** `pandoc.wasm` is verified against its pinned SHA-256 before it is cached or compiled. The service worker serves only hashed assets cache-first. (#36, #48)

### Docling pack without Intel components

> **Coordinator: verify at merge time.** This entry describes #51, which is still open. Keep it only if #51 merges before the `v0.2.0` tag, and edit it to match the merged change. Otherwise delete this section and the matching sentence under Known limitations.

- x86_64 Docling packs no longer contain proprietary Intel IPP, MKL or Intel OpenMP. OpenCV is rebuilt without IPP, x86_64 CPU PyTorch is built from source with OpenBLAS and oneDNN, and Windows uses LLVM OpenMP. These toolchain wheels are built in this repository's CI and pinned by size and SHA-256. ARM packs keep the audited official wheels. (#51)

### Known limitations

- **Unsigned desktop builds.** Desktop installers are not code-signed for macOS or Windows. Gatekeeper blocks the first launch of the downloaded macOS app until the user allows it. Updater artifacts are still signed and verified. (#40)
- **No Intel macOS desktop build.** macOS installers and in-app updates support Apple silicon only. (#40)
- **Docling pack platforms:** `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `aarch64-apple-darwin` (macOS 14 or later) and `x86_64-pc-windows-msvc`. There is no Intel macOS pack and no musl Linux pack. CI builds the `aarch64` Linux pack but does not run it. On Windows, the pack needs the latest x64 Microsoft Visual C++ Redistributable. Every platform needs Tesseract 5 on `PATH` for OCR, but digital documents convert without it. (#38, #43, #46)
- **The sandbox is Linux-only.** macOS and Windows keep process groups and Job Objects, and `ASHIFT_SANDBOX=required` fails on them. (#33)
- **Pandoc stays external** (`>=3.12, <4`) for DOCX and EPUB in the CLI and the desktop app. (#34, #40)
- **Browser limits.** The first DOCX or EPUB conversion downloads about 61 MB for `pandoc.wasm`. Inputs over 16 MB, OCR, legacy Office and LibreOffice routes need the CLI or the desktop app. The static web export does not fit the Cloudflare Pages 25 MiB file limit. Netlify, GitHub Pages (which serves it without cross-origin isolation) and Node `next start` are supported. (#36, #48)
- **No cloud service.** The web app has no cloud mode. The `ariad-server` crate (the `/v1` HTTP API and conversion workers) is in the source tree but is not packaged or supported in this release. The self-hosted cloud product is planned for 1.0.0. (#36, #37)
- **Package channels.** 0.1.0 was not published to Homebrew, crates.io or WinGet. A channel counts as confirmed for 0.2.0 only after the release smoke workflow passes on it.

## 0.1.0 (2026-10-21)

Roadmap 1a: Markdown, HTML, DOCX and EPUB conversion through `ariad-ir+json`, the `convert`, `inspect`, `plan`, `engines` and `doctor` commands, and the `ashift mcp` server. See the [v0.1.0 release](https://github.com/bavanchun/AriadShift/releases/tag/v0.1.0).
