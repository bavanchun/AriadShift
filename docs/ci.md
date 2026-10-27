# Continuous integration

How GitHub Actions checks this repository, and how to run the same checks locally. The branch model it serves is in [git-workflow.md](git-workflow.md).

## Workflows

| Workflow | Runs on | What it does |
|---|---|---|
| [`ci.yml`](../.github/workflows/ci.yml) | Pull requests into `dev` or `main`, pushes to `dev` or `main`, manual | The gate for every change (below) |
| [`bench.yml`](../.github/workflows/bench.yml) | Pull requests with `bench` label, nightly schedule, manual | Runs base and heavy-route benchmarks, real Docling conformance, capabilities validation and generated public report |
| [`codeql.yml`](../.github/workflows/codeql.yml) | Pull requests (code changes), pushes to `dev` or `main`, weekly, manual | CodeQL for Rust and for the workflows themselves; alerts go to the repository's Security tab |
| [`security.yml`](../.github/workflows/security.yml) | Daily, manual, and pull requests that change it | `cargo deny check advisories` on `dev` and on `main`, so a newly published RustSec advisory fails a run even when no code changed |
| [`fuzz-nightly.yml`](../.github/workflows/fuzz-nightly.yml) | Daily, manual, and pull requests that change it | Date-pinned nightly AddressSanitizer fuzzing on unsafe-adjacent parsers (10 min per target) |
| [`pack-docling.yml`](../.github/workflows/pack-docling.yml) | Pull requests that touch the Docling engine, its mapper, `uv.lock`, the `justfile` or the workflow; tags `pack-docling-v*` | Engine ruff and pytest on Linux and Windows, the real-engine tests with cached models and Tesseract, and the Linux x86_64 pack converted offline. Tags and pull requests that change the pack builder build every target and merge the manifests. Details in [docling.md](docling.md#ci) |
| [`torch-docling.yml`](../.github/workflows/torch-docling.yml) | Pull requests changing the torch source build inputs, manual | Builds or reuses the MKL-free CPU wheels on native Linux and Windows x86_64, audits binaries and probes torch/torchvision. Maintainers publish reviewed outputs as checksummed toolchain inputs; see [docling.md](docling.md#the-pack). |
| [`links.yml`](../.github/workflows/links.yml) | Weekly, manual, and pull requests that change it | lychee checks every Markdown link on `dev`, configured by [`lychee.toml`](../lychee.toml) |
| [`dependabot.yml`](../.github/dependabot.yml) | Weekly (Monday, Asia/Saigon) | Update pull requests into `dev` for GitHub Actions, Cargo, pnpm and uv |
| [`release.yml`](../.github/workflows/release.yml) | Tag push `'**[0-9]+.[0-9]+.[0-9]+*'`, pull requests touching release paths (build-only rehearsal) | Builds 5 targets on 3 OSes, one CycloneDX SBOM per crate, Homebrew formula, and invokes `publish-crates.yml` (pull requests scoped via `paths:` filter) |
| [`desktop.yml`](../.github/workflows/desktop.yml) | Desktop/shared/Rust changes in pull requests, `v*` tags, manual | Builds installers on three OSes, checks generated bindings and runs the real Linux WebDriver conversion; tag jobs attach signed updates to the CLI-owned release (see [desktop.md](desktop.md)) |
| [`cloud-e2e.yml`](../.github/workflows/cloud-e2e.yml) | Daily at 04:00 UTC on `dev`, manual, path-matched pushes and pull requests, or the `cloud-e2e` PR label | Builds all three Compose profiles, proves image-only Vietnamese PDF OCR through the completed Docling route, then checks DOCX content, deletion and quotas |
| [`release-smoke.yml`](../.github/workflows/release-smoke.yml) | Manual dispatch (`workflow_dispatch`) | Multi-channel smoke test verifying binary installation across Homebrew, shell, PowerShell, and cargo-binstall |
| [`publish-crates.yml`](../.github/workflows/publish-crates.yml) | Called by `release.yml` | Publishes crates (`ariad-core`, `ariad-host`, `ariad-cli`) to crates.io with API version polling and idempotence |

GitHub runs `schedule` triggers, and offers the manual "Run workflow" button, only for workflows on the default branch, `main`. A new scheduled workflow therefore starts after the next promotion; until then, its pull request run is its test.

## The CI gate

`ci.yml` has eight jobs:

1. **Scope** (Linux, seconds) runs [`scripts/ci-scope.sh`](../scripts/ci-scope.sh). It picks the commit range to lint and decides whether the build, test and fuzz jobs must run, and separately whether the web job must run. The script also provides a `--classify` mode for table-driven offline testing.
2. **Static checks** (Linux) run once for every event:
   - each new commit against the commit rules (`just lint-commits`);
   - `just static`: formatting, `typos`, `actionlint`, `zizmor` with online audits, `cargo deny`, pnpm and uv lockfiles, the license check of production JavaScript dependencies (`js-licenses`), Biome (`web-lint`), release workflow security guard (`check-release-workflow`), license inventory check (`licenses-check`), WinGet manifest generator test (`test-winget-manifest`), and scope classifier test (`test-ci-scope`). The release workflow guard is a best-effort structural check that catches accidental regressions of the release supply chain; it is not a security boundary, which `actionlint` and `zizmor` back up.
3. **Lint (windows-2025)** runs Clippy on Windows, for early feedback on Windows-only code.
4. **Verify** runs on `ubuntu-26.04`, `macos-26` and `windows-2025`. It installs Pandoc (plus distribution-pinned LibreOffice Writer/Calc/Impress and qpdf on Linux), sets up uv, runs Clippy (Windows already has it from job 3), the WASM check (Linux only, since it does not depend on the host), `just test` (including shared engine conformance, with LibreOffice required on Linux), `just bench-test`, `just bench-check`, and `just package-check`.
5. **Server integration** (Linux, `ubuntu-26.04`) runs the `ariad-server` tests against PostgreSQL 18 with pgmq (a service container) and SeaweedFS (started in a step, because service containers cannot take a command), with Pandoc installed and the `ashift` engine binary built. The database and S3 tests skip in the Verify job, where neither exists; `just test-server` runs the same suite locally in Docker. The job also checks all three Dockerfiles, builds the server image and converts Markdown to DOCX using real Pandoc in required mode in a read-only container with tmpfs, then probes procfs access to the worker and its supervisor. The signed Docling image, the Office namespace profile and the direct-launch Kubernetes fragment are not exercised by this smoke.
6. **Fuzz** (Linux, `ubuntu-26.04`) runs cargo-fuzz 0.13.2 on stable Rust with `-s none` for 60 seconds per target (`markdown_reader`, `front_matter`, `html_reader`, `pandoc_ast_to_ir`, `docling_to_ir`, `ir_json`, `limits_validate`, `pdf_reader`, `typst_writer`), with cached corpus and crash artifact upload on failure or cancellation.
7. **Web** (Linux, `ubuntu-26.04`) builds the wasm module, typechecks every JavaScript package, runs Vitest, builds the site (`just web-ci`) and runs Playwright on Chromium, Firefox and WebKit (`just web-e2e`), uploading traces on failure. See [web.md](web.md).
8. **CI passed** is the single result to look at. It fails unless Scope and Static checks passed, jobs 3 to 6 either passed or were skipped by the scope decision, and job 7 passed when the scope asked for it and was skipped otherwise.

### When the build, test and fuzz jobs are skipped

The same tree is never built twice without a reason:

| Event | Build, test and fuzz | Why |
|---|---|---|
| Pull request into `dev` | Run | The change is new |
| Push to `dev` after a merge | Run | Only runs on a base branch may save the Rust cache that later pull requests read, and `dev` may have moved since the pull request ran |
| Promotion pull request into `main`, and the push to `main` after it | Skipped when the tree matches a `dev` commit whose CI push run succeeded | That exact tree was already built and tested on all three OSes |
| Hotfix pull request into `main`, and the push after it | Run | The tree is new |
| Change to `docs/`, `plans/`, or the root `README.md`, `ARCHITECTURE.md`, `AGENTS.md`, `CLAUDE.md` only | Skipped | Static checks still spell-check and lint it |
| Change confined to the JavaScript workspace (`apps/` except `apps/desktop/src-tauri/`, `packages/`, `brand/`, `.node-version`, the root pnpm, Biome and Vitest files, or `scripts/check-js-licenses.mjs`) and prose | Skipped | No Rust code changed; the web and static jobs cover it |
| Change confined to `infra/test/e2e/` or `.github/workflows/cloud-e2e.yml` and prose | Skipped | The cloud-e2e workflow runs the test; static checks lint the workflow |
| Change under `apps/desktop/src-tauri/` | Run | Native desktop Rust needs the same three-OS checks as the other crates |
| Manual run, or a push whose previous commit is unknown | Run | Nothing to compare against |

The web job runs when a change touches `apps/`, `packages/`, `brand/`, `.node-version`, `crates/ariad-core/`, `crates/ariad-wasm/`, the root pnpm, Biome, Vitest, Cargo or toolchain files, the `justfile`, `ci.yml`, `ci-scope.sh` or `install-wasm-bindgen.sh`, and on manual runs and pushes without a known previous commit. Promotions follow the same tree rule as the other jobs.

The native Linux and web jobs download LibreOffice and browser system libraries from Ubuntu's primary archive over HTTPS, replacing Azure entries in direct sources and indirect APT mirror lists. Browser binaries come from the pinned Playwright release. Ubuntu's package signatures and the browser cache policy remain in effect. The native Linux runner also enables unprivileged user namespaces for its disposable VM and verifies user/mount namespace creation before running tests. This satisfies LibreOffice's private `/tmp` requirement; the engine still runs with Landlock, seccomp and host limits, and namespace setup failures remain fatal. The workflow setting does not change the application or configure an end user's machine.

A newer push to the same pull request cancels its older run. Runs on `dev` and `main` are grouped by commit and are never cancelled.

CodeQL is the one deliberate repeat: GitHub computes pull request alerts against the base branch's analysis, so `dev` and `main` are analysed on push too.

CI disables development/test debug information and incremental compilation to bound Typst and package-verification artifact sizes. Every native Verify job also runs `cargo check -p ariad-cli --no-default-features` to keep the slim CLI build working.

The Rust cache is written only by pushes to `dev` and `main` (the web job shares this rule, and saves the Playwright browser cache on pushes only); pull requests read it but never save their own, so they cannot evict the base branch's cache. The fuzz caches (cargo-fuzz binary, `fuzz/target`, and corpus) follow the same policy: saved on pushes to `dev` and `main`, while pull requests restore caches without saving. The nightly workflow (`fuzz-nightly.yml`) saves caches on schedule, dispatch and its own pull request runs (PR-scoped entries are not visible to `dev` or `main`); crash artifacts upload on failure or cancellation.

## Cloud E2E pack dependency

Pushes and pull requests use the single cloud path list in
[`ci-scope.sh`](../scripts/ci-scope.sh) (`--classify-cloud`). It covers the web,
shared Rust crates, workspace/toolchain inputs, image and service sources, the
image-only fixture and its manifest. [`test-ci-scope.sh`](../scripts/test-ci-scope.sh)
checks both included and excluded paths. Unrelated label events skip expensive
jobs and cannot cancel an active Compose job; cancellation is scoped to that job.

For PR, push and manual runs, the isolated **Resolve same-head CI pack** job
queries `pack-docling.yml` runs by the PR head SHA (push/manual: workflow SHA).
It reuses an active or successful run with unexpired artifacts. If none is
usable, it dispatches **Docling pack** on the matching branch after checking
that the ref still points to that SHA. Only this job has `actions: write`; it
executes no checked-out code. The Compose job has `actions: read` and waits for
**Pack (x86_64-unknown-linux-gnu)** to succeed and publish both
`docling-pack-x86_64-unknown-linux-gnu` and `docling-pack-shared`.
A failed pack job or missing components fails E2E rather than skipping it.

The Compose job rechecks the run's head SHA and uses
[`gh run download`](https://cli.github.com/manual/gh_run_download) for both
artifacts. It generates the unsigned manifest with the built server image's
`ashift __pack manifest`, safely extracts components through the pack builder,
and sets `E2E_DOCLING_PACK_DIR`. The worker uses the existing read-only bind
mount override. This development route skips signature installation and its
`requires_ashift` host-range check; it proves runtime conversion, not release
installation. No component is borrowed from another head or a release.

Fork tokens cannot dispatch repository workflows. A fork run can consume an
already approved same-head pack run; otherwise it fails with a maintainer
prerequisite. Maintainers must approve the fork's **Docling pack** run, or bring
the reviewed head into a repository branch and run both workflows there.
The pack wait is bounded to 350 minutes; Compose then has 180 minutes.
Expired one-day PR artifacts are rebuilt through the same dispatch mechanism.

The nightly deliberately retains the production Dockerfile's signed-release
installer. It requires a published signed pack compatible with the host version;
a passing PR override does not establish that release prerequisite.

For local proof, set `E2E_DOCLING_PACK_DIR` to a real unpacked CI pack and run
`bash infra/test/e2e/run.sh`. The script checks the fixture hash, the final
`GET /v1/jobs/{id}` route (`docling`), absence of `ocr_required`, and the OCR
words “Khu vườn đọc sách” in the downloaded DOCX. Removing `tessdata` from a
separate test copy must make the same script fail before starting the stack.

## Commit rules

[`scripts/check-commits.sh`](../scripts/check-commits.sh) checks each commit in the range:

- [committed](https://github.com/crate-ci/committed), configured by [`committed.toml`](../committed.toml): Conventional Commits with this repository's types, a subject of at most 72 characters in the imperative mood, without a trailing period, and body lines of at most 72 columns;
- no reference to an AI tool anywhere in the message (`git-workflow.md` forbids them).

Dependabot's own subjects are exempt from the committed rules, not from the AI check.

## Running the checks locally

| Command | Runs |
|---|---|
| `just ci` | Shared local quality gate: `just static`, Clippy, the WASM check, tests, `bench-test`, `bench-check`, `package-check`, `web-ci` (wasm build, typecheck, Vitest, site build), and desktop-scoped `desktop-ci`; browser E2E and installer packaging run separately |
| `just desktop-ci` | On desktop/shared dependency changes: regenerate bindings and command permissions, reject drift, build the frontend and run UI, build-tool and native tests; see [desktop.md](desktop.md) for installer and Linux smoke commands |
| `just web-e2e` | The web job's Playwright tests; needs `just web-build` and `just web-browsers` first |
| `just bench` | Run benchmark harness on fixture suite |
| `just bench-check` | Validate capabilities.json against schema and invariants |
| `just bench-test` | Run unit tests for benchmark harness |
| `just package-check` | Verify core, host, CLI and internal WASM packaging; server and desktop are excluded, and desktop installers have their own workflow |
| `just test-server-container` | Linux container build/check and required-mode worker smoke; needs a dedicated `ARIAD_TEST_DATABASE_URL`. Set `ARIAD_TEST_CONTAINER_SMOKE=1` with `just test-server` to reuse its throwaway service and clean everything up |
| `just test-server` | The `ariad-server` database and S3 tests in throwaway Docker containers (`ariadshift-test-pg` on 127.0.0.1:55442, `ariadshift-test-s3` on 127.0.0.1:58342), removed afterwards; an ownership label prevents removing another worktree's containers |
| `just static` | The OS-independent checks (requires `cargo-about 0.9.2`, `uv`, `python3`, and `pwsh` in CI) |
| `just lint-commits` | Your commits since `origin/dev`; pass a range to check others, for example `just lint-commits main..dev` |
| `just lint-workflows` | `actionlint` and `zizmor`. Online audits require a clean `GH_TOKEN`/`GITHUB_TOKEN` or a logged-in `gh` CLI |
| `just deny-advisories` | The daily advisory scan |
| `just fuzz` | Run fuzz targets locally on stable Rust (`target=""` for all, `seconds="60"`) |

The throwaway PostgreSQL in `just test-server` allows 180 seconds for `pg_ctl` operations and TCP readiness. This accommodates initialization on busy hosts; if the service never becomes ready, the recipe prints its container log and fails before compiling the test binaries.

[`scripts/with-github-token.py`](../scripts/with-github-token.py) is the credential source for zizmor. It prefers a clean `GH_TOKEN`, then `GITHUB_TOKEN`, then captures `gh auth token` with stderr separate. Malformed environment values are ignored with a diagnostic; CLI output containing whitespace, control characters or wrapper status lines is rejected without printing the value. Only the normal trailing CLI line ending is removed. Missing credentials fail the audit with instructions to authenticate. `just test-github-token` checks these paths and runs as part of `lint-workflows`. The launcher passes the validated value directly to zizmor's environment; no shell capture or manual token export is needed.

`just lint-tools` installs `actionlint` and `committed` into `.tools/bin`, verified against SHA-256 checksums pinned in [`scripts/install-lint-tools.sh`](../scripts/install-lint-tools.sh). `zizmor` runs through `uvx` at the version pinned in the `justfile`.

`just package-check` builds each generated crate archive with sibling dependencies from this checkout through Cargo configuration patches. [Cargo removes path dependencies when packaging](https://doc.rust-lang.org/cargo/commands/cargo-package.html), so verification would otherwise select the published `0.1.0` siblings while new APIs await the next release. Archive extraction and build verification remain enabled. The recipe uses a separate `package-check` target directory under the workspace target directory (or `CARGO_TARGET_DIR`) so verification builds cannot overwrite the workspace's native library artifacts.

## Maintaining the pipeline

- Every action is pinned to a full commit SHA with its release tag in a comment. zizmor's online audit fails CI when a comment does not match its SHA.
- Dependabot proposes action and dependency updates into `dev` once a release is seven days old. Dependabot security updates stay off: they would open pull requests against `main` and bypass `dev`.
- To bump `actionlint` or `committed`, change the version in `scripts/install-lint-tools.sh` and replace every checksum with the values from the release assets. To bump `zizmor`, change its version in the `justfile`.
- `.github/actionlint.yaml` declares the `ubuntu-26.04` runner label, which actionlint 1.7.12 does not know yet. Remove it once a newer actionlint lists that label.
- Later workflows plug in as follows:
  - the benchmark and the release workflows are separate files with their own triggers;
  - none of them repeats the gate.
