---
phase: 12
title: "Release pipeline"
status: pending
priority: P1
effort: "10h"
dependencies: [9, 10]
---

# Phase 12: Release pipeline

## Goal

Prepare everything needed to publish `ashift` v0.1.0 through GitHub Releases (archives plus shell and PowerShell installers), the Homebrew tap, crates.io (for `cargo binstall` and `cargo install`) and winget, and prove it with dry runs. Nothing is published in this phase; publication waits for the stop in phase 13.

## Context links

- [1a tooling facts](../reports/researcher-261006-1209-phase-1a-tooling-facts.md) §3 (dist 0.33.0, SHA pins via `[dist.github-action-commits]`, retired default runners, Homebrew `pandoc` dependency)
- [binstall and winget facts](../reports/researcher-261006-1209-binstall-winget-facts.md):
  - dist archives are named `ariad-cli-<target>.<ext>`;
  - binstall works with `--git` without crates.io;
  - the first winget submission needs `wingetcreate new` (MIT); winget-releaser (AGPL-3.0) only updates existing packages;
  - the portable-zip manifest shape.
- ARCHITECTURE §15 (`THIRD_PARTY_LICENSES` and an SBOM per release), §16 (as updated in phase 2)

## Scout first

Re-check dist's latest stable release and its changelog. Run `dist init --yes` in a scratch worktree to see exactly what it generates for this workspace before writing the real config.

## Requirements

dist config (`dist-workspace.toml`):
- `cargo-dist-version = "0.33.0"` (or newer stable if re-verified); `ci = "github"`; `allow-dirty = ["ci"]`.
- Only `ariad-cli` is distributed; the binary is `ashift`. The library crates and `engine-probe` are excluded. The workspace sets `publish = false` (`Cargo.toml:11`), which dist reads as "do not distribute", so `crates/ariad-cli/Cargo.toml` must set `[package.metadata.dist] dist = true`. Step 1 asserts that `dist plan` lists `ariad-cli`.
- Targets: `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`, `x86_64-pc-windows-msvc`.
- `[dist.github-custom-runners]`:
  - macOS targets on `macos-26`;
  - Linux on `ubuntu-26.04` (musl);
  - Windows on `windows-2025`;
  - `global` on `ubuntu-26.04`.

  If dist needs an ARM Linux runner for `aarch64-unknown-linux-musl`, use `ubuntu-26.04-arm` if it exists, otherwise cross-compile; record which.
- Installers `shell`, `powershell` and `homebrew`; `tap = "bavanchun/homebrew-tap"`; `formula = "ashift"`; `publish-jobs = ["homebrew"]`; `[dist.dependencies.homebrew] pandoc = { stage = ["run"] }`.
- **Secrets hygiene.**
  - The generated release workflow reads `HOMEBREW_TAP_TOKEN` only from a GitHub `release` environment. Its deployment rule allows `v*` tags only, so no branch workflow (for example `spike/docling`) can read it.
  - The token is a fine-grained PAT, or a GitHub App token, with `contents: write` on `bavanchun/homebrew-tap` only.
  - Within the publish job, the token is exposed only to the step that pushes the formula (step-level `env`), never to the `brew update` / `brew style` steps. This is part of the single documented `allow-dirty` edit if dist's template does not already do it.
- `[dist.github-action-commits]` pins every action dist uses to a full SHA. Verify each SHA against the action's release tag and record the tag in a comment.
- The generated `release.yml` is edited in one place only: dist's `curl | sh` self-install becomes a download of the exact dist release archive, followed by a SHA-256 check against a value stored in the workflow, and then the run. The edit carries a comment that names the dist version and how to refresh the checksum. `brew update` stays as is; it is recorded as an exception in ARCHITECTURE.
- `pr-run-mode = "upload"`, so a PR touching release config builds every artifact without publishing.
- SBOM and licenses:
  - enable dist's CycloneDX SBOM and `cargo-auditable` if dist 0.33 supports them, otherwise add equivalent steps;
  - generate `THIRD_PARTY_LICENSES` with `cargo-about` (MIT OR Apache-2.0), using a committed `about.toml` and template, and include it plus `LICENSE` and `NOTICE` in every archive;
  - `NOTICE` names Pandoc as an external GPL-2.0-or-later tool that is not bundled.

