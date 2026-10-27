# Owner decisions OD-1 to OD-21 for v1.0.0

Date: 2026-10-27 23:55 +07. The owner delegated these decisions to the Mac coordinator. They settle section 5 of `console-review-261027-2045-master-fix-plan-final.md`. Facts checked before deciding: the repository `bavanchun/AriadShift` is public; the only release and tag is `v0.1.0`; the `release` environment holds `PACK_MINISIGN_KEY`, the Tauri signing keys, `CARGO_REGISTRY_TOKEN` and `HOMEBREW_TAP_TOKEN`.

| # | Decision |
|---|---|
| OD-1 | (b) No separate app v0.2.0. The next app release is v1.0.0. The Docling pack is released on its own tag (OD-2). Close `release/v0.2.0--notes-fix` after salvaging any notes fix into the v1.0.0 notes. |
| OD-2 | Publish `pack-docling-v0.2.0` from dev's builder after I3 and I4 land, with `requires_ashift` covering 1.x. No such tag exists today, so there is nothing to re-publish. Never move or overwrite a published pack tag; a bad pack gets a new patch version. |
| OD-3 | Treat the existing `PACK_MINISIGN_KEY` in the `release` environment as the production pack key. No rotation for v1. |
| OD-4 | Draft-then-publish: the workflow creates a draft release with all signed assets, the Mac coordinator checks it, then it is published. |
| OD-5 | (b) Keep the recorded single-key decision, plus required reviewers on the `release` environment and a `v*` / `pack-*` tag ruleset. |
| OD-6 | Registration cap `e7101d8` only for v1. C4 goes to the post-v1 backlog. |
| OD-7 | Accept the global OAuth token lock as documented. D18 is post-v1. |
| OD-8 | Grant LibreOffice a dedicated IPC directory rather than exempting it. When the sandbox is refused, fail closed with a clear error. `/opt` rules are Linux-only. No desktop setting for v1. |
| OD-9 | Self-service sign-up is off by default at v1 (enabled by an explicit operator setting, documented as lacking email verification). Plain-HTTP LAN only behind an explicit opt-in flag, documented. Path-prefixed `ARIAD_PUBLIC_URL` is unsupported in v1 and documented so. `file://` storage is documented as single-tenant only, which settles E5 without code. |
| OD-10 | Classic shell verbs instead of `IExplorerCommand`. MSI installs per-user (HKCU). |
| OD-11 | Keep retention-bound deletion. Require the password server-side when the account has one; passkey-only and social accounts must have a recent sign-in instead. |
| OD-12 | Accept the listed residual risks and record them in the security docs. |
| OD-13 | Windows Docling pack deferred to #83. Social sign-in on web is out of v1. `/v1/blobs/{token}` stays outside the frozen contract. |
| OD-14 | Keep anonymous job ids in `localStorage`, documented in the privacy notes. Signed-in users fail closed on auth errors. |
| OD-15 | "Compose stack" becomes a required check once B2 is green. Fuzz seeds are committed as fixtures with `fixtures/manifest.toml` entries. Keep PR #85's explicit boot budget. |
| OD-16 | (a) Allow `libgomp` (GCC runtime exception) and LGPL `libquadmath`/`libgfortran` with notices, matching AGENTS.md; update the gate, ARCHITECTURE.md and its Decision Log in the same change. (b) Keep `CC0-1.0` only if the component it covers is data or metadata; if it covers code, remove the exception and the dependency. |
| OD-17 | (a) Writer-internal output ceiling; do not change the frozen `schemas/engine-protocol.v1.json`. (b) Enforce the `ariad-ir/1` invariants in IR validation. (c) Yes, `max_nesting_depth` counts Markdown inline nesting. |
| OD-18 | (a) Cancel terminates the worker and keeps the input metadata. (b) Add a finite `Limits::browser()` profile. (c) Keep the 64 MiB per-entry stamping cap for the web only; the local host follows the HOST-2 fix. |
| OD-19 | (a) Take the conservative route: list the pandoc.wasm build recipe and link every dependency's source on `/licenses`. (b) The repository is public, so the current links work. |
| OD-20 | Honour `ASHIFT_PACK_<NAME>` only in dev builds or when an explicit opt-in variable is also set, and print a warning. CI E2E sets the opt-in. |
| OD-21 | Fail the export with a clear error. |

## Release authority

The coordinator approves each of these individually, on the exact commit, before ASUS runs it: the `pack-docling-v0.2.0` tag and draft release, the `dev` to `main` promotion, and the `v1.0.0` tag and draft release. Nothing is published without that approval.

## Housekeeping (H1)

Delete `feat/v1-hardening--review` now and `--oauth-rev` after step 3a. Close `docs/v1-security-policy`. Close `fix/v1-security-audit` and its sub-branches after step 4. Close Dependabot #60 after step 5.
