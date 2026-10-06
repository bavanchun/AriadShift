---
phase: 13
title: "Release gate and acceptance"
status: pending
priority: P1
effort: "7h"
dependencies: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]
---

# Phase 13: Release gate and acceptance

## Goal

Stop for the user's go-ahead, then publish v0.1.0 through every channel, verify the installs on clean runners, and close roadmap 1a against its acceptance criteria.

## Stop for the user

This phase starts with a stop. The coordinator presents the following and waits. `--auto` does not answer it: it is publication, outside any in-plan decision.

1. The final `dev` SHA and its three-OS CI run, the bench diff, and the dry-run evidence from phase 12.
2. The exact outward actions, each confirmed separately:
   - create the public repo `bavanchun/homebrew-tap` (the coordinator runs `gh repo create`);
   - create the `release` environment with a `v*` tag deployment rule, then the tokens and secrets named in phase 12. `HOMEBREW_TAP_TOKEN` is a fine-grained token with `contents: write` on `bavanchun/homebrew-tap` only. **The user creates the tokens and sets the secrets themselves** (for example `! gh secret set HOMEBREW_TAP_TOKEN --env release`); the coordinator never sees or stores a token value;
   - bump the version to 0.1.0 on `dev`, promote `dev` to `main`, push the tag `v0.1.0` on `main`, and let `release.yml` publish the GitHub Release, the installers and the formula;
   - the first winget submission (a PR to `microsoft/winget-pkgs` from the user's fork), as the binstall/winget report and phase 12 describe;
   - the crates.io publication of `ariad-core`, `ariad-host` and `ariad-cli` 0.1.0 by `publish-crates.yml` (validation decision). The user creates the scoped crates.io token and sets `CARGO_REGISTRY_TOKEN` in the `release` environment. Crate names cannot be reclaimed once published.
3. Nothing outward happens without an explicit "yes" for that item. A declined item is recorded as not done, with the acceptance criterion left open.

## Requirements after the go-ahead

- Version 0.1.0 in `[workspace.package]` and in `Cargo.lock`, committed as `chore(release): prepare 0.1.0` on a branch from `dev` and merged into `dev` through a pull request. The internal dependencies are workspace-inherited since phase 2, and phase 12 rehearsed the bump, so this is a one-line manifest change plus the lockfile. The release notes come from dist's generated body plus a short summary of 1a. Root markdown stays limited (AGENTS.md), so there is no `CHANGELOG.md` at the root; the notes live in the GitHub Release.
- Promote `dev` to `main` with a pull request merged as a merge commit ([docs/git-workflow.md](../../docs/git-workflow.md) "Promotion and hotfixes"). Tag `v0.1.0` on that merge commit. Watch `release.yml` to completion.
- Run `release-smoke.yml` (phase 12) against v0.1.0 and confirm on clean runners:
  - `macos-26`: `brew install bavanchun/tap/ashift`, then `ashift doctor` and `ashift convert` on a Markdown file to DOCX and EPUB;
  - `ubuntu-26.04`: the shell installer, plus Homebrew on Linux if the runner image has it;
  - `windows-2025`: the PowerShell installer, then `ashift doctor` and a conversion, with Pandoc installed from its official release;
  - `cargo binstall ariad-cli` (crates.io).
- Winget: generate the manifests with `scripts/winget-manifest.sh 0.1.0` and submit `VChun.AriadShift` with `wingetcreate submit` (MIT) from the user's fork, after the user signs the Microsoft CLA on the PR. Record the PR URL. Its approval is outside our control; acceptance needs an open, validated submission, not a merge.
- Close-out:
  - Tick the plan's acceptance criteria with evidence (run URLs, the release URL, the winget PR URL).
  - Fill a "Completion record" in `plan.md`.
  - Update AGENTS.md "Current state" (commands and routes now available) and README (install section with every channel and the Intel-Mac Pandoc note).
  - Write a short `docs/releasing.md` describing the release procedure for the next version (bump on `dev`, promote, tag on `main`).
  - The close-out commits land on `dev` through the usual pull request. They reach `main` at the next promotion, or at once if the user approves a second promotion.

## Files

- Modify: `Cargo.toml`, `Cargo.lock`, `README.md`, `AGENTS.md`, `plans/261006-0544-docling-spike-and-roadmap-1a/plan.md`
- Create: `docs/releasing.md`
- External, after explicit approval only: the `bavanchun/homebrew-tap` repo, the `release` environment and its secrets, the GitHub Release `v0.1.0`, the three crates on crates.io, the winget PR for `VChun.AriadShift`

## Success criteria

- Every roadmap 1a acceptance criterion in `plan.md` is ticked with evidence, or left open with the user's recorded reason.
- `release-smoke.yml` is green for every channel that was published.

## Risk assessment

| Risk | Signal | Response |
|---|---|---|
| `release.yml` fails mid-publish | Some artifacts uploaded, the formula not pushed | Do not delete the tag silently. Report it, fix it forward with v0.1.1 if anything was published, and ask the user before deleting a release |
| Homebrew formula audit fails (`brew style`, `brew audit`) | Publish job red | Fix the dist config and re-run the publish job only; the release itself stays |
| winget validation flags the binary | Bot comments on the PR | Address the manifest or explain; unsigned binaries are allowed but must pass the Defender/SmartScreen scans |
| Token leak | A token printed in logs | Tokens live only in GitHub secrets set by the user; workflows never echo them; rotate immediately if exposed |