cargo-binstall:
- Add an explicit `[package.metadata.binstall]` block to `crates/ariad-cli/Cargo.toml`:
  - `pkg-url = "{ repo }/releases/download/v{ version }/{ name }-{ target }{ archive-suffix }"`, `bin-dir = "{ name }-{ target }/{ bin }{ binary-ext }"`, `pkg-fmt = "txz"`;
  - a Windows override to `zip` with `bin-dir = "{ bin }.exe"`.
- Documented install: `cargo binstall ariad-cli` (crates.io metadata, binaries from GitHub Releases); `cargo install ariad-cli` builds from source.

crates.io (validation decision: publish from CI):
- Each of `ariad-core`, `ariad-host` and `ariad-cli` sets `publish = true`, overriding the workspace's `publish = false`. Each also gets `description`, `readme`, `keywords` and `categories`. License and repository are inherited. `engine-probe` stays behind its test feature.
- Packaging must succeed from the package alone. `cargo package --workspace` (Cargo resolves unpublished workspace siblings during packaging; verify on 1.99, otherwise package in order with `--no-verify` for the dependents and record why) must pass in `just ci` through a new `package-check` recipe. Phase 7 already keeps `capabilities.json` inside `ariad-core`.
- `.github/workflows/publish-crates.yml` is a reusable workflow that dist calls through `publish-jobs = ["homebrew", "./publish-crates"]`. It:
  - runs in the `release` environment, with `CARGO_REGISTRY_TOKEN` as an environment secret, exposed only to the publish step;
  - publishes `ariad-core`, then `ariad-host`, then `ariad-cli`, waiting for each version to appear in the index before the next;
  - is idempotent: a version that already exists is skipped, so a re-run after a partial failure finishes the rest.
- The token is a crates.io scoped token (`publish-new` for the first release; `publish-update` restricted to these three crates afterwards). The user creates it in phase 13.

winget:
- `packaging/winget/` holds a manifest template for the portable zip. It uses `InstallerType: zip`, `NestedInstallerType: portable`, `ashift.exe` with the alias `ashift`, multi-file manifests, and schema 1.9.0 or newer. The identifier is `VChun.AriadShift` with moniker `ashift` (validation).
- `scripts/winget-manifest.sh <version>` fills the version, URL and SHA-256 from a published release into `target/winget/`, for `wingetcreate submit` in phase 13. The first submission is done once, by hand.
- `.github/workflows/winget.yml` is not added now. Later versions use `wingetcreate update`, which is MIT-licensed and needs a token; that is follow-up after the package exists. Do not use winget-releaser (AGPL-3.0).

Smoke workflow:
- `.github/workflows/release-smoke.yml` is triggered by `workflow_dispatch` with an input `version` (and optionally on `release: published`).
- It installs `ashift` per channel on clean runners and runs `ashift --version`, `ashift doctor` and one conversion:
  - Homebrew on `macos-26`;
  - the shell installer on `ubuntu-26.04`;
  - the PowerShell installer on `windows-2025`;
  - `cargo binstall ariad-cli` on `ubuntu-26.04`.
- Pandoc comes from the formula on Homebrew and from the official release elsewhere. The workflow has `permissions: contents: read` and SHA-pinned actions.

Docs:
- README "Install" section: every channel, Pandoc as a prerequisite outside Homebrew, and the Intel-Mac note (Homebrew builds Pandoc from source; the official Pandoc installer is faster).
- `docs/releasing.md` is written in phase 13; this phase leaves notes in its report.

## Files

