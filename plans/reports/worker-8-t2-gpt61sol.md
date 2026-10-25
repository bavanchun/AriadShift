# Release workflow delivery

The release workflow retains the cargo-dist Homebrew generator and verifies the
formula against built archives before publishing, including in PR rehearsals.
Missing crates.io or tap credentials now produce notices and skip the
corresponding publishing operations successfully.

## Files changed

- `.github/workflows/release.yml` runs verifier regression tests, checks all
  archive hashes and formula platform branches, stages only the new formula,
  and skips tap checkout, download, commit and push without its token.
- `.github/workflows/publish-crates.yml` skips checkout, toolchain setup and
  publication without its token. Configured registry errors still fail.
- `scripts/release/verify-homebrew.py` checks workspace version, release URLs,
  SHA-256, macOS/Linux CPU branches, Pandoc dependency and installed binary.
  All five archives, including Windows, must match their checksum sidecars.
- `scripts/release/test-verify-homebrew.py` covers valid targets, corrupt
  archives, wrong hashes, URLs, versions and platforms, missing files, extra
  URLs, unsupported Unix targets and required installation metadata.
- `scripts/check-release-workflow.sh` enforces formula verification and token
  gates while retaining pinned actions, minimal permissions, protected
  publishing jobs and isolation of actual credential values.
- `docs/releasing.md` documents optional publishing, formula verification,
  channel smoke prerequisites and ownership of desktop and pack assets.

## Commits and publication

- `d8f4c12`: `ci(release): verify formula archives and skip optional publishing`.
- This report is committed separately on `feat/v1-distribution--release`.
- Push target: `origin/feat/v1-distribution--release`, with upstream tracking.

## Evidence and review

No builds, tests, workflow dispatches or registry publications were run on
ASUS, as required by the brief. `git diff --cached --check` passed before the
implementation commit. The complete diff received an inline review for token
exposure, absent and present token behavior, target coverage and existing
guard mutation anchors. Automated lint and test results remain unverified.

Current sources checked: the pinned
[cargo-dist 0.33.0 formula template](https://github.com/axodotdev/cargo-dist/blob/v0.33.0/cargo-dist/templates/installer/homebrew.rb.j2),
the [Homebrew installer documentation](https://axodotdev.github.io/cargo-dist/book/installers/homebrew.html),
GitHub guidance on [missing secrets and conditional use](https://docs.github.com/en/actions/how-tos/write-workflows/choose-what-workflows-do/use-secrets)
and [reusable workflow secret forwarding](https://docs.github.com/en/actions/how-tos/reuse-automations/reuse-workflows),
and Python documentation for hashlib and tomllib. No dependency or tool
versions were added or changed.

## Coordinator verification commands

Run these sequentially in the shared build queue:

```sh
cd /home/vchun/Codes/01-AriadShift-wt-p8-release
python3 scripts/release/test-verify-homebrew.py
just check-release-workflow
just lint-workflows
just package-check
just ci
just lint-commits origin/dev..HEAD
```

Then run a release PR rehearsal and follow
[the formula download and verification procedure](../../docs/releasing.md#verify-the-formula-without-publishing).
Confirm that all five target archives and `ashift.rb` are present. The
rehearsal must pass without entering the protected publishing environment.

## Open concerns

- Coordinator verification is pending. Actionlint, zizmor, guard regression
  tests and actual cargo-dist artifact verification were not executed locally.
- The verifier intentionally follows the cargo-dist 0.33.0 Ruby structure.
  Recheck it when upgrading dist or changing release targets.
- The existing four-channel smoke workflow expects published crates.io and
  tap versions. It is unsuitable as a GitHub-only release gate when either
  token was absent; this limitation is documented.
- Desktop attachments and independently versioned signed pack releases retain
  their existing workflows. GHCR implementation belongs to the cloud worker.

Status: DONE_WITH_CONCERNS
