# Worker p3-t1: v0.2.0 version bump and release notes

Branch `release/v0.2.0--notes`, based on `origin/dev` at `3b85281`. Nothing was built or tested, as the brief requires.

## Commits

- `1c3d0c3` chore(release): prepare 0.2.0
- `f32384a` docs: add the 0.2.0 release notes
- (this report) docs(plans): add the release notes worker report

## Files changed

| File | Change |
|---|---|
| `Cargo.toml` | `[workspace.package] version`, `ariad-core` and `ariad-host` workspace dependency versions set to `0.2.0` (the three entries in `docs/releasing.md` step 1.2). |
| `apps/desktop/src-tauri/tauri.conf.json` | `version` set to `0.2.0`. It was an explicit `0.1.0` that duplicated the crate version, so the bundle and updater metadata would otherwise stay at 0.1.0. |
| `schemas/openapi.v1.json` | `info.version` set to `0.2.0`. The server generates it from `CARGO_PKG_VERSION`, and `crates/ariad-server/tests/openapi_drift.rs` compares the whole file, so the drift test would fail without this change. This was the only line that changed. |
| `docs/release-notes.md` | New file with the release notes. |

`apps/web/package.json` and `apps/desktop/package.json` have no `version` field, so they were left unchanged. Member crates inherit `version.workspace = true`. `docs/releasing.md` does not say that any capabilities or protocol version strings follow the release, so none were changed.

## Release notes location

`docs/releasing.md` names no notes file. It says that root markdown is restricted and that `dist` generates the release notes. `dist` only auto-detects `CHANGELOG*` or `RELEASES*` in a package or workspace root, so the notes go in `docs/release-notes.md`. That location is allowed by AGENTS.md, and `dist` ignores it, so release artifacts do not change. The file uses `## 0.2.0 (unreleased)` headings, which parse-changelog can read in case it is wired up later.

The notes cover `git log v0.1.0..origin/dev` (excluding plan-only commits) plus PRs #48, #49 and #51, read with `gh pr view`. They have sections for breaking changes and upgrade notes, the surfaces (CLI, MCP, desktop, web, server, engines and packs, libraries), and known limitations: unsigned desktop builds, no Intel macOS desktop build, Docling pack platforms, Linux-only sandbox, external Pandoc, browser limits, self-hosted-only cloud, and unconfirmed package channels.

## Commands for the coordinator

```bash
# After #48, #49 and #51 merge into dev, rebase this branch and bump ariad-typst (see concern 1)
git fetch origin && git rebase origin/dev
cargo check --workspace           # refreshes Cargo.lock
just licenses                     # regenerates THIRD_PARTY_LICENSES with 0.2.0 crate versions
cargo test -p ariad-server --test openapi_drift
just ci
just check-release-workflow
just package-check
grep -rn '0\.1\.0' Cargo.toml apps/desktop/src-tauri/tauri.conf.json schemas/openapi.v1.json   # expect no output
```

## Open concerns

1. **#48 adds `ariad-typst`.** It adds `ariad-typst = { path = "crates/ariad-typst", version = "0.1.0" }` to `[workspace.dependencies]` next to the lines this branch edits, and its `docs/releasing.md` change adds that entry to the bump list. Expect a rebase conflict in `Cargo.toml`. Set `ariad-typst` to `0.2.0` while resolving it, or `cargo publish` order and `just package-check` will see mismatched siblings.
2. **Notes that depend on the open PRs.** Bullets tagged `(#48)`, `(#49)` and `(#51)` describe those PRs. Remove or edit them if a PR does not merge before the tag. The links to `bench-report.md` and `schemas/ir.v1.json` resolve only after #49 merges. #51 was still a draft when these notes were written: its toolchain wheels were not yet published or pinned.
3. **`THIRD_PARTY_LICENSES` and `Cargo.lock`.** Both embed the workspace version. They are not in this worker's ownership, so the coordinator must regenerate them (`just licenses-check` fails until it does).
4. **Stale text in files this worker does not own:**
   - `docs/server.md:38` says "while this workspace is still 0.1.0"; that clause is now stale.
   - The comment at `crates/ariad-cli/tests/docling_pack.rs:30` says the tree "is still 0.1.0". The test logic still works with 0.2.0.
   - `AGENTS.md` "Current state" and `README.md:40` should be updated after the tag.
   - `README.md` "Project documents" and `docs/releasing.md` step 1.5 could link `docs/release-notes.md` and say to pass its 0.2.0 section to `gh release edit v0.2.0 --notes-file` (or prepend it to the dist-generated body).
5. **Release date.** The heading reads `0.2.0 (unreleased)`. Replace it with the tag date when tagging.

Status: DONE_WITH_CONCERNS
