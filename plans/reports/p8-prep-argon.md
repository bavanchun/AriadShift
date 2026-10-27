# Worker report: p8-prep (`feat/v1-distribution--prep`)

- Branch: `feat/v1-distribution--prep`
- Worktree: `/home/vchun/Codes/01-AriadShift-wt-p8-prep`
- Base (`origin/feat/v1-distribution`): `13d7551af593ab7b93e3951780e961d61a2dd307`
- Code HEAD before this report commit: `fa1f518030319e8796ca0e011810f5a772ec177f`

## Summary of work

1. **Merged `origin/feat/v1-distribution--t5` (`c543c467dd9a3e38d07849f1fa3717b3b56fdc58`) with a merge commit (`61c5e719c6944bb2d7f3646e2368efda19780cee`):**
   - Clean fast-path merge (`ort` strategy, zero conflicts) bringing the `1.0.0` version bump across `Cargo.toml`, `Cargo.lock`, `apps/desktop/src-tauri/tauri.conf.json`, `schemas/openapi.v1.json`, `crates/ariad-cli/tests/docling_pack.rs`, `docs/cli.md`, and `docs/server.md`, along with `docs/release-notes.md` (covering the R4 local `file://` transfer deadline limitation and `v1.x` roadmap issues `#61` to `#83`).

2. **Merged `origin/docs/v1-security-policy` (`d939f570a7b5f817b25cedce457a3df7f4563b59`) with a merge commit (`4d9d593d27254c82c5b0f64972215278b017fb0b`):**
   - Clean merge (`ort` strategy, zero conflicts) bringing the revised `docs/SECURITY.md` with its top-level `## Known limitations` and `## Out of scope` sections.

