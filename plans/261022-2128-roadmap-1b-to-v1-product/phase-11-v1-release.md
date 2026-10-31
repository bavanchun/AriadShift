---
phase: 11
title: "v1.0: remote MCP, OS integration, audit, release"
status: pending
priority: P1
effort: "30h"
dependencies: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
runs_on: ASUS, Mac
branch: feat/v1-hardening
---

# Phase 11: v1.0 — remote MCP, OS integration, audit, release

## Goal

Close roadmap 5: a remote MCP endpoint, operating-system integration, a security audit with no open high-severity finding, a frozen `/v1` API, and the `v1.0.0` release of every surface.

## Requirements

- **Remote MCP:** a Streamable HTTP MCP endpoint in `ariad-server` (rmcp server transport behind axum), authenticated by API key or JWT, exposing `list_engines`, `plan`, `inspect` and `convert` over uploads (inline content up to a stated size or an upload id) plus job status and output URLs. `docs/mcp.md` gains a remote section with client configuration.
- **OS integration** through the desktop app:
  - macOS: a Finder Quick Action or Services entry "Convert with AriadShift" that opens the app with the selected files;
  - Windows: Explorer context-menu verbs for supported extensions, registered by the installer per user;
  - Linux: a `.desktop` entry with MIME types and actions, plus a Nautilus script and a Dolphin service menu shipped in the `.deb` and `.rpm`.
- **macOS engine sandbox:** Seatbelt profiles applied to engine processes where `sandbox-exec` is available, best effort, reported by `doctor`.
- **Security audit:** refresh the threat model (`docs/security/threat-model.md`), review authentication, uploads and presigned URLs, MCP path confinement, pack verification and extraction, the sandbox, every parser, and the web app's headers and CSP; run cargo-deny, `pnpm audit`, `pip-audit`; add fuzz targets for archive preflight, `docx_meta` and any parser that lacks one. Fix every high finding; record the report under this plan.
- **API freeze:** `/v1` marked stable in the OpenAPI document; a snapshot test fails on breaking changes; `docs/api.md` states the versioning and deprecation policy.
- **Docs:** README describes every surface with install and quick start; `docs/` covers CLI, MCP (local and remote), desktop, web, self-hosting, API and SDK, packs, security; ARCHITECTURE.md status becomes "v1".
- **Release v1.0.0:** version bump, promotion of `dev` to `main`, tag `v1.0.0`, the CLI release, desktop bundles, GHCR images; smoke tests for every channel that publishes; evidence recorded.

## Acceptance criteria

- Roadmap 5 criteria in ARCHITECTURE §17: zero open high-severity issues; `/v1` API frozen.
- Every product acceptance criterion in [plan.md](./plan.md) is ticked with evidence or listed under the user's blocked items.
