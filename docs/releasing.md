# Releasing AriadShift

How a new version of AriadShift (`ashift` and the workspace crates `ariad-core`, `ariad-typst`, `ariad-host`, and `ariad-cli`) is prepared, promoted, tagged, verified, and submitted to package channels. The branch model is defined in [git-workflow.md](git-workflow.md), the CI gate is described in [ci.md](ci.md), and published releases are listed on [GitHub Releases](https://github.com/bavanchun/AriadShift/releases).

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
   - `[workspace.dependencies]` -> `ariad-host = { path = "crates/ariad-host", version = "X.Y.Z" }`
   - `[workspace.dependencies]` -> `ariad-typst = { path = "crates/ariad-typst", version = "X.Y.Z" }`

   The workspace crates (`crates/ariad-core/Cargo.toml`, `crates/ariad-typst/Cargo.toml`, `crates/ariad-host/Cargo.toml`, and `crates/ariad-cli/Cargo.toml`) inherit `version.workspace = true` and workspace dependency versions automatically.
3. Refresh [`Cargo.lock`](../Cargo.lock) and regenerate [`THIRD_PARTY_LICENSES`](../THIRD_PARTY_LICENSES) (which embeds the workspace crate versions checked by `just licenses-check`). `cargo-about` must be exact version `0.9.2` as pinned in [`justfile`](../justfile) (`cargo_about_version := "0.9.2"`; install with `cargo install --locked cargo-about --version 0.9.2` if needed):
   ```bash
   cargo check --workspace
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
5. Stage the explicit paths and commit as `chore(release): prepare X.Y.Z`:
   ```bash
   git add Cargo.toml Cargo.lock THIRD_PARTY_LICENSES
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

### 2. Promote `dev` to `main` with a merge commit

1. Confirm `dev` is green on all three OSes with no known regression, and obtain the maintainer's explicit go-ahead ([git-workflow.md](git-workflow.md) "Promotion and hotfixes").
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

- **`plan`**: Runs `cargo-dist` to compute the release manifest (`dist-manifest.json`), target matrix, and artifact list. On pull requests touching release paths, `release.yml` runs in rehearsal mode (`publishing == 'false'`) and stops after building artifacts. If `plan` fails, check `dist-workspace.toml` and `Cargo.toml` version consistency.
- **`build-local-artifacts`**: Builds `ashift` (`ashift.exe` on Windows) with `cargo-auditable` across the five release targets (`aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`, `x86_64-pc-windows-msvc`) and packages each archive with `LICENSE`, `NOTICE`, `THIRD_PARTY_LICENSES`, and a `.sha256` checksum. If a target build fails before `host` runs, no release has been published yet; fix the defect on `dev`, promote, and cut the next version tag.
- **`build-global-artifacts`**: Generates the shell and PowerShell installers, the Homebrew formula (`ashift.rb`), and CycloneDX SBOMs (`*.cdx.xml`) for `ariad-core`, `ariad-typst`, `ariad-host`, and `ariad-cli`.
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
- **`Publish crates in dependency order`** publishes `ariad-core`, `ariad-typst`, `ariad-host`, and `ariad-cli` sequentially. Before each crate, it queries the crates.io API (`User-Agent: AriadShift-Release-CI`); if HTTP `200` is returned, that crate version is already published and is skipped. After publishing a crate, it polls the crates.io API until HTTP `200` before moving to the next dependent crate. Because already-published crates are skipped, re-running a failed `publish` job after a transient registry timeout or token fix is idempotent.

### `release-smoke.yml` — what runs and what to do on failure

- Dispatched manually via `workflow_dispatch -f version=X.Y.Z` after `release.yml` completes (`GITHUB_TOKEN` release creation does not trigger `release: [published]` workflows).
- Runs four clean-runner jobs in parallel:
  - **`smoke-homebrew`**: Taps `bavanchun/homebrew-tap` on macOS, installs `ashift` (and `pandoc` via formula dependency), checks `ashift --version` and `ashift doctor`, and converts `fixtures/md/en-crlf.md` to `.docx`.
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
  - **`CARGO_REGISTRY_TOKEN`**: Scoped crates.io API token (`publish-new` for initial crate registration; `publish-update` restricted to `ariad-core`, `ariad-typst`, `ariad-host`, and `ariad-cli` for subsequent releases). Passed only to the `Validate registry token` and `Publish crates in dependency order` steps in `publish-crates.yml`.

[`scripts/check-release-workflow.sh`](../scripts/check-release-workflow.sh) enforces in CI that `HOMEBREW_TAP_TOKEN` and `CARGO_REGISTRY_TOKEN` appear nowhere else in `.github/workflows/release.yml` or `.github/workflows/publish-crates.yml`, and that both publishing jobs declare `environment: release`.

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
