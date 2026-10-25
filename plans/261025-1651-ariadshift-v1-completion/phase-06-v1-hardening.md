---
phase: 6
title: "v1 hardening: remote MCP, OS integration, Seatbelt, API freeze, fuzz"
status: pending
depends_on: [4]
branch: feat/v1-hardening
---

# Phase 6: v1 hardening

The build half of roadmap phase 11 ([spec](../261022-2128-roadmap-1b-to-v1-product/phase-11-v1-release.md)).

## Tasks

| Task | Owner (model) | Files owned | Work |
|---|---|---|---|
| T1 | GPT-6.1-Sol High | `crates/ariad-server/src/mcp/**` (new), `crates/ariad-server/tests/mcp_remote.rs` (new), the remote section of `docs/mcp.md` | Remote MCP on spec 2026-07-28 Streamable HTTP with OAuth (protected-resource metadata pointing at Better Auth as authorization server), API keys still accepted. Follow `ak-mcp-builder`. Tools: `list_engines`, `plan`, `inspect`, `convert` (inline up to a stated cap, or an upload id), job status and output URLs. The coordinator mounts the route in `app.rs`. |
| T2 | Opus 5.5 High | Bundle config in `apps/desktop/src-tauri/`, `apps/desktop/os-integration/**` (new) | macOS Finder Quick Action / Services entry, Windows per-user Explorer verbs, and a Linux `.desktop` file with MIME types plus Nautilus and Dolphin entries in `.deb`/`.rpm`. An optional REA study of Raycast/Arc polish feeds UI touches, written to `plans/reports/rea-261025-desktop-polish.md`. |
| T3 | GPT-6.1-Sol High | `crates/ariad-host/src/sandbox/macos*.rs` (new), the `doctor` sandbox lines in `crates/ariad-cli` | Best-effort Seatbelt profiles for engine processes where `sandbox-exec` exists, reported by `doctor`; no `unsafe`. |
| T4 | Opus 5.5 High | `crates/ariad-server/tests/openapi_v1_snapshot.rs` (new), the stability marker in `schemas/openapi.v1.json`, the versioning section of `docs/api.md` | Freeze `/v1`: a snapshot test fails on breaking changes; the deprecation policy is documented. |
| T5 | Opus 5.5 High | `fuzz/fuzz_targets/{archive_preflight,docx_meta}.rs` (new), `fuzz/Cargo.toml`, the `fuzz-nightly.yml` target list | Fuzz targets for archive preflight, `docx_meta` and any parser that lacks one (inventory in the report). |

## Build and test pass (coordinator, sequential)

```sh
source /Volumes/AriadWork/02-Ariadnev-Eco/.cache/env.sh
just ci && just test-server && just desktop-ci
just fuzz archive_preflight 60 && just fuzz docx_meta 60
cargo run -p ariad-cli -- doctor
```

## Acceptance

- [ ] MCP Inspector connects over HTTP through OAuth (and with an API key), lists the tools and converts an uploaded file.
- [ ] The macOS Quick Action opens the app with the selection; the Windows and Linux registrations are present in the bundle outputs.
- [ ] `doctor` reports Seatbelt status; an engine write outside its job dir is denied cleanly.
- [ ] The snapshot test passes and fails on a deliberate field removal. The new fuzz targets run 60 s clean.
