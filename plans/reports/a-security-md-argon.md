# Worker report: a-security-md (`docs/SECURITY.md`)

Branch: `docs/v1-security-policy` (cut from `origin/feat/v1-distribution` at `88c52eb7aa4d38a1fd8776200793a9997f6f41ad`).
Worktree: `/home/vchun/Codes/01-AriadShift-wt-sec-md`.

## Summary of work

Extended `docs/SECURITY.md` (47 lines, well below the 80-line limit) with a factual security policy covering private vulnerability reporting, out-of-scope documented limitations, SemVer and engine-pack version support, surfaces and trust boundaries, and coordinated disclosure.

## Claims and verification sources

| Section / Claim | Repository source |
|---|---|
| Report via GitHub private vulnerability reporting (`/security/advisories` and `/security/advisories/new`); no public issues; no private documents; include affected component, version, reproduction, and impact | Existing `docs/SECURITY.md`; repository setting verified (`gh api repos/bavanchun/AriadShift/private-vulnerability-reporting` enabled) |
| Out of scope: local `file://` blob transfer deadline (`min(15 min, signed URL lifetime: 15 min upload, 5 min download)` before `408 transfer_timeout`; use S3/SeaweedFS for large files) | `origin/feat/v1-distribution--t5:docs/release-notes.md` ("Known limitations" and "Local `file://` storage transfers"), `origin/fix/v1-security-audit:plans/reports/worker-7-r4-opus55.md`, `docs/server.md` ("Object storage (`api` and `worker`)") |
| Out of scope: unsigned v1 desktop installers relying on `SHA256SUMS` and Minisign `.minisig` signatures rather than Apple Developer ID or Windows Authenticode certificates | `ARCHITECTURE.md` §16 and §18 ("Code signing", "Desktop release trust"), `docs/install.md` §2–§3, `origin/feat/v1-distribution--t5:docs/release-notes.md` ("Known limitations") |
| Out of scope: platform/kernel sandbox gaps in `best-effort` mode (Windows Job Objects only; Linux kernels lacking Landlock ABI 9 for UNIX sockets or user/mount namespaces for LibreOffice; gVisor lacking Landlock for `required` mode; macOS Seatbelt broad read scope under `/usr` and `/opt/homebrew`) | `ARCHITECTURE.md` §11.3 ("Isolation Layers") and §18 ("macOS engine sandbox"), `docs/server.md` ("Container runtime support"), `origin/feat/v1-distribution--t5:docs/release-notes.md` ("Known limitations") |
| Out of scope: missing reverse-proxy body-size limits in front of a public S3 upload endpoint (`x-amz-checksum-sha256` bound, not `Content-Length`) | `ARCHITECTURE.md` §9.4 ("Object Storage"), `docs/server.md` ("Data lifecycle"), `origin/feat/v1-distribution--t5:docs/release-notes.md` ("Known limitations") |
| Supported versions: SemVer; `latest 1.x` (when `1.0.0` ships) and `latest 0.x` (until `1.0.0` ships); no backports to older minor or patch lines (fix-forward on `main`); engine packs (`docling`, `pack-<name>-v<version>`) versioned separately and fixed via new signed pack releases | `docs/releasing.md` ("Release procedure", "Assets owned by other workflows", "Failure responses"), `docs/git-workflow.md` ("Model", "Promotion and hotfixes"), `docs/packs.md` ("Manifest (`ariad-pack/1`)", "Signing and publishing") |
| Surfaces and trust boundaries: out-of-process engines (`Pandoc`, `PDF`, `Typst`, `LibreOffice`, `Docling`) over `ariad-engine/1`, Linux Landlock/seccomp (`best-effort` locally, `required` on Linux workers) and macOS Seatbelt (`sandbox-exec`), memory watchdogs, finite limits, pure-Rust bounded/fuzzed `ariad-core` readers | `ARCHITECTURE.md` §1, §6.2, §7, §11.1–§11.3, §14; `docs/cli.md` |
| Signed engine packs (`pack.toml.minisig` prehashed Minisign with `pack:<name>` and `version:<version>`, compiled-in keys, size/SHA-256/extraction bounds) and desktop release verification | `ARCHITECTURE.md` §7.1, §16; `docs/packs.md`; `docs/docling.md`; `docs/install.md`; `docs/desktop.md` |
| Local MCP path confinement (`--allow-dir`, client roots, capability-based output promotion, symlink and hidden-component refusal) and browser Web Worker + OPFS + `pandoc.wasm` SHA-256 verification | `ARCHITECTURE.md` §4, §8.1; `docs/mcp.md` ("Security and Path Confinement"); `docs/web.md`; `origin/feat/v1-distribution--t5:docs/release-notes.md` |
| Cloud `/v1` authentication (Better Auth EdDSA JWT/JWKS, read-only `x-api-key`, rate-limited anonymous quotas), SHA-256-bound S3 presigned `PUT`, HMAC-SHA256 local `/v1/blobs/{token}` URLs, remote `/mcp` Streamable HTTP with OAuth 2.1 (dynamic client registration, S256 PKCE, `/mcp` audience) or API keys, self-hosting isolation | `ARCHITECTURE.md` §4, §8.3, §9.1–§9.5, §10, §11.3; `docs/api.md`; `docs/server.md` |
| Internal v1 security review summary: no external certification claimed; internal review on `fix/v1-security-audit` found 0 critical, 1 high + 2 medium in `security-audit-261025-v1.md` and 2 high + 1 medium in `code-review-261025-phase7-final.md`, all 6 fixed in code, 0 open critical/high code findings, documented medium/low/info follow-ups | `origin/fix/v1-security-audit:plans/reports/security-audit-261025-v1.md` and `origin/fix/v1-security-audit:plans/reports/code-review-261025-phase7-final.md` |
| Coordinated disclosure: private handling in GitHub security advisories; fixes ship in a release or pack release with an advisory when appropriate | Brief §4; GitHub Security Advisories workflow |

