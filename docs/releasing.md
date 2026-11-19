# Releasing AriadShift

How a new version of AriadShift (`ashift` and the published workspace crates `ariad-core`, `ariad-pdf`, `ariad-engine-io`, `ariad-typst`, `ariad-host`, and `ariad-cli`) is prepared, promoted, tagged, verified, and submitted to package channels. The branch model is defined in [git-workflow.md](git-workflow.md), the CI gate is described in [ci.md](ci.md), and published releases are listed on [GitHub Releases](https://github.com/bavanchun/AriadShift/releases).

Every step that touches `main`, GitHub environments, tags, or external package registries requires the maintainer's explicit go-ahead ([AGENTS.md](../AGENTS.md)).

`toolchain-wheels-v*` (and legacy `toolchain-torch-*`) tags identify reviewed
native build inputs for the [Docling pack](docling.md#the-pack), built and
published by [`.github/workflows/toolchain-wheels.yml`](../.github/workflows/toolchain-wheels.yml).
The product release workflow excludes them; they do not publish `ashift`, crates
or engine packs. Creating or replacing a toolchain release still requires
explicit authorization, and `engines/docling/pack/pins.toml` (`[toolchain]`)
pins each published wheel and sidecar by URL, `build_key`, size and SHA-256
(`licenses.py --render-toolchain-pins`) rather than resolving a moving release.

---

## Base PDF build size

The embedded Typst compiler increases the base CLI size; the browser build does not enable it. A slim CLI can be built with `cargo build -p ariad-cli --release --no-default-features`. The repository defines no binary or download size budget.

The measured GNU x86_64 Linux `dist` binary grew from 18,067,576 bytes (17.23 MiB) to 80,416,152 bytes (76.69 MiB). The baseline is the historical pre-feature measurement, before the sandbox and pack work, so it slightly overstates the PDF feature's own delta.

Release binaries and compressed archives measured directly from the [baseline rehearsal](https://github.com/bavanchun/AriadShift/actions/runs/39899966610) and the [PDF rehearsal](https://github.com/bavanchun/AriadShift/actions/runs/39992356754), all sizes in bytes:

| Target | Binary before | Binary after | Archive before | Archive after |
|---|---:|---:|---:|---:|
| `x86_64-unknown-linux-musl` | 22,835,864 | 79,115,288 | 5,020,292 | 18,142,284 |
| `aarch64-unknown-linux-musl` | 22,075,704 | 76,224,168 | 4,555,660 | 16,647,108 |
| `x86_64-apple-darwin` | 19,435,992 | 71,038,952 | 4,470,476 | 16,772,844 |
| `aarch64-apple-darwin` | 18,703,376 | 68,937,024 | 4,078,608 | 15,552,432 |
| `x86_64-pc-windows-msvc` | 16,747,520 | 60,802,048 | 6,334,287 | 24,004,827 |

Archives are the shipped `.tar.xz` files (Unix) or `.zip` file (Windows), including bundled release files. These figures exclude GitHub Actions' outer artifact ZIP wrapper. For comparison, the rounded artifact downloads recorded during review were:

| Target | Artifact download before | Artifact download after |
|---|---:|---:|
| x86_64 Linux musl | 5.02 MB | 18.15 MB |
| aarch64 Linux musl | 4.56 MB | 16.66 MB |
| x86_64 macOS | 4.47 MB | 16.78 MB |
| aarch64 macOS | 4.08 MB | 15.56 MB |
| x86_64 Windows | 6.33 MB | 23.96 MB |

MB is decimal; MiB is binary. These release builds differ from the GNU binary measurement above. The paired rehearsal runs provide a closer comparison than the historical GNU baseline.

---

## Release procedure (`vX.Y.Z`)

### 1. Bump the version on `dev` through a pull request

1. Create a release preparation branch from an up-to-date `dev`:
   ```bash
   git switch dev
   git pull --ff-only
   git switch -c chore/release-X.Y.Z
   ```
2. Update the workspace version entries in the root [`Cargo.toml`](../Cargo.toml):
   - `[workspace.package]` -> `version = "X.Y.Z"`
   - `[workspace.dependencies]` -> `ariad-core = { path = "crates/ariad-core", version = "X.Y.Z" }`
   - `[workspace.dependencies]` -> `ariad-engine-io = { path = "crates/ariad-engine-io", version = "X.Y.Z" }`
   - `[workspace.dependencies]` -> `ariad-ffi = { path = "crates/ariad-ffi", version = "X.Y.Z" }`
   - `[workspace.dependencies]` -> `ariad-pdf = { path = "crates/ariad-pdf", version = "X.Y.Z" }`
   - `[workspace.dependencies]` -> `ariad-host = { path = "crates/ariad-host", version = "X.Y.Z" }`
   - `[workspace.dependencies]` -> `ariad-typst = { path = "crates/ariad-typst", version = "X.Y.Z" }`

   The workspace crates (`crates/ariad-core/Cargo.toml`, `crates/ariad-pdf/Cargo.toml`, `crates/ariad-engine-io/Cargo.toml`, `crates/ariad-typst/Cargo.toml`, `crates/ariad-host/Cargo.toml`, and `crates/ariad-cli/Cargo.toml`) inherit `version.workspace = true` and workspace dependency versions automatically. The unpublished crates `ariad-server`, `ariad-wasm` and `ariad-desktop` inherit the same workspace version.

   Also set `"version": "X.Y.Z"` in [`apps/desktop/src-tauri/tauri.conf.json`](../apps/desktop/src-tauri/tauri.conf.json). Tauri reads it instead of the Cargo version, so it does not follow the workspace version automatically.
3. Refresh [`Cargo.lock`](../Cargo.lock) and regenerate [`THIRD_PARTY_LICENSES`](../THIRD_PARTY_LICENSES) (which embeds the workspace crate versions checked by `just licenses-check`). Also refresh the version-bound files in the same bump: [`fuzz/Cargo.lock`](../fuzz/Cargo.lock), the server snapshot [`crates/ariad-server/tests/snapshots/openapi_v1.json`](../crates/ariad-server/tests/snapshots/openapi_v1.json), [`schemas/openapi.v1.json`](../schemas/openapi.v1.json) `info.version`, and the SDK lock [`packages/sdk/generator.lock.json`](../packages/sdk/generator.lock.json) (`pnpm --filter @ariadshift/sdk generate` then `lock`); the release pull request must pass `just sdk-check` and `cargo check --locked --manifest-path fuzz/Cargo.toml`. `cargo-about` must be exact version `0.9.2` as pinned in [`justfile`](../justfile) (`cargo_about_version := "0.9.2"`; install with `cargo install --locked cargo-about --version 0.9.2` if needed):
   ```bash
   cargo check --workspace
   cargo update --workspace --manifest-path fuzz/Cargo.toml
   cargo check --locked --manifest-path fuzz/Cargo.toml
   just openapi
   ARIAD_UPDATE_OPENAPI_BASELINE=1 cargo test --locked -p ariad-server --test openapi_v1_snapshot
   pnpm --filter @ariadshift/sdk generate
   pnpm --filter @ariadshift/sdk lock
   just sdk-check
   just licenses
   ```
4. Run the local quality gate:
   ```bash
   just ci
   ```
   For a web release, verify the Pandoc licence/copyright and corresponding-source
   links shipped beside `pandoc.wasm` and on `/licenses`. Keep the corresponding
   source available for as long as that WASM build is distributed. If upstream
   stops serving it, provide an accessible copy before shipping; see
   [browser engine distribution](web.md#build-and-deploy).
   In a fresh worktree, run `pnpm --filter @ariadshift/web build` once before `just licenses-check` or `just web-build`: the notices sync reads the Next.js bundle analysis that only `next build --analyze` writes, so both fail on a tree that has never been built.
5. Stage the explicit paths and commit as `chore(release): prepare X.Y.Z`:
   ```bash
   git add Cargo.toml Cargo.lock THIRD_PARTY_LICENSES apps/desktop/src-tauri/tauri.conf.json fuzz/Cargo.lock crates/ariad-server/tests/snapshots/openapi_v1.json schemas/openapi.v1.json packages/sdk/generator.lock.json
   git diff --staged
   git commit -m "chore(release): prepare X.Y.Z"
   ```
   Root markdown is limited to `README.md`, `ARCHITECTURE.md`, `AGENTS.md`, and `CLAUDE.md` ([AGENTS.md](../AGENTS.md)), so there is no `CHANGELOG.md` at the repository root; release notes are generated by `dist` in the GitHub Release.
6. Push the branch and open a pull request into `dev` (pass `--body` or `--fill` so `gh pr create` works non-interactively):
   ```bash
   git push -u origin chore/release-X.Y.Z
   gh pr create --base dev --title "chore(release): prepare X.Y.Z" --body "Prepare release vX.Y.Z."
   ```
   Because `Cargo.toml`, `Cargo.lock`, and `THIRD_PARTY_LICENSES` match the `on.pull_request.paths` filter in [`.github/workflows/release.yml`](../.github/workflows/release.yml), this pull request runs both `ci.yml` and `release.yml` in build-and-upload rehearsal mode (`pr-run-mode = "upload"`), building all five platform archives without publishing.
7. Once `ci.yml` (all three OSes) and `release.yml` are green, merge into `dev`:
   ```bash
   gh pr merge --rebase --delete-branch
   ```
   When a pull request into `dev` is squash-merged, GitHub appends ` (#NNN)` to its title. Keep the title plus that suffix at 72 characters or fewer, or the `lint-commits` check on the next promotion fails (seen with #396 and #411; see #425).

### 2. Promote `dev` to `main` with a merge commit

1. Confirm `dev` is green (`ci.yml` `CI passed` on the `dev` head) and that the heavy workflows (Docling pack `pack-docling.yml`, Cloud E2E `cloud-e2e.yml`, Release rehearsal `release.yml`, and required engine pack workflows `pack-*`) are green on `dev`, with no known regression, and obtain the maintainer's explicit go-ahead ([git-workflow.md](git-workflow.md) "Promotion and hotfixes").
2. Open a promotion pull request from `dev` into `main` titled `chore: promote dev to main`:
   ```bash
   gh pr create --base main --head dev --title "chore: promote dev to main" --body "Promote dev to main for vX.Y.Z."
   ```
3. Wait for the CI gate on the pull request (`ci.yml` runs Scope and Static checks and skips duplicate build, test, and fuzz jobs when the tree matches a green `dev` push run).
4. Merge the promotion pull request with a **merge commit** — never squash, never rebase, and never pass `--delete-branch`:
   ```bash
   gh pr merge --merge
   ```
   Using a merge commit preserves commit SHAs across `dev` and `main` so future promotions remain clean merges.

### 3. Tag `vX.Y.Z` on the `main` merge commit

1. Update local `main` and verify that `HEAD` is the promotion merge commit whose second parent is `origin/dev`:
   ```bash
   git switch main
   git pull --ff-only
   git rev-parse HEAD^2
   test "$(git rev-parse HEAD^2)" = "$(git rev-parse origin/dev)"
   git log -1 --oneline
   ```
2. Create and push the version tag on that `main` merge commit:
   ```bash
   git tag vX.Y.Z
   git push origin vX.Y.Z
   ```
3. Resolve the triggered run ID and watch [`.github/workflows/release.yml`](../.github/workflows/release.yml) to completion:
   ```bash
   run_id=$(gh run list --workflow=release.yml --branch=vX.Y.Z --limit 1 --json databaseId --jq '.[0].databaseId')
   gh run watch "$run_id" --exit-status
   ```
   Before proceeding to smoke testing, verify that `host`, `prepare-homebrew-formula`, `publish-homebrew-formula`, and `custom-publish-crates` all succeeded individually (see the `announce` note under [Workflows](#workflows)).

### 4. Run the post-release smoke workflow

Trigger [`.github/workflows/release-smoke.yml`](../.github/workflows/release-smoke.yml) against the newly published version and watch all four clean-runner channel jobs pass:

```bash
gh workflow run release-smoke.yml -f version=X.Y.Z
run_id=$(gh run list --workflow=release-smoke.yml --limit 1 --json databaseId --jq '.[0].databaseId')
gh run watch "$run_id" --exit-status
```

### 5. Generate and submit WinGet manifests

Once the GitHub Release assets for `vX.Y.Z` are live, generate the WinGet manifests with [`scripts/winget-manifest.sh`](../scripts/winget-manifest.sh) and submit them to [`microsoft/winget-pkgs`](https://github.com/microsoft/winget-pkgs) (see [WinGet submission](#winget-submission) below).

### 6. Verify no `TBD-AFTER-RELEASE` placeholders remain

Before merging any release close-out documentation update into `dev`, verify that no unreplaced `TBD-AFTER-RELEASE` placeholder token remains in `README.md`, `AGENTS.md`, or `docs/` (only this section in `docs/releasing.md` should match):

```bash
grep -rn TBD-AFTER-RELEASE README.md AGENTS.md docs
```

---

## Workflows

The workflow table and supply-chain pinning rules live in [ci.md](ci.md). Exact runner labels, action SHAs, and pinned tool versions are owned by [`.github/workflows/release.yml`](../.github/workflows/release.yml), [`.github/workflows/publish-crates.yml`](../.github/workflows/publish-crates.yml), [`.github/workflows/release-smoke.yml`](../.github/workflows/release-smoke.yml), [`dist-workspace.toml`](../dist-workspace.toml), and [`scripts/check-release-workflow.sh`](../scripts/check-release-workflow.sh) (`just check-release-workflow`).

CLI archives use precise package builds so the desktop WebView dependencies are
excluded from musl targets. Desktop installers and updater metadata are built by
[the desktop workflow](../.github/workflows/desktop.yml); see
[desktop release configuration](desktop.md#updates-and-release-configuration).

### `release.yml` — what runs and what to do on failure

Product release tag runs match only `v*` tags (excluding `pack-*` and `toolchain-*` tags) and verify that the tagged commit is an ancestor of `origin/main` before planning. Pull request rehearsals do not require that ancestry.

- **`plan`**: Runs `cargo-dist` to compute the release manifest (`dist-manifest.json`), target matrix, and artifact list. On pull requests touching release paths, `release.yml` runs in rehearsal mode (`publishing == 'false'`) and stops after building artifacts. If `plan` fails, check `dist-workspace.toml` and `Cargo.toml` version consistency.
- **`build-local-artifacts`**: Builds `ashift` (`ashift.exe` on Windows) with `cargo-auditable` across the five release targets (`aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`, `x86_64-pc-windows-msvc`) and packages each archive with `LICENSE`, `NOTICE`, `THIRD_PARTY_LICENSES`, and a `.sha256` checksum. If a target build fails before `host` runs, no release has been published yet; fix the defect on `dev`, promote, and cut the next version tag.
- **`build-global-artifacts`**: Generates the shell and PowerShell installers, the Homebrew formula (`ashift.rb`), and CycloneDX SBOMs (`*.cdx.xml`) for `ariad-core`, `ariad-pdf`, `ariad-engine-io`, `ariad-pdfium-engine`, `ariad-typst`, `ariad-host`, `ariad-cli`, `ariad-server`, `ariad-wasm`, and `ariad-desktop`.
- **`host`** (`contents: write`, tag runs only): Uploads all archives, checksums, installers, and SBOMs and creates the GitHub Release via `gh release create`. Once `host` succeeds, the GitHub Release and its assets are public.
- **`prepare-homebrew-formula`** (unprivileged, no secrets): Checks out `bavanchun/homebrew-tap` without credentials, downloads all `artifacts-*` workflow artifacts into `homebrew-tap/Formula/` (`merge-multiple: true`), runs `brew update` and `brew style --except-cops FormulaAudit/Homepage,FormulaAudit/Desc,FormulaAuditStrict --fix "Formula/${filename}" || true`, and uploads `homebrew-tap/Formula/*.rb` as the `prepared-homebrew-formula` artifact. If `brew update` or the tap checkout fails here, re-run the job; `host` does not need to re-run.
- **`publish-homebrew-formula`** (`environment: release`): Checks out `bavanchun/homebrew-tap`, downloads `prepared-homebrew-formula` into `Formula/`, commits `Formula/ashift.rb` (or logs `Formula already up-to-date` on re-run), and pushes `HEAD:main` using `HOMEBREW_TAP_TOKEN` (masked via `::add-mask::`). No `brew` commands run in this job. Safe to re-run if the push fails.
- **`custom-publish-crates`**: Calls [`.github/workflows/publish-crates.yml`](../.github/workflows/publish-crates.yml) with `CARGO_REGISTRY_TOKEN`.
- **`announce`**: Gate job depending on `plan`, `host`, `prepare-homebrew-formula`, `publish-homebrew-formula`, and `custom-publish-crates`.
  > [!IMPORTANT]
  > `announce`'s `if:` condition requires `needs.host.result == 'success'` and allows `publish-homebrew-formula` and `custom-publish-crates` to be either `'success'` or `'skipped'` (to support prereleases). Because `publish-homebrew-formula` depends on `prepare-homebrew-formula`, a failure in `prepare-homebrew-formula` causes `publish-homebrew-formula` to be `skipped`, allowing `announce` — and the overall workflow run — to finish green without updating the Homebrew formula. Always confirm that `prepare-homebrew-formula` and `publish-homebrew-formula` both succeeded before running `release-smoke.yml`.

### `publish-crates.yml` — what runs and what to do on failure

- Runs inside `environment: release` (`v*` tags only).
- **`Validate registry token`** fails closed before any `cargo publish` if `CARGO_REGISTRY_TOKEN` is unset or empty.
- **`Publish crates in dependency order`** publishes `ariad-core`, `ariad-pdf`, `ariad-engine-io`, `ariad-typst`, `ariad-host`, and `ariad-cli` sequentially, and `just check-release-workflow` fails if that list or its order drifts. Before each crate, it queries the crates.io API (`User-Agent: AriadShift-Release-CI`); if HTTP `200` is returned, that crate version is already published and is skipped. After publishing a crate, it polls the crates.io API until HTTP `200` before moving to the next dependent crate. Because already-published crates are skipped, re-running a failed `publish` job after a transient registry timeout or token fix is idempotent.

### `release-smoke.yml` — what runs and what to do on failure

- Dispatched manually via `workflow_dispatch -f version=X.Y.Z` after `release.yml` completes (`GITHUB_TOKEN` release creation does not trigger `release: [published]` workflows).
- Runs four clean-runner jobs in parallel:
  - **`smoke-homebrew`**: Taps `bavanchun/homebrew-tap` on macOS, trusts the tap (`brew trust --tap bavanchun/tap`), installs `ashift` (and `pandoc` via formula dependency), checks `ashift --version` and `ashift doctor`, and converts `fixtures/md/en-crlf.md` to `.docx`.
  - **`smoke-shell`**: Installs Pandoc, runs the release's `ariad-cli-installer.sh` on Linux, and verifies `--version`, `doctor`, and `.md -> .docx` conversion.
  - **`smoke-powershell`**: Installs Pandoc, runs `ariad-cli-installer.ps1` in `pwsh` on Windows, reloads `Path`, and verifies `--version`, `doctor`, and `.md -> .docx` conversion.
  - **`smoke-binstall`**: Installs Pandoc and `cargo-binstall` on Linux, installs `ariad-cli@<version>` with `--disable-strategies compile,quick-install` (forcing prebuilt binary resolution from GitHub Releases), and verifies `--version`, `doctor`, and `.md -> .docx` conversion.
- If a smoke job fails, inspect whether the channel artifact (Homebrew formula commit, GitHub Release installer/archive, or crates.io `[package.metadata.binstall]` metadata) is missing or out of sync before announcing the release.

---

## The `release` environment and secrets

Repository `bavanchun/AriadShift` defines a GitHub Actions environment named **`release`**:

- **Deployment protection rule**: Restricted to tags matching `v*` only. Branch pushes and pull requests cannot enter the `release` environment or read its secrets.
- **Required environment secret names** (created and set directly by the maintainer, e.g. `gh secret set <NAME> --env release`; never committed or echoed):
  - **`HOMEBREW_TAP_TOKEN`**: Fine-grained personal access token scoped exclusively to repository `bavanchun/homebrew-tap` with `contents: write` permission only. Passed via step-level `env` only to the `Push formula` step in `publish-homebrew-formula` (`release.yml`).
  - **`CARGO_REGISTRY_TOKEN`**: Scoped crates.io API token (`publish-new` for initial crate registration; `publish-update` restricted to `ariad-core`, `ariad-pdf`, `ariad-engine-io`, `ariad-typst`, `ariad-host`, and `ariad-cli` for subsequent releases; the first release that contains `ariad-pdf` and `ariad-engine-io` registers those two crate names and therefore needs `publish-new` as well). Passed only to the `Validate registry token` and `Publish crates in dependency order` steps in `publish-crates.yml`.
  - **`APPLE_CERTIFICATE`**: Base64-encoded Developer ID Application `.p12` certificate. Imported into a temporary keychain by `.github/workflows/macos.yml`.
  - **`APPLE_CERTIFICATE_PASSWORD`**: Password protecting the Developer ID `.p12` certificate.
  - **`APPLE_SIGNING_IDENTITY`**: Full identity string for code signing (e.g. `Developer ID Application: <Name> (<TEAMID>)`).
  - **`APPLE_API_KEY_ID`**: App Store Connect API Key ID for `notarytool`.
  - **`APPLE_API_ISSUER`**: App Store Connect Issuer UUID for `notarytool`.
  - **`APPLE_API_KEY`**: App Store Connect `.p8` private key (PEM string or base64-encoded) for `notarytool`.
  - **`SPARKLE_ED_PRIVATE_KEY`**: Sparkle 2 EdDSA private key (44-character base64 string representing the 32-byte private seed, exported via `generate_keys -x <file>`) passed via stdin to `generate_appcast`.

In addition, the repository or environment variable **`vars.SPARKLE_PUBLIC_ED_KEY`** (an environment variable of the `release` environment takes precedence over a repository variable of the same name) stores the corresponding public key, injected during signed and ad-hoc release builds into `AriadShift.app/Contents/Info.plist` under `SUPublicEDKey`. Development builds and an unsigned (never published) build leave the key empty, keeping the updater disabled.

[`scripts/check-release-workflow.sh`](../scripts/check-release-workflow.sh) enforces in CI that `HOMEBREW_TAP_TOKEN` and `CARGO_REGISTRY_TOKEN` appear nowhere else in `.github/workflows/release.yml` or `.github/workflows/publish-crates.yml`, and that both publishing jobs declare `environment: release`.

---

## macOS release, signing, and updates

Native macOS app releases (`AriadShift.app`) are packaged, signed (Developer ID or ad-hoc, see the signing modes below), notarized when Developer ID is available, and published by [`.github/workflows/macos.yml`](../.github/workflows/macos.yml).

### Artifacts and packaging

1. **`AriadShift-<version>-universal.dmg`**: Universal disk image containing both `arm64` and `x86_64` architecture slices of `AriadShift.app` and its embedded `Contents/Helpers/ashift` binary. Includes an `/Applications` symlink for drag-and-drop installation. Built via [`scripts/macos-dmg.sh`](../scripts/macos-dmg.sh) using `ditto`, from the notarized, stapled app bundle in signed mode or from the ad-hoc signed app bundle in adhoc mode.
2. **`appcast.xml`**: Sparkle 2 RSS update feed containing version metadata, OS requirements (`LSMinimumSystemVersion` 15.0), enclosure file size, absolute download URL, and `sparkle:edSignature` EdDSA signatures. Generated by [`scripts/macos-appcast.sh`](../scripts/macos-appcast.sh) from pinned Sparkle 2.10.1 tools. Produced in signed and adhoc modes; in signed mode only when `SPARKLE_ED_PRIVATE_KEY` is configured (otherwise the signed DMG is published without an appcast), while adhoc mode fails without it. Never produced in unsigned mode.
3. **Signing modes.** The workflow's `mode` output has three values, decided once before the build:
   - **`signed`**: all six Apple secrets (`APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_API_KEY_ID`, `APPLE_API_ISSUER` and `APPLE_API_KEY`) exist. The app is signed with Developer ID, notarized and stapled, and the DMG and appcast are published. This is the path described below and is unchanged.
   - **`adhoc`**: the Apple secrets are all absent and `SPARKLE_ED_PRIVATE_KEY` exists. The app is ad-hoc signed, packaged into `AriadShift-<version>-universal.dmg`, and published with an `appcast.xml` signed by the Sparkle EdDSA key. There is no notarization, so users follow the first-launch steps in [macos.md](macos.md#install). This replaces the former rule that an unsigned build is never published: an ad-hoc build is published only because Sparkle's EdDSA signature protects updates.
   - **`unsigned`**: the Apple secrets are absent and `SPARKLE_ED_PRIVATE_KEY` is absent too. Nothing is published; the DMG is a workflow artifact (kept 7 days) with the updater disabled (empty `SUPublicEDKey`), as before, and the publish job does not run.

   A partially populated set of Apple secrets fails the run (fail closed; [`scripts/macos-check-secrets.sh`](../scripts/macos-check-secrets.sh) lists the missing names and [`scripts/macos-check-secrets-workflow.test.sh`](../scripts/macos-check-secrets-workflow.test.sh) runs the workflow's own step to prove it), and a guard step fails the job if the mode is not one of `signed`, `adhoc` or `unsigned`. The publish job uploads only fixed file names (no globs).

   The Tauri app (`apps/desktop`) no longer ships for macOS; this Swift app is the only macOS release artifact (Tauri stays on Windows and Linux).

### Release pipeline flow and secret protection

- Signing, notarization and DMG creation run **only on `v*` tags under `environment: release`**, in the `release-sign` job (runner `xcode-27`, see [ci.md](ci.md#macos-workflow-runner); `NOTARY_TIMEOUT` 20 minutes per submission, job timeout 120 minutes). The separate `release-publish` job (`needs: release-sign`, Linux, only for a signed or adhoc build, no environment secrets) waits for the CLI workflow to create the GitHub Release and attaches the DMG (Developer ID or ad-hoc signed) and the appcast.
- The first step of `release-sign` checks out the full history and requires the tagged commit to be an ancestor of `origin/main` (`git merge-base --is-ancestor`, the same gate as the pack workflows), before any step that holds a secret. This is defense in depth: the `release` environment must still have a required reviewer and a deployment rule limited to `v*` tags, which cannot be checked from the repository.
- The signing mode is decided right after that gate, before the build. Before the build, the job requires a semantic-version tag and, **in signed and adhoc modes**, a `vars.SPARKLE_PUBLIC_ED_KEY` that matches `^[A-Za-z0-9+/=]{44}$`.
- The `release-sign` job executes the following sequential pipeline:
  0. Fetch the pinned Pandoc for both app architectures (`scripts/install-pandoc.sh --macos-arch arm64` and `x86_64`, each archive checked against its SHA-256). The build embeds it as `Contents/Helpers/pandoc` with its licence notices; see [macos.md](macos.md#bundled-pandoc). This adds about 315 MB to the app on disk and about 69 MB to the download.
  1. Build the universal Release app via `just macos-build`, injecting `MARKETING_VERSION` (from the release tag), `CURRENT_PROJECT_VERSION` (the GitHub run number), `SPARKLE_PUBLIC_ED_KEY` (from `vars.SPARKLE_PUBLIC_ED_KEY`, signed and adhoc modes; the unsigned build keeps the project default, an empty key) and `CODE_SIGN_INJECT_BASE_ENTITLEMENTS=NO`. The recipe forwards each setting as one quoted argument.
  2. Verify that the built `Info.plist` has `CFBundleShortVersionString` equal to the tag version, `CFBundleVersion` equal to the run number and, in signed and adhoc modes, `SUPublicEDKey` equal to `vars.SPARKLE_PUBLIC_ED_KEY`. In unsigned mode the step instead fails unless `SUPublicEDKey` is empty or absent (updater disabled).
  3. **Signed mode only:** sign all nested components inside-out (Sparkle XPC services, Autoupdate, Updater.app, Sparkle.framework, loose dylibs, then frameworks, every Mach-O file in `Contents/Helpers` (the `ashift` helper and Pandoc), outer app) with `--options runtime --timestamp`. The walk is the one in [`scripts/macos-signing-lib.sh`](../scripts/macos-signing-lib.sh), shared with the ad-hoc script, so a dylib inside a framework is signed before the framework seals it in both modes. Nested components keep the entitlements they were built with (`--preserve-metadata=entitlements,requirements,flags,runtime`, as Sparkle documents). The outer app is signed with the explicit minimal entitlements file [`scripts/macos-app.entitlements`](../scripts/macos-app.entitlements) instead of whatever the build injected.
  4. **Signed mode only:** verify every signed bundle with `codesign --verify --deep --strict` and `codesign -dvv`, and **fail when any bundle carries `com.apple.security.get-task-allow`** (read with `codesign -d --entitlements - --xml`). Apple notarization rejects that debug entitlement, and this gate stops the job before anything is submitted. `bash scripts/macos-sign-notarize.sh verify-app <app>` runs the same checks on an existing app.
  5. **Signed mode only:** package the app bundle into a zip with `ditto -c -k --keepParent` and submit it to Apple `notarytool submit --wait --output-format json`. Require JSON `status == "Accepted"`.
  6. **Signed mode only:** staple the notarization ticket to the app (`xcrun stapler staple`) and verify with `spctl --assess --type exec`.
  7. **Signed mode only:** package the stapled app into the release DMG using `ditto` and verify image integrity with `hdiutil verify`.
  8. **Signed mode only:** sign the release DMG, submit to `notarytool`, require `status == "Accepted"`, staple the ticket to the DMG, and assess with `spctl --assess --type open --context context:primary-signature`.
  9. Stage the DMG alone in a dedicated directory and generate `appcast.xml` via `generate_appcast` with `--download-url-prefix "https://github.com/${GITHUB_REPOSITORY}/releases/download/${RELEASE_TAG}/"`. In signed mode this uses the notarized DMG and is skipped with a warning when `SPARKLE_ED_PRIVATE_KEY` is not set; in adhoc mode it is mandatory (see the adhoc steps below).
  10. Upload the DMG and appcast as the workflow artifact `AriadShift-release-assets` (a backup that survives a failed publish; `overwrite: true` lets a re-run of all jobs replace it), then clean up the temporary keychain in an unconditional `always()` step. The job exports the SHA-256 of the DMG and of the appcast as job outputs, and `release-publish` downloads the artifact, runs `sha256sum -c` against those outputs and only then uploads with `GH_REPO` set (the job has no checkout, so `gh` needs the repository explicitly).
- **Adhoc mode replaces steps 3 to 9** (no Apple credential is involved):
  1. Ad-hoc sign the built app with [`scripts/macos-adhoc-sign.sh`](../scripts/macos-adhoc-sign.sh) (`app` subcommand), walking the same bundle in the same order as signed mode, inside-out and never with `--deep` for signing, with the hardened runtime and `--timestamp=none`. The script then gates on `codesign --verify --deep --strict`, the ad-hoc and runtime flags, and the absence of `com.apple.security.get-task-allow` for every signed target, failing before any DMG exists.
  2. Build `AriadShift-<tag>-universal.dmg` from the ad-hoc signed app with `scripts/macos-dmg.sh`. There is no notarization, stapling or `spctl` assessment.
  3. Generate `appcast.xml` with `scripts/macos-appcast.sh` and `SPARKLE_ED_PRIVATE_KEY`; the step fails if the key is empty, the appcast is missing, it carries no `sparkle:edSignature`, or its URL is not the release DMG that `release-publish` attaches.
  4. The artifact upload, digest export and `release-publish` steps (step 10 above) are the same as in signed mode; `release-publish` also fails if an adhoc release has no appcast.
- Notarization, stapling and `spctl` assessment run in signed mode only.
- Branch pushes and pull requests run non-signing verification (`just macos-build` and `just macos-test`) on `xcode-27` and upload the unsigned application bundle compressed as a zip via `ditto -c -k --keepParent`. Run the workflow green on a pull request once before the first release tag.
- Secrets are scoped with least privilege: the sign/notarize step receives only the six Apple credentials, while the appcast step receives only `SPARKLE_ED_PRIVATE_KEY`. The publish job holds only the workflow `GITHUB_TOKEN`.
- Secrets are never echoed, never committed, and never passed on command-line arguments. In CI, a temporary keychain ending in `.keychain-db` is created in `$RUNNER_TEMP`, used for signing via explicit `--keychain`, and unconditionally deleted in `always()`. The imported `.p12` is decoded under `umask 077` and deleted immediately after import. The notary API key and Sparkle EdDSA key are written to restricted temporary files (`0600`) or piped directly via standard input (`--ed-key-file -`).
- Build numbers: `CURRENT_PROJECT_VERSION` is the workflow run number, which only grows per workflow. Sparkle orders updates by `CFBundleVersion`. A hotfix tag cut from an older branch after a newer release still gets a higher build number than that release, so Sparkle offers it as an update even though its marketing version is lower. Cut hotfixes from the current release line, or bump the marketing version past the newest release.

### Sparkle key runbook

#### 1. Key storage
- The Sparkle EdDSA private key is generated using Sparkle's `generate_keys` tool and exported with `generate_keys -x <file>`. The file content is a 44-character base64 string representing the 32-byte Ed25519 private seed.
- The repository owner holds the key. It is stored **only** as `SPARKLE_ED_PRIVATE_KEY` in the GitHub Actions `release` environment, plus one offline, cold-storage backup (e.g., encrypted flash drive in a secure safe). The public key is not secret and is stored as described in the next bullet. Losing the key means installed copies cannot be updated by Sparkle, so the backup is mandatory.
- The corresponding public key is stored in the variable `vars.SPARKLE_PUBLIC_ED_KEY` (the canonical location, which `macos.yml` reads; a `release` environment variable of the same name takes precedence over a repository variable) and injected at build time into `apps/macos/AriadShift/Info.plist` under `SUPublicEDKey`.
- **Setup order matters.** Set `vars.SPARKLE_PUBLIC_ED_KEY` first, then add `SPARKLE_ED_PRIVATE_KEY` to the `release` environment. With only the secret set, a `v*` tag fails at the public-key check before anything is built. With neither set, and no Apple secrets, the tag builds in `unsigned` mode (nothing is published), which is the safe default. A key that is only whitespace counts as set for mode selection (GitHub cannot trim a secret), so the build runs, but the appcast step fails on the blank key, `release-sign` fails and nothing is published.

#### 2. Key rotation
Sparkle 2 does not use or recognize `SUOldPublicEDKeys`. To rotate the EdDSA update signing key:
1. Generate a new Ed25519 keypair using Sparkle's `generate_keys` tool and export the private key with `generate_keys -x <new-key-file>`.
2. Update the public key in `vars.SPARKLE_PUBLIC_ED_KEY` to the new public key.
3. Build and release an intermediate app version (e.g. `vX.Y.0`) signed and published with the **old** `SPARKLE_ED_PRIVATE_KEY`. Existing installed clients verify the update with their current trusted key and receive the new `SUPublicEDKey`. In ad-hoc mode the EdDSA key alone protects updates; there is no Developer ID identity to check as a second barrier.
4. After allowing a transition period for users to update, replace `SPARKLE_ED_PRIVATE_KEY` in the `release` environment with the new private key export. All subsequent releases and appcasts will then be signed with the new key.

#### 3. Compromise response
If `SPARKLE_ED_PRIVATE_KEY` is suspected or confirmed compromised:
1. **Immediate revocation in CI**: Delete `SPARKLE_ED_PRIVATE_KEY` from the `release` environment immediately (`gh secret delete SPARKLE_ED_PRIVATE_KEY --env release`) to prevent unauthorized feed publication.
2. **Generate replacement key**: Generate a fresh keypair offline using `generate_keys` and export via `generate_keys -x`.
3. **Emergency release**: Note that Gatekeeper is NOT the barrier for Sparkle in-app updates (Sparkle applies updates directly without Gatekeeper quarantine). In ad-hoc mode the only cryptographic barrier is Sparkle's EdDSA signature verification, so whoever holds `SPARKLE_ED_PRIVATE_KEY` can publish updates that installed clients accept: treat a leak as a full compromise of the update channel. (Only in signed mode do installed clients also check the Developer ID code-signing identity, which an attacker holding only the EdDSA key lacks.) Cut an immediate patch release with the new public key in `vars.SPARKLE_PUBLIC_ED_KEY`, signed with the new EdDSA key, and with Developer ID and notarization when that path is available. Installed copies still trust the old key, so users of an already compromised copy may need to reinstall manually.
4. **Purge compromised assets**: Remove any unauthorized release assets or tampered `appcast.xml` files from GitHub Releases.
5. **Feed invalidation**: Push the authentic, freshly generated `appcast.xml` signed with the replacement key.

### Code ownership enforcement
`.github/CODEOWNERS` defines required maintainer review (`@bavanchun`) for release workflows (`.github/workflows/**`), `.github/CODEOWNERS`, macOS release scripts and entitlements (`scripts/macos-*`), the `justfile`, Xcode configuration (`apps/macos/project.yml`), app build scripts (`apps/macos/scripts/`, including the helper embedding), app metadata (`apps/macos/AriadShift/Info.plist`), FFI framework build automation (`scripts/build-xcframework.sh`) and this release guide. Note that GitHub only enforces code-owner review on pull requests if the repository maintainer has enabled the "Require review from Code Owners" setting in the repository ruleset (`protect-main-dev`), because automation does not alter repository rulesets.

---

## WinGet submission

WinGet multi-file manifests (schema `1.9.0`, package identifier `VChun.AriadShift`, moniker `ashift`) are rendered from [`packaging/winget/*.yaml.tmpl`](../packaging/winget/) by [`scripts/winget-manifest.sh`](../scripts/winget-manifest.sh):

```bash
scripts/winget-manifest.sh <version> [archive_path] [output_dir]
```

1. **Generate the manifests**:
   ```bash
   scripts/winget-manifest.sh X.Y.Z
   ```
   - Validates that `<version>` matches `^[0-9]+\.[0-9]+\.[0-9]+$` with no newlines.
   - When `[archive_path]` is omitted (recommended after the GitHub Release is published), fetches `https://github.com/bavanchun/AriadShift/releases/download/vX.Y.Z/ariad-cli-x86_64-pc-windows-msvc.zip.sha256` and validates the 64-character hex SHA-256 digest. When `[archive_path]` is provided, the script invokes `sha256sum`; on stock macOS where `/usr/bin/shasum` (`shasum -a 256`) is installed without `sha256sum`, omit `[archive_path]` so the published `.sha256` asset is fetched over HTTPS (or provide a `sha256sum` wrapper calling `shasum -a 256 "$@"`).
   - Writes the three rendered manifest files into `target/winget/X.Y.Z/` (or `[output_dir]`):
     - `VChun.AriadShift.yaml` (`ManifestType: version`)
     - `VChun.AriadShift.installer.yaml` (`ManifestType: installer`, `InstallerType: zip`, `NestedInstallerType: portable`, `RelativeFilePath: ashift.exe`, `PortableCommandAlias: ashift`, `Architecture: x64`)
     - `VChun.AriadShift.locale.en-US.yaml` (`ManifestType: defaultLocale`, `Publisher: VChun`, `PackageName: AriadShift`, `Moniker: ashift`, `License: Apache-2.0`)
2. **Submit to `microsoft/winget-pkgs`**:
   - **macOS / Linux route (`gh` and `git`)**: Fork [`microsoft/winget-pkgs`](https://github.com/microsoft/winget-pkgs), create a branch from `upstream/master`, copy the three generated YAML files from `target/winget/X.Y.Z/` into `manifests/v/VChun/AriadShift/X.Y.Z/`, push the branch to the fork, and open a pull request against `master`:
     ```bash
     gh repo fork microsoft/winget-pkgs --clone=false
     git clone --depth 1 --filter=blob:none "https://github.com/<maintainer>/winget-pkgs.git" /tmp/winget-pkgs
     cd /tmp/winget-pkgs
     git remote add upstream https://github.com/microsoft/winget-pkgs.git 2>/dev/null || true
     git fetch --depth 1 upstream master
     git switch -c "ariadshift-X.Y.Z" upstream/master
     mkdir -p "manifests/v/VChun/AriadShift/X.Y.Z"
     cp "/path/to/AriadShift/target/winget/X.Y.Z/"*.yaml "manifests/v/VChun/AriadShift/X.Y.Z/"
     git add "manifests/v/VChun/AriadShift/X.Y.Z"
     git commit -m "New version: VChun.AriadShift version X.Y.Z"
     git push -u origin "ariadshift-X.Y.Z"
     gh pr create --repo microsoft/winget-pkgs --base master --head "<maintainer>:ariadshift-X.Y.Z" \
       --title "New version: VChun.AriadShift version X.Y.Z" \
       --body "Submit VChun.AriadShift version X.Y.Z."
     ```
   - **Windows route (`wingetcreate`)**: `wingetcreate` (MIT-licensed, `wingetcreate submit` / `wingetcreate update`) is **Windows-only** (distributed only as `.exe` and `.msixbundle` assets) and can be used when running on Windows.
   - Sign the Microsoft Contributor License Agreement (CLA) on the pull request if prompted.
   - Never use `winget-releaser` (licensed under AGPL-3.0, which is prohibited by [AGENTS.md](../AGENTS.md)).

---

## Failure responses

The desktop workflow verifies the merged `latest.json` after all three release
jobs finish: Linux x86-64, Apple silicon and Windows x86-64 must reference
attached assets whose signatures match the committed updater public key.
Its verification job fails on missing entries, concurrent upload loss or a key
mismatch. Repair the desktop assets before announcing the release.

The app uses GitHub's `releases/latest/download/latest.json` endpoint. A newer
CLI-only release without desktop assets makes this endpoint return 404; in-app
updates pause until the next release containing desktop metadata. Local
conversion remains available.

| Failure mode | Signal | Response |
|---|---|---|
| `release.yml` fails mid-publish | Some artifacts uploaded or GitHub Release created, but formula push or crate publish did not complete | Do not delete the tag silently. Report the failure. Because `publish-homebrew-formula` and `publish-crates.yml` are idempotent, re-run the failed publish job if `host` already succeeded. If any artifact or crate was published with a defect that requires code changes, fix forward with the next patch version (for example `v0.1.1`) and ask the maintainer before deleting a GitHub Release. |
| A publish job is refused by crates.io or by the tap | `Publish crates in dependency order` fails with `401 Unauthorized`, or the `Push formula` step fails with `could not read Username for 'https://github.com'` | The `release` environment secret is set but does not hold a working token; `Validate registry token` only rejects an empty value. The maintainer stores a valid token (`gh secret set <NAME> --env release`), then re-runs only the failed jobs of the tag run with `gh run rerun <run-id> --failed`. `host` and the GitHub Release stay as they are, and `release-smoke.yml` runs after the re-run succeeds. |
| Homebrew formula preparation, push, or smoke fails | `brew update` or tap checkout fails in `prepare-homebrew-formula`, `git push` fails in the `Push formula` step of `publish-homebrew-formula`, or `smoke-homebrew` fails in `release-smoke.yml` (`brew style --fix` in `prepare-homebrew-formula` ends with `|| true` and cannot fail the job) | Check `bavanchun/homebrew-tap` accessibility and `HOMEBREW_TAP_TOKEN` validity (`contents: write` on `bavanchun/homebrew-tap`). If `host` already succeeded and the issue is transient or a token/tap configuration problem, re-run `prepare-homebrew-formula` and `publish-homebrew-formula`; the published GitHub Release stays intact. Because `announce` allows `publish-homebrew-formula` to be `skipped`, always verify that `prepare-homebrew-formula` and `publish-homebrew-formula` both succeeded before running `release-smoke.yml`. If the generated formula itself is broken, fix forward with the next patch release. |
| WinGet validation flags the binary | Automated bot comments or validation checks fail on the `microsoft/winget-pkgs` pull request | Address the manifest issue or respond on the pull request. Unsigned binaries are permitted for portable `.zip` packages in WinGet, but the archive must pass Microsoft Defender, SmartScreen, and multi-engine antivirus scans. |
| Token leak | A token value appears in workflow logs or terminal output | Tokens live only in GitHub `release` environment secrets set by the maintainer, and workflows never echo them (`HOMEBREW_TAP_TOKEN` HTTP auth is masked via `::add-mask::`). If a token is ever exposed, revoke and rotate it immediately on GitHub or crates.io and replace the secret in the `release` environment. |
