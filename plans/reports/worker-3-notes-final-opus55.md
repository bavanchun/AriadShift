# Worker p3-notes: final v0.2.0 version bump and release notes

Branch `release/v0.2.0--notes`, which now contains `origin/dev` at `68ac304` (#48, #49). Nothing was built or tested, as the brief requires.

## Commits

- `a5ba6da` Merge `origin/dev` into `release/v0.2.0--notes`. This resolves the `Cargo.toml` conflict with #48's new `ariad-typst` workspace dependency, and sets the workspace crates in `Cargo.lock` to `0.2.0` by hand.
- `be19273` docs: finalize the 0.2.0 release notes
- (this report) docs(plans): add the final release notes worker report

## Files changed

| File | Change |
|---|---|
| `Cargo.toml` | Conflict resolved: `ariad-core`, `ariad-typst` and `ariad-host` workspace dependencies are at `0.2.0`, and `[workspace.package] version` is `0.2.0`. |
| `Cargo.lock` | `version` lines for `ariad-cli`, `ariad-core`, `ariad-desktop`, `ariad-host`, `ariad-server`, `ariad-typst` and `ariad-wasm` changed from `0.1.0` to `0.2.0`. No other lock lines changed (`git diff origin/dev -- Cargo.lock` shows only these 7 lines). |
| `docs/release-notes.md` | Rewritten as the final 0.2.0 draft. |

## Version check

- `0.2.0` everywhere: the `Cargo.toml` workspace and all three internal workspace deps; every member (`crates/*`, `apps/desktop/src-tauri`) through `version.workspace = true`; `apps/desktop/src-tauri/tauri.conf.json`; the `schemas/openapi.v1.json` `info.version`; `engines/docling/pyproject.toml`; and the pack `REQUIRES_ASHIFT = ">=0.2.0, <0.3.0"`.
- No `version` field exists in `package.json`, `apps/web/package.json`, `apps/desktop/package.json` or `brand/package.json`, so none was added.
- `crates/ariad-core/data/capabilities.json` keeps `"ashift_version": "0.1.0"`. It records which build produced the measured benchmark (bench provenance), not the release version, so I left it unchanged. If the coordinator wants it to say 0.2.0, regenerate it through the bench workflow; do not edit it by hand.

## Release notes

- `docs/releasing.md` names no changelog file. Step 1.5 says no root `CHANGELOG.md` exists and that `dist` writes the GitHub Release body. `docs/release-notes.md` therefore stays the only notes file, and I made no changelog edit.
- The new sections are Upgrade notes, Added, Changed, Fixed, Security, a marked #51 section, and Known limitations. Every entry cites the PR that merged it into `dev`. I matched each of the 242 non-plan commits in `v0.1.0..origin/dev` to its PR by comparing subjects with `gh pr view <n> --json commits`. Those PRs are #31, #33–#50, plus the squash commits of #48 and #49. Facts were checked against `docs/cli.md`, `docs/mcp.md`, `docs/desktop.md`, `docs/web.md`, `docs/bench-report.md`, the bodies of PR #49 and #51, `apps/web/engine-assets.mjs`, `apps/web/src/converter/wasm-cache.ts` and `apps/desktop/src-tauri/src/watch.rs` (the 16-watch limit).
- The upgrade notes cover `ariad-ir/1`, the EPUB identifier change, Typst as the default PDF route, the Linux sandbox default, LibreOffice namespaces, exit codes, host-versioned and signed engine packs, and the pinned `pandoc.wasm` SHA-256 (`engine_integrity`).
- **Removed as v1.0.0 scope:** the whole "Cloud server (new, self-hosted)" section, which covered the `/v1` API, workers, queues, container images, PostgreSQL/S3 and isolation. The server crate is in `dev`, but it is neither packaged nor supported in 0.2.0. One Known limitations line now says so and points to 1.0.0. Server-only hardening (#44, #45, #47) and the JWT advisory build changes are left out. The notes do not mention compose, the SDK, auth, GHCR, remote MCP, Seatbelt, a first-run guide or Minisign-signed installers.
- **#51:** I put it in its own section, "Docling pack without Intel components", with a blockquote telling the coordinator to verify it at merge time. On 2026-10-25, #51 is OPEN as a draft. Its body says the toolchain wheels are not yet published or pinned. Edit the entry to match the merged change, or delete the section if #51 misses the tag.

## Commands for the coordinator

```bash
source /Volumes/AriadWork/02-Ariadnev-Eco/.cache/env.sh
cd /Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift-wt-p3-notes
git diff origin/dev -- Cargo.toml Cargo.lock          # expect only 0.1.0 -> 0.2.0 lines
cargo check --workspace --locked                      # confirms the hand-edited lock needs no other change
just licenses                                         # THIRD_PARTY_LICENSES still lists ariad-* 0.1.0 (lines ~14920-14926)
cargo test -p ariad-server --test openapi_drift
just ci
just check-release-workflow
just package-check
just lint-commits                                     # the merge commit is skipped by scripts/check-commits.sh
typos docs/release-notes.md                           # passed locally with typos-cli 1.50.3
```

## Open concerns

1. **`THIRD_PARTY_LICENSES`** still says `ariad-* 0.1.0`, so `just licenses-check` fails until `just licenses` runs. That file is outside my ownership. Check whether the desktop JavaScript licence output (`scripts/generate-desktop-licenses.mjs`) also embeds the version.
2. **#51 entry.** See above. The same PR edits `docs/releasing.md`, so I did not touch that file, to avoid a conflict.
3. **Stale text in files I do not own:** `docs/server.md:38` ("this workspace is still 0.1.0"), the comment at `crates/ariad-cli/tests/docling_pack.rs:30`, `README.md:40` and the AGENTS.md "Current state" all need updating at or after the tag.
4. **Release date.** The heading still reads `0.2.0 (unreleased)`. Replace it with the tag date when tagging. The notes are not wired into `dist`; pass the 0.2.0 section to `gh release edit v0.2.0 --notes-file` (or prepend it) once the release exists.

Status: DONE_WITH_CONCERNS