## Deliberate omissions and reasons

- **Contact email addresses, PGP keys, response-time SLAs, bug bounties, safe-harbor legal language, and hard-coded support dates:** None exist in the repository; omitted per brief instructions.
- **External audits or compliance certifications (SOC 2, ISO 27001, etc.):** None exist; explicitly stated that no external certification is claimed.
- **Clickable Markdown links to `docs/release-notes.md`, `docs/self-hosting.md`, and `docs/security/threat-model.md`:** Those files live on sibling unmerged branches (`origin/feat/v1-distribution--t5` and `origin/fix/v1-security-audit`) and are not yet present in `git ls-files` on `docs/v1-security-policy` (`88c52eb`). Referenced as inline code paths so every Markdown link in `docs/SECURITY.md` resolves cleanly on this branch today (`git ls-files --error-unmatch`) as well as after integration.

## Links verified on branch (`git ls-files` and headings)

All 12 relative file targets in `docs/SECURITY.md` were checked against `git ls-files --error-unmatch` and heading anchors were checked with `rg`:

- `../ARCHITECTURE.md` (`#1-architectural-principles`, `#4-product-surfaces`, `#7-engine-protocol`, `#11-security-and-sandboxing`, `#113-isolation-layers`, `#14-quality-testing-and-benchmarking`)
- `api.md` -> `docs/api.md`
- `cli.md` -> `docs/cli.md`
- `desktop.md` -> `docs/desktop.md`
- `docling.md` -> `docs/docling.md`
- `git-workflow.md` -> `docs/git-workflow.md`
- `install.md` -> `docs/install.md` (`#2-checksum-and-minisign-verification`)
- `mcp.md` -> `docs/mcp.md` (`#security-and-path-confinement`)
- `packs.md` -> `docs/packs.md`
- `releasing.md` -> `docs/releasing.md` (`#failure-responses`, `#assets-owned-by-other-workflows`)
- `server.md` -> `docs/server.md` (`#object-storage-api-and-worker`, `#container-runtime-support`, `#data-lifecycle`)
- `web.md` -> `docs/web.md`

## Commits

- `e954a9b` `docs: describe supported versions and reporting`
- plus the commit adding this report (`plans/reports/a-security-md-argon.md`)

## Commands run and outcomes

- `git status -sb && git rev-parse HEAD && git ls-files docs/`: verified checkout on `docs/v1-security-policy` at `88c52eb7aa4d38a1fd8776200793a9997f6f41ad`.
- `git show origin/feat/v1-distribution--t5:docs/release-notes.md`: read v1.0.0 and v0.2.0 release notes and known limitations.
- `git show origin/fix/v1-security-audit:plans/reports/security-audit-261025-v1.md`, `code-review-261025-phase7-final.md`, `worker-7-r4-opus55.md`: verified audit findings and status.
- `rg '^#{1,3} ' ARCHITECTURE.md docs/*.md`: verified heading anchors.
- Link verification loop (`rg` + `git ls-files --error-unmatch`): all 12 relative targets passed (`OK file`).
- `typos docs/SECURITY.md plans/reports/a-security-md-argon.md`: passed (`exit 0`).
- `sh scripts/install-lint-tools.sh && sh scripts/check-commits.sh origin/feat/v1-distribution..HEAD`: passed (`exit 0`).
- Build/test gate: not run (task brief specifies build-free).

## Open concerns

- Once `feat/v1-distribution--t5`, `feat/cloud-product`, and `fix/v1-security-audit` are merged into the integration branch alongside this branch, the inline code references `` `docs/release-notes.md` `` and `` `docs/self-hosting.md` `` in `docs/SECURITY.md` can optionally be turned into Markdown links if desired.

Status: DONE
