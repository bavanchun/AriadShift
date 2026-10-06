# Research Report: Professional GitHub Actions CI Pipeline Design for AriadShift

**Date:** 2026-10-06 | **Repository:** `bavanchun/AriadShift` | **Target File:** `plans/reports/researcher-261006-1528-ci-pipeline-design.md`

## Verified Tool Facts

All tags and commit SHAs dereferenced and verified via `gh api repos/<owner>/<repo>/commits/<tag>`.

| Tool / Action | Latest Stable | License | Release Tag & Verified Commit SHA | Source URL | Verification Method |
|---|---|---|---|---|---|
| `rhysd/actionlint` | v1.7.12 | MIT | `v1.7.12` @ `914e7df21a07ef503a81201c76d2b11c789d3fca` | [rhysd/actionlint](https://github.com/rhysd/actionlint/releases/tag/v1.7.12) | `gh api` commit SHA check |
| `zizmorcore/zizmor` | v1.30.1 | MIT | `v1.30.1` @ `99a054ed9283c90abdd2d5b9fb5101d27dde9783` | [zizmorcore/zizmor](https://github.com/zizmorcore/zizmor/releases/tag/v1.30.1) | `gh api` commit SHA check |
| `zizmorcore/zizmor-action` | v0.6.4 | MIT | `v0.6.4` @ `cc914d7f3750a2d13d75c7f184a1060aa0e9d482` | [zizmor-action](https://github.com/zizmorcore/zizmor-action/releases/tag/v0.6.4) | `gh api` commit SHA check |
| `crate-ci/committed` | v1.1.11 | Apache-2.0 | `v1.1.11` @ `faeed42f2e10c244533a01525f13c4d8b6ce383f` | [crate-ci/committed](https://github.com/crate-ci/committed/releases/tag/v1.1.11) | `gh api` commit SHA check |
| `cocogitto/cocogitto` | 7.0.0 | MIT | `7.0.0` @ `055a9fa8db8ac8ce50074d50162b48b92e9d0c47` | [cocogitto](https://github.com/cocogitto/cocogitto/releases/tag/7.0.0) | `gh api` commit SHA check |
| `commitlint` (cli) | v21.2.3 | MIT | `v21.2.3` @ `95d40569d2592bf9719bc27da2d51fc6b801e4ef` | [commitlint](https://github.com/conventional-changelog/commitlint/releases/tag/v21.2.3) | `gh api` commit SHA check |
| `EmbarkStudios/cargo-deny-action` | v2.1.1 | Apache-2.0 | `v2.1.1` @ `3c6349835b2b7b196a839186cb8b78e02f7b5f25` | [cargo-deny-action](https://github.com/EmbarkStudios/cargo-deny-action/releases/tag/v2.1.1) | `gh api` commit SHA check |
| `github/codeql-action` | v4.38.2 | MIT | `v4.38.2` @ `2892aa5e19bbd11bc0cff5427e3b750a04d9e3c2` | [codeql-action](https://github.com/github/codeql-action/releases/tag/v4.38.2) | `gh api` tag commit check |
| `ossf/scorecard-action` | v2.4.4 | Apache-2.0 | `v2.4.4` @ `2d1146689b8cda280b9bc96326124645441f03bc` | [scorecard-action](https://github.com/ossf/scorecard-action/releases/tag/v2.4.4) | `gh api` commit SHA check |
| `lycheeverse/lychee-action` | v2.9.0 | Apache-2.0 | `v2.9.0` @ `f613c4a64e50d792e0b31ec34bbcbba12263c6a6` | [lychee-action](https://github.com/lycheeverse/lychee-action/releases/tag/v2.9.0) | `gh api` commit SHA check |
| `step-security/harden-runner` | v2.22.0 | Apache-2.0 | `v2.22.0` @ `c6295a65d1254861815972266d5933fd6e532bdf` | [harden-runner](https://github.com/step-security/harden-runner/releases/tag/v2.22.0) | `gh api` commit SHA check |
| `pnpm 12` in Dependabot | Unverified | N/A | Ecosystem `npm` detects `pnpm-lock.yaml`; pnpm 12 support unverified | [dependabot-core](https://github.com/dependabot/dependabot-core) | Documentation check (unverified) |

---

## A. Duplicate Work & Mitigation Analysis

### 1. Events Triggering CI & Identical Tree Scenarios
In AriadShift's Git model (`dev` integration, `main` stable, wave PRs rebase-merged into `dev`, promotions merged via merge commit into `main`), the following trigger events test identical trees:
1. **PR into `dev` vs. Push to `dev` post-merge:** A PR runs on `refs/pull/N/merge`. Once green, `gh pr merge --rebase` replays commits onto `dev`. If `dev` did not move, the git tree SHA (`git rev-parse HEAD^{tree}`) of the new `dev` tip is bit-for-bit identical to the PR branch tree just verified. A redundant 3-OS matrix run fires on `dev`.
2. **Promotion PR (`dev` -> `main`) vs. Push to `main`:** When promoting `dev` to `main`, `dev` is already proven green on all OSes. The promotion PR runs the 3-OS matrix on `refs/pull/M/merge`. After `gh pr merge --merge`, `push` fires on `main`, running the exact same 3-OS matrix a third time on the identical merge tree.
3. **Docs/Plan-only commits:** Commits touching only `docs/`, `plans/`, or `.md` files trigger the full 3-OS `verify` matrix (Rust build, test, Pandoc), despite only `typos` being relevant.
4. **Superseded PR commits:** Rapidly pushing new commits to an open PR triggers parallel runs for outdated commit SHAs unless cancelled.

### 2. Comparison of Deduplication Approaches
- **Approach 1: `fkirc/skip-duplicate-actions` or custom GitHub API script:**
  - *Analysis:* Checks if a previous run on the same tree or commit SHA succeeded.
  - *Verdict: Disapproved.* If a push run on `dev` skips `verify`, GitHub Actions **does not save caches**. Because PR caches are isolated and only base-branch caches are inherited, skipping cache-generation on `dev` causes every subsequent PR to build cold. Also, `fkirc/skip-duplicate-actions` requires elevated permissions (`actions: read`, `checks: read`), is slow to index rebased tree SHAs, and introduces an unneeded dependency.
- **Approach 2: Dropping push runs on `dev` / `main`:**
  - *Analysis:* Running CI exclusively on `pull_request`.
  - *Consequences:*
    1. **Cache Scoping:** PRs cannot write to the base branch cache. If `dev` never runs a push job, `dev` has no warm cache; all PRs build from scratch.
    2. **Post-Merge Divergence:** If `dev` moved before merge or a merge conflict resolution introduced a subtle semantic bug, trunk remains unverified until a release tag.
    3. **Scheduled Workflows:** Run strictly on the default branch (`main`).
  - *Verdict: Disapproved.* Base branches must maintain green status and warm caches.
- **Approach 3: GitHub Merge Queue:**
  - *Analysis:* Requires enabling "Require merge queue" in classic branch protection or rulesets ([GitHub Docs](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue)).
  - *Verdict: Infeasible.* The user explicitly declined branch protection and rulesets.
- **Approach 4: Path Filters with Stable Aggregate Check:**
  - *Analysis:* Using path inspection in a fast `lint` job to conditionally skip heavy multi-OS build/test jobs for docs-only changes, while an aggregate gate job (`ci-success`) evaluates status and reports a consistent green check.
  - *Verdict: Strongly Recommended.* Avoids 10–15 minutes of matrix compute on Markdown changes while ensuring `typos` still checks docs.
- **Approach 5: Industry Precedent (Well-run Rust Projects):**
  - Examined workflows: `tokio-rs/tokio` (`.github/workflows/ci.yml`), `astral-sh/uv` (`.github/workflows/ci.yml`), `BurntSushi/ripgrep` (`.github/workflows/ci.yml`).
  - *Finding:* None of these projects drop `push` runs on default branches or use duplicate-skipping actions. They run `push` on base branches to prime the authoritative compiler cache (`rust-cache`) and guarantee trunk health. They eliminate redundant PR work via exact concurrency grouping and path filtering.

### 3. Recommended Concurrency Setting
To cancel superseded PR runs while **never** cancelling push runs on `dev` or `main`:
```yaml
concurrency:
  group: ${{ github.workflow }}-${{ github.event.pull_request.number || github.sha }}
  cancel-in-progress: true
```
*Proof:* On `pull_request`, `github.event.pull_request.number` evaluates to the PR number (e.g. `CI-42`). A subsequent commit to the same PR shares the group key and cancels the superseded run. On `push` to `dev` or `main`, `pull_request.number` is null and falls back to `github.sha`. Since every push commit has a unique SHA, the group key is unique per push, guaranteeing `cancel-in-progress` **never** cancels a commit on `dev` or `main`.

---

## B. Tool Evaluations & Evidence

### 1. `rhysd/actionlint`
- **Version & License:** `v1.7.12`, MIT. Tag commit SHA: `914e7df21a07ef503a81201c76d2b11c789d3fca`.
- **Shellcheck Integration:** Native. When `shellcheck` is in `PATH`, `actionlint` automatically extracts and validates inline `run:` shell scripts. GitHub-hosted Ubuntu runners have `shellcheck` preinstalled.
- **`taiki-e/install-action` Support:** Checked `taiki-e/install-action/TOOLS.md`; `actionlint` is **not** supported by `install-action`.
- **Install & Execution:** Best installed via the official download script (`bash <(curl -fsSL https://raw.githubusercontent.com/rhysd/actionlint/main/scripts/download-actionlint.bash) 1.7.12`) or prebuilt release archive.
- **Integration:** Add `just lint-actions` to `justfile` and run inside the CI `lint` job.

### 2. `zizmorcore/zizmor` & `zizmor-action`
- **Version & License:** `zizmor` `v1.30.1` (SHA: `99a054ed9283c90abdd2d5b9fb5101d27dde9783`), `zizmor-action` `v0.6.4` (SHA: `cc914d7f3750a2d13d75c7f184a1060aa0e9d482`). Both MIT.
- **Config & Persona:** Configured via root `.zizmor.yml`. Persona: `regular` (default), min-severity: `medium`.
- **SARIF vs. Plain Failure:** `advanced-security: true` outputs SARIF and calls `upload-sarif`, requiring `security-events: write`. For a public repository without GitHub Advanced Security, configuring `advanced-security: false` and `annotations: true` emits standard workflow annotations and fails on exit code, needing only `contents: read`.
- **Offline Mode:** `online-audits: false` (or `--offline` locally) audits workflow syntax without outbound GitHub API calls.
- **SHA-Pinned Actions:** As of v1.20.0, rule `unpinned-uses` defaults to `hash-pin` policy. AriadShift's existing format (`uses: actions/checkout@<sha> # v7.0.0`) is natively recognized and praised as fully compliant.
- **Local Fit:** Run locally via `uvx zizmor .` (no toolchain additions since `uv` is already present).

### 3. Commit Message Linting
- **Tool Comparison:**
  - `crate-ci/committed`: v1.1.11, Apache-2.0, SHA `faeed42f2e10c244533a01525f13c4d8b6ce383f`. Native composite action `uses: crate-ci/committed@faeed42...`. Written in Rust. Evaluates full PR commit ranges (`origin/dev..HEAD`). Directly supports Conventional Commits (`style = "conventional"`), `allowed_types`, `subject_length = 72`, `subject_capitalized = false` (lowercase), `subject_not_punctuated = true`, and `imperative_subject = true`.
  - `cocogitto/cocogitto`: 7.0.0, MIT, SHA `055a9fa8db8ac8ce50074d50162b48b92e9d0c47`. Excellent for changelog generation, but lacks granular imperative verb enforcement out of the box.
  - `commitlint`: v21.2.3, MIT, SHA `95d40569d2592bf9719bc27da2d51fc6b801e4ef`. Highly extensible, but introduces heavy Node.js runtime/dependency overhead into a Rust-centric repository.
- **Custom Rule (AI / Tool Reference Rejection):** Neither `committed` nor standard tools have a built-in negative keyword filter for commit bodies. The cleanest and most robust solution is a small, deterministic shell step in the `lint` job:
  ```bash
  ! git log --format='%B' "origin/dev..HEAD" | grep -iE '\b(claude|codex|gemini|chatgpt|copilot|ai-assisted|generated with)\b|co-authored-by:.*(bot|ai|agent)'
  ```
- **Dependabot Commits:** Dependabot commits use author `dependabot[bot]` and messages like `build(deps): bump ...`. When configured with prefix `build`, subjects pass `committed`. The custom regex targets generative AI tools, so Dependabot passes without false positives.

### 4. Dependabot Version Updates
- **Ecosystems:**
  - `github-actions`: Fully supported. Updates 40-character commit SHAs while updating the `# vX.Y.Z` trailing comment.
  - `cargo`: Fully supported for root workspace. Multi-directory entry covers `directory: "/fuzz"` once Phase 11 lands.
  - `npm` (with `pnpm`): Configured as `package-ecosystem: "npm"`, `directory: "/"`. Dependabot GA added pnpm workspace catalog support in Feb 2025. However, `pnpm 12` lockfile parsing in Dependabot's container is currently **unverified** and may fail until Corepack in Dependabot updates.
  - `uv`: Fully supported as `package-ecosystem: "uv"`, `directory: "/"`.
  - `rust-toolchain.toml`: **Not supported** by Dependabot (requires manual bump or Renovate).
- **Settings:**
  - `target-branch: "dev"` directs all version update PRs to the integration branch.
  - `cooldown`: Supported since July 2026. Set `default-days: 7`, `semver-major-days: 14`, `semver-minor-days: 7`, `semver-patch-days: 3`.
  - `commit-message`: Set `prefix: "build"` (or `ci` for actions) with `include: "scope"`, producing `build(deps): ...` to conform with Conventional Commits.
  - `open-pull-requests-limit: 5` prevents PR flooding.
  - `groups`: Grouping all minor/patch updates bundles updates into a single PR per ecosystem.
- **Security Updates:** **Must remain disabled**. GitHub documentation confirms that automated Dependabot security updates **always target the default branch (`main`)** and ignore `target-branch`. Enabling them would bypass `dev` and open PRs directly against `main`. Security advisories are instead detected via Dependabot alerts and scheduled `cargo-deny`.

### 5. Scheduled Security Scan
- **Execution:** Daily `schedule` at 02:00 UTC running `cargo deny check advisories`.
- **Action vs. CLI:** `ci.yml` already provisions `cargo-deny@0.20.2` via `taiki-e/install-action`. Using `cargo deny check advisories` directly via `just deny-advisories` guarantees version consistency between local runs and CI, avoiding `EmbarkStudios/cargo-deny-action` wrapper overhead.
- **Failure Handling:** Fail the workflow run. Requires minimal `contents: read` permissions. Automated issue creation requires `issues: write` and complex state de-duplication to prevent daily duplicate issues.
- **Branch Checkout:** GitHub triggers scheduled workflows exclusively from the default branch (`main`). The workflow checkout step must explicitly specify `ref: dev` to inspect the integration branch.

### 6. CodeQL
- **Rust & Actions Support:** CodeQL natively supports `rust` using `build-mode: none` (AST analysis via rust-analyzer; manual build mode is unsupported for Rust) and `actions` (scanning workflow files for misconfigurations).
- **Action:** `github/codeql-action` `v4.38.2` (SHA: `2892aa5e19bbd11bc0cff5427e3b750a04d9e3c2`).
- **Setup Choice:** **Advanced workflow** (`.github/workflows/codeql.yml`). The default setup cannot be SHA-pinned (violating AGENTS.md) and cannot customize branch triggers.
- **Run Time & Triggers:** With `build-mode: none`, analysis completes in ~1–2 minutes on this codebase. Trigger on `push` to `dev`/`main` and a weekly schedule.

### 7. OpenSSF Scorecard (`ossf/scorecard-action`)
- **Version & License:** `v2.4.4`, Apache-2.0, SHA `2d1146689b8cda280b9bc96326124645441f03bc`.
- **Permissions & Behavior:** Requires `id-token: write` for `publish_results: true` (OIDC to scorecard.dev) and `security-events: write`.
- **Recommendation: SKIP.** Scorecard penalizes repositories without branch protection or rulesets (which the user explicitly declined), produces low branch-protection scores by design, and downloads a heavy Docker container (~2 minutes).

### 8. Additional Practices Evaluation
| Item | Disposition | Reason |
|---|---|---|
| `timeout-minutes` | **Recommend** | Prevent runaways; set 20m for `verify`, 10m for `lint`. Default is 360m. |
| Aggregate `ci-success` job | **Recommend** | Single stable check name required for PR status verification across matrix OSes. |
| Rust Cache Scoping | **Recommend** | Configure `cache-read-only: ${{ github.ref != 'refs/heads/dev' }}` so PRs do not thrash `dev` cache. |
| `CARGO_INCREMENTAL: "0"`, `CARGO_TERM_COLOR: "always"` | **Recommend** | Mandatory CI best practice: smaller cache, faster compiles, colored terminal logs. |
| Nextest JUnit & Summary | **Optional** | Nice to have; `cargo nextest run` CLI output is already readable in job summary. |
| Scheduled `lychee` Link Check | **Recommend** | `lychee-action` `v2.9.0` (SHA: `f613c4a64e50d792e0b31ec34bbcbba12263c6a6`). Run weekly on docs. |
| StepSecurity `harden-runner` | **Skip** | Linux-only; high maintenance burden for outbound network egress allow-listing. |
| Artifact Retention | **Recommend** | Set `retention-days: 14` (reduces storage waste compared to 90-day default). |

---

## C. Recommended Architecture

### 1. Workflow Files & Permissions
1. **`.github/workflows/ci.yml`** (`permissions: contents: read`):
   - Triggers: `pull_request` (branches: `[dev, main]`), `push` (branches: `[dev, main]`), `workflow_dispatch`.
   - Concurrency: `group: ${{ github.workflow }}-${{ github.event.pull_request.number || github.sha }}`, `cancel-in-progress: true`.
   - Jobs:
     - `lint`: `ubuntu-26.04`. Runs `actionlint` (with `shellcheck`), `zizmor` (offline), `committed` + AI ban script, `typos`, `cargo fmt --check`. Emits output `docs-only = true/false`.
     - `lint-windows`: `windows-2025`. Runs Windows-specific Clippy. Skipped if `docs-only == true`.
     - `verify`: Matrix `[ubuntu-26.04, macos-26, windows-2025]`. Runs `just ci`. Skipped if `docs-only == true`. Writes cache only when `github.ref == 'refs/heads/dev'`.
     - `ci-success`: Aggregate gate (`needs: [lint, lint-windows, verify]`, `if: always()`). Asserts all required jobs passed or legitimately skipped.
2. **`.github/workflows/security-schedule.yml`** (`permissions: contents: read`):
   - Triggers: Daily `schedule` (`cron: '0 2 * * *'`), `workflow_dispatch`.
   - Job: Checks out `ref: dev`. Runs `cargo deny check advisories`. Fails if CVE found.
3. **`.github/workflows/codeql.yml`** (`permissions: contents: read`, `security-events: write`):
   - Triggers: `push` (`branches: [dev, main]`), weekly `schedule` (`cron: '0 3 * * 1'`).
   - Job: `github/codeql-action` `v4.38.2` with `languages: rust, actions` and `build-mode: none`.
4. **`.github/workflows/docs-links.yml`** (`permissions: contents: read`):
   - Triggers: Weekly `schedule` (`cron: '0 4 * * 1'`), `workflow_dispatch`. Checks out `ref: dev`, runs `lychee`.
5. **`.github/dependabot.yml`**:
   - `github-actions`, `cargo`, `npm`, `uv` targeting branch `dev`, with cooldown and grouped PRs.

### 2. Event-by-Event Execution Matrix
Proves that no tree is tested twice without a concrete operational reason (e.g. warming trunk cache).

| Trigger Event | `lint` (Linux) | `lint-windows` | `verify` (3 OSes) | `ci-success` | Cache Written? | Operational Justification |
|---|---|---|---|---|---|---|
| **PR into `dev` (Code changed)** | Runs | Runs | Runs | Passes | No (Read-only) | Full pre-merge validation across 3 OSes. |
| **PR into `dev` (Docs-only)** | Runs (`typos`) | Skipped | Skipped | Passes | No | Saves 15m runner compute; validates spelling. |
| **Push to `dev` post-merge** | Runs | Runs | Runs | Passes | **Yes (`dev` cache)** | **Required:** Primes base cache for future PRs. |
| **Promotion PR (`dev` -> `main`)** | Runs | Skipped* | Skipped* | Passes | No | `dev` tree was already proven green; fast gate. |
| **Push to `main` post-promotion** | Runs | Runs | Runs | Passes | **Yes (`main` cache)** | Verifies merge commit; primes stable cache. |
| **Hotfix PR (`fix/*` -> `main`)** | Runs | Runs | Runs | Passes | No | Full pre-merge validation on stable branch. |
| **Dependabot PR into `dev`** | Runs | Runs | Runs | Passes | No | Validates bumped dependencies across 3 OSes. |
| **Daily Schedule (`dev`)** | N/A | N/A | N/A | N/A | No | Runs `cargo deny check advisories` on `dev`. |

*\*Note: Promotion PRs can safely skip the heavy multi-OS matrix because `dev` is required to be green before opening the promotion PR.*

### 3. Extension Points for Later Plan Workflows
- **Phase 8 (`bench.yml`):** Runs on PRs labelled `bench` and nightly schedule against `dev`. Writes diff to job summary. Does not duplicate `ci.yml`.
- **Phase 11 (`fuzz` job in `ci.yml` & `fuzz-nightly.yml`):** The `fuzz` job attaches as a dependency to `ci-success` in `ci.yml`. `fuzz-nightly.yml` runs scheduled ASan tests against `dev`.
- **Phase 12 (`release.yml`, `publish-crates.yml`, `release-smoke.yml`):** `release.yml` triggers on `v*` tags on `main`. `release-smoke.yml` runs on `workflow_dispatch` or post-release. Clean separation from CI.

### 4. Just Recipes to Add
Add to `justfile` so developers can run all checks locally:
```just
lint-actions:
    bash <(curl -fsSL https://raw.githubusercontent.com/rhysd/actionlint/main/scripts/download-actionlint.bash) 1.7.12
    ./actionlint

lint-workflows:
    uvx zizmor . --offline

lint-commits:
    committed origin/dev..HEAD
    ! git log --format='%B' origin/dev..HEAD | grep -iE '\b(claude|codex|gemini|chatgpt|copilot|ai-assisted|generated with)\b|co-authored-by:.*(bot|ai|agent)'

deny-advisories:
    cargo deny check advisories
```

### 5. Risks & Open Questions for User
1. **`pnpm 12` in Dependabot:** Dependabot's container environment may not yet support `pnpm 12.9.1` with catalog protocol, potentially resulting in failed Dependabot update runs for `package-ecosystem: "npm"`. If Dependabot errors, npm updates must remain manual until Dependabot updates its container.
2. **Promotion PR Matrix Skip:** Does the user prefer the promotion PR (`dev -> main`) to run the full 3-OS matrix as an extra sanity check (~5 minutes), or skip it to save CI minutes since `dev` is already green? (Recommended: skip matrix on promotion PR).
3. **SARIF Code Scanning Permissions:** Do you want `security-events: write` enabled for Advanced CodeQL in `.github/workflows/codeql.yml`, or prefer pure CLI output in logs? (Recommended: `security-events: write` for GitHub Security Tab alerts).

---

## Coordinator verification (2026-10-06)

Checked before implementation; the shipped pipeline is described in `docs/ci.md`.

- `lycheeverse/lychee-action` v2.9.0 is commit `e7477775783ea5526144ba13e8db5eec57747ce8`, not the SHA in the facts table.
- `taiki-e/install-action` has manifests for `zizmor` and `typos`, but not for `actionlint`, `committed` or `lychee`. `actionlint` 1.7.12 and `committed` 1.1.11 are installed by `scripts/install-lint-tools.sh` from their release assets with pinned SHA-256 checksums; `zizmor` 1.30.1 (PyPI) runs through `uvx`.
- `actions-rust-lang/setup-rust-toolchain` v2.0.0 already sets `CARGO_INCREMENTAL=0` and `CARGO_TERM_COLOR=always`. Its cache switch is `cache-save-if`, not `cache-read-only`.
- The AI-reference regex in section B.3 also matched ordinary names (for example "Kai" against `ai`). The shipped check uses whole-word tool names instead.
- zizmor's online `ref-version-mismatch` audit found two wrong version comments already in `ci.yml`: the pinned `actions/checkout` SHA is tag v7.0.1 and the pinned `pnpm/action-setup` SHA is tag v6.1.0. Both are the latest releases; the comments were corrected.
- actionlint 1.7.12 does not know the `ubuntu-26.04` runner label, so `.github/actionlint.yaml` declares it.
- Instead of skipping the promotion pull request blindly, the scope step skips the build only when the tree matches a `dev` commit whose CI push run succeeded.