- Create: `dist-workspace.toml`, `.github/workflows/release.yml`, `.github/workflows/release-smoke.yml`, `about.toml`, `about.hbs` (or the dist-native equivalent), `packaging/winget/*.yaml.tmpl`, `scripts/winget-manifest.sh`
- Modify: `crates/ariad-{core,host,cli}/Cargo.toml` (crates.io metadata, `publish = true`; binstall metadata and `[package.metadata.dist] dist = true` in `ariad-cli`), root `Cargo.toml` (the `[profile.dist]` that `dist init` writes), `justfile` (`package-check` in `ci`); create `.github/workflows/publish-crates.yml`; `NOTICE`, `README.md`, `ARCHITECTURE.md` §16, `deny.toml` (only if cargo-about needs nothing; no new allowances). The Decision Log row for the release supply chain already exists from phase 2.
- Must not touch: `[workspace.package] version` (phase 13 bumps it)

## Implementation steps

1. Scratch `dist init` and review; then write the real config. Commit `build(release): configure dist for ashift`.
2. Pin actions and patch the dist installer with its checksum; `dist plan` must pass with `allow-dirty`. Commit.
3. Licenses and SBOM; check that a local `dist build --artifacts=local --target x86_64-unknown-linux-musl` archive contains `ashift`, `LICENSE`, `NOTICE` and `THIRD_PARTY_LICENSES`, and that the binary runs (`ashift --version`, `ashift doctor`) on this host. Commit.
4. crates.io metadata, `package-check` and `publish-crates.yml` (a dry run with `cargo publish --dry-run` per crate where the registry allows). binstall metadata; prove it locally with `cargo binstall --manifest-path crates/ariad-cli/Cargo.toml --dry-run` against the PR-run artifacts if possible, otherwise record it as verified in phase 13. Commit each.
5. winget template and fill script, tested against the PR-run Windows zip's SHA-256. Commit.
6. `release-smoke.yml`. Commit.
7. **Version-bump rehearsal** on the dry-run branch: set `[workspace.package] version` to `0.1.0` there only, and confirm `cargo metadata`, `dist plan` and the PR build all pass. Phase 2's workspace-inherited internal dependencies make this a one-line change. Do not merge the bump.
8. Dry run in CI: the coordinator pushes a branch `release/dry-run` and opens a PR (owned repo, no publication). It confirms that every target builds and uploads artifacts, downloads them, and checks each archive's layout and the Windows zip name used by winget and binstall. It then closes the PR without merging the branch, or merges the config if the PR was the delivery vehicle; the coordinator decides per the git workflow.
9. README; `just ci`. Commit.

## Success criteria

- `dist plan` passes; the PR run builds all 5 targets on the pinned runners with pinned actions.
- Every archive contains the binary and the license files; the musl binary runs on this host; the Windows zip matches the winget and binstall naming.
- No step publishes anything: no release, no tap push, no crates.io upload, no winget PR.
- `just ci` runs `package-check` green on three OSes.

## Risk assessment

| Risk | Signal | Response |
|---|---|---|
| dist regenerates `release.yml` and drops the checksum patch | `dist plan` consistency error or the patch disappears | Keep the patch minimal and documented; `allow-dirty = ["ci"]` prevents overwrite; a test greps for the checksum step |
| musl build fails for a dependency | Link errors | Every current dependency is pure Rust; if one is not, stop and report rather than switching to glibc silently |
| ARM Linux runner unavailable | dist cannot schedule `aarch64-unknown-linux-musl` | Cross-compile with `cargo-zigbuild` only if dist supports it natively; otherwise drop that target and record it as a follow-up for the user's confirmation |
| Homebrew formula fails `brew audit` | Publish job would fail in phase 13 | Run `brew audit --new --formula` on the generated formula in the PR run (macOS job) before phase 13 |

## Security

- Every third-party action is SHA-pinned; the only unpinned network step is `brew update`, which is documented.
- Release secrets are added only in phase 13 by the user, as environment secrets in `release`. This phase's workflows reference them by name, expose them to one step, and never print them.