3. **Migrated `tauri-apps/tauri-action` from `0.6.2` to `1.0.0` (`8f54473200f9d1ba94a6e238390f4ce784f09a29`):**
   - Verified `refs/tags/v1.0.0` via `gh api repos/tauri-apps/tauri-action/git/ref/tags/v1.0.0` -> annotated tag object `2d793bf2c8c505b9b76211601b09aab48922225d` -> commit SHA `1deb371b0cd8bd54025b384f1cd735e725c4060f` (matching Dependabot PR #60 commit `77db9a0e8ba4f38e357fcc1e150d30504503e2d6`).
   - Updated `.github/workflows/desktop.yml` to pin `tauri-apps/tauri-action@1deb371b0cd8bd54025b384f1cd735e725c4060f # v1.0.0` and renamed `includeUpdaterJson: true` to `uploadUpdaterJson: true` per `action.yml` and `CHANGELOG.md` (`#1208` / `25f3730`) at `v1.0.0`.
   - Updated `apps/desktop/scripts/verify-release.mjs` (`releaseTargets`) and `apps/desktop/scripts/verify-release.test.mjs` so updater platform entries in `latest.json` match the GitHub REST API asset URLs (`candidate.url`, `${githubBaseUrl}/repos/${owner}/${repo}/releases/assets/${id}`) produced by `tauri-action` `1.0.0` (`#1315` / `53c8863`) as well as `candidate.browser_download_url`.
   - Updated `docs/install.md:209` to list `AriadShift_<version>_aarch64.app.tar.gz` (`.sig`) as the macOS updater artifact, matching `tauri-action` `1.0.0` (`#1194` / `db4399e`, which adds `_${version}` to `.app.tar.gz` and `.app.tar.gz.sig` bundles) and `scripts/desktop-release-sums.test.mjs:23-24`.

4. **Replaced developer-only `just pandoc` hints in user-facing runtime errors (`fa1f518030319e8796ca0e011810f5a772ec177f`):**
   - Updated user-facing Pandoc missing/version/describe error strings and `doctor` hints in `crates/ariad-host/src/commands/doctor.rs`, `crates/ariad-host/src/convert.rs`, `crates/ariad-host/src/engines/mod.rs`, `crates/ariad-host/src/engines/pandoc.rs`, `crates/ariad-host/src/pandoc_bin.rs`, and the `crates/ariad-core/src/protocol.rs` unit test fixture to instruct users to `install Pandoc 3.12 or newer (<4) or set ASHIFT_PANDOC`.
   - Updated the CLI test assertions in `crates/ariad-cli/tests/commands_cli.rs:816` and `crates/ariad-cli/tests/convert_cli.rs:177` to match the updated user-facing hint and error message.
   - Kept `just pandoc` only in developer documentation (`docs/bench.md:133`, `docs/desktop.md:169`, `docs/install.md:299` under "From a repository checkout") and developer golden-test setup `.expect(...)` messages in `crates/ariad-cli/tests/`.

## Commits in `origin/feat/v1-distribution..HEAD`

### First-parent commits on `feat/v1-distribution--prep`

- `61c5e719c6944bb2d7f3646e2368efda19780cee` `chore: merge feat/v1-distribution--t5 into feat/v1-distribution--prep`
- `4d9d593d27254c82c5b0f64972215278b017fb0b` `chore: merge docs/v1-security-policy into feat/v1-distribution--prep`
- `8f54473200f9d1ba94a6e238390f4ce784f09a29` `ci: migrate tauri-action to 1.0.0 and accept API asset URLs`
- `fa1f518030319e8796ca0e011810f5a772ec177f` `fix(host): direct users to install Pandoc 3.12 or newer in error hints`
- plus the commit adding this report (`plans/reports/p8-prep-argon.md`)

### Merged branch commits included in range

- From `origin/feat/v1-distribution--t5`:
  - `04a69c5f322bd801badd1141e0356b16570a0b02` `docs: add the release notes file with the 0.2.0 entry`
  - `7fad35a608be866713d951262ef573886c307366` `docs: draft the 1.0.0 release notes`
  - `f31437f1cf8cf384c45473e745e405e80391a228` `docs(plans): record the 1.0.0 release notes draft report`
  - `05b89ae8163bfa824b1dfed2f44adb01dde934b9` `chore(release): prepare 1.0.0`
  - `e45608f1a86fd4c55640ef4bc8015015854cbbf7` `docs: complete the 1.0.0 release notes`
  - `c543c467dd9a3e38d07849f1fa3717b3b56fdc58` `docs(plans): add the 1.0.0 release notes worker report`
- From `origin/docs/v1-security-policy`:
  - `e954a9ba4c35e3837d037c2e2b0196e86e7e73f2` `docs: describe supported versions and reporting`
  - `6bd99e1e5218eff5c0c6e93c12fb86ad90f86b04` `docs: add security policy task report`
  - `8c88f52b9f3f912fb0242634e03c96c0c5e96f29` `docs: list known limitations in the security policy`
  - `d939f570a7b5f817b25cedce457a3df7f4563b59` `docs(plans): record known-limitations revision in security policy report`

## Review findings closed (`plans/reports/code-review-261025-phase8.md`)

1. **Concern #1 (release workflow guard mutation anchors at `scripts/test-check-release-workflow.sh:802` and `:808`):**
   - Already repaired on `origin/feat/v1-distribution` in commit `291f6928db564e8947faa3ea2b159dba1c801d30` and verified here by running `sh scripts/check-release-workflow.sh && sh scripts/test-check-release-workflow.sh` (80 tests: 78 PASS, 2 SKIP because `pwsh` is not on default `PATH`, 0 FAIL).
2. **Concern #2 (`docs/install.md:209` macOS updater archive naming):**
   - Under `tauri-action` `0.6.2`, `.app.tar.gz` and `.app.tar.gz.sig` omitted the `<version>` segment (`AriadShift_aarch64.app.tar.gz`), which conflicted with `scripts/desktop-release-sums.test.mjs:23-24`. Migrating to `tauri-action` `1.0.0` (`1deb371b0cd8bd54025b384f1cd735e725c4060f`, upstream PR `#1194`) adds `_${version}` to `.app.tar.gz` and `.app.tar.gz.sig` (`AriadShift_<version>_aarch64.app.tar.gz`), and updating `docs/install.md:209` aligns the action, the checksum test fixtures, and the installation guide.
3. **Concern #3 (version-bump and docs-sweep completion):**
   - Merging `origin/feat/v1-distribution--t5` and `origin/docs/v1-security-policy` completes the Phase 8 T5 `1.0.0` version bump, `docs/release-notes.md`, and `docs/SECURITY.md` policy on the distribution branch. (Sibling GHCR/Compose verification belongs to `feat/cloud-product`.)

## Commands run and outcomes

- `gh api repos/tauri-apps/tauri-action/git/ref/tags/v1.0.0` and `gh api repos/tauri-apps/tauri-action/git/tags/2d793bf2c8c505b9b76211601b09aab48922225d`: verified `v1.0.0` resolves to commit `1deb371b0cd8bd54025b384f1cd735e725c4060f`.
- `node --check apps/desktop/scripts/verify-release.mjs && node --check apps/desktop/scripts/verify-release.test.mjs`: passed (`exit 0`).
- `node --test apps/desktop/scripts/verify-release.test.mjs`: passed (2/2 tests pass in 117 ms).
- `node --test scripts/desktop-release-sums.test.mjs`: passed (7/7 tests pass in 267 ms).
- `python3 scripts/release/test-verify-homebrew.py`: passed (8/8 tests pass in 39 ms).
- `sh scripts/check-release-workflow.sh && sh scripts/test-check-release-workflow.sh`: passed (80 total, 78 PASS, 2 SKIP, 0 FAIL).
- `flock ~/Codes/01-AriadShift/.run/build.lock nice -n 19 zsh -lic 'just lint-workflows'`: passed (`actionlint` 0 errors; `zizmor` 0 findings).
- `cargo fmt --check -p ariad-core -p ariad-host -p ariad-cli`: passed (`exit 0`).
- `/home/vchun/Codes/01-AriadShift-wt-p5-int2/node_modules/.bin/biome check apps/desktop/scripts/verify-release.mjs apps/desktop/scripts/verify-release.test.mjs docs/install.md docs/release-notes.md docs/SECURITY.md`: passed (`exit 0`).
- `typos`: passed (`exit 0`) on all modified files.
- `sh scripts/check-commits.sh origin/feat/v1-distribution..HEAD`: passed (`exit 0`).

## What could not be verified without a build

- Rust compilation and unit/integration tests for the updated Pandoc hint strings and `1.0.0` version bump (`cargo nextest run -p ariad-core -p ariad-host -p ariad-cli`, specifically `doctor_missing_pandoc_exit_code_5`, `maps_usage_route_limit_tool_and_input_errors_to_documented_exit_codes`, `protocol::tests`, and `docling_pack`), `just licenses` / `just licenses-check` (`THIRD_PARTY_LICENSES` and desktop licenses regeneration after the `1.0.0` bump), `just package-check`, `just ci`, `just desktop-ci`, and `just desktop-build` were not run per the build-free brief and are left for the gate worker.

## Open questions / concerns

- None. As noted in `plans/reports/a-t5-v1-notes-argon.md`, the gate worker running on the integrated branch should run `just licenses` so `THIRD_PARTY_LICENSES` reflects `1.0.0`, and when `feat/cloud-product` is merged alongside `schemas/openapi.v1.json` (`1.0.0`), `pnpm --filter @ariadshift/sdk generate` will refresh `packages/sdk/generator.lock.json`.

Status: DONE
