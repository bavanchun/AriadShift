# Continuous integration

How GitHub Actions checks this repository, and how to run the same checks locally. The branch model it serves is in [git-workflow.md](git-workflow.md).

## Workflows

| Workflow | Runs on | What it does |
|---|---|---|
| [`ci.yml`](../.github/workflows/ci.yml) | Pull requests into `dev` or `main`, pushes to `dev` or `main`, manual | The gate for every change (below) |
| [`bench.yml`](../.github/workflows/bench.yml) | Pull requests with `bench` label, nightly schedule, manual | Runs benchmark harness, compares metrics against baseline, and validates capabilities schema |
| [`codeql.yml`](../.github/workflows/codeql.yml) | Pull requests (code changes), pushes to `dev` or `main`, weekly, manual | CodeQL for Rust and for the workflows themselves; alerts go to the repository's Security tab |
| [`security.yml`](../.github/workflows/security.yml) | Daily, manual, and pull requests that change it | `cargo deny check advisories` on `dev` and on `main`, so a newly published RustSec advisory fails a run even when no code changed |
| [`fuzz-nightly.yml`](../.github/workflows/fuzz-nightly.yml) | Daily, manual, and pull requests that change it | Date-pinned nightly AddressSanitizer fuzzing on unsafe-adjacent parsers (10 min per target) |
| [`links.yml`](../.github/workflows/links.yml) | Weekly, manual, and pull requests that change it | lychee checks every Markdown link on `dev`, configured by [`lychee.toml`](../lychee.toml) |
| [`dependabot.yml`](../.github/dependabot.yml) | Weekly (Monday, Asia/Saigon) | Update pull requests into `dev` for GitHub Actions, Cargo, pnpm and uv |

GitHub runs `schedule` triggers, and offers the manual "Run workflow" button, only for workflows on the default branch, `main`. A new scheduled workflow therefore starts after the next promotion; until then, its pull request run is its test.

## The CI gate

`ci.yml` has six jobs:

1. **Scope** (Linux, seconds) runs [`scripts/ci-scope.sh`](../scripts/ci-scope.sh). It picks the commit range to lint and decides whether the build, test and fuzz jobs must run.
2. **Static checks** (Linux) run once for every event:
   - each new commit against the commit rules (`just lint-commits`);
   - `just static`: formatting, `typos`, `actionlint`, `zizmor` with online audits, `cargo deny`, and the pnpm and uv lockfiles.
3. **Lint (windows-2025)** runs Clippy on Windows, for early feedback on Windows-only code.
4. **Verify** runs on `ubuntu-26.04`, `macos-26` and `windows-2025`. It installs Pandoc, sets up uv, runs Clippy (Windows already has it from job 3), the WASM check (Linux only, since it does not depend on the host), `just test`, `just bench-test`, and `just bench-check`.
5. **Fuzz** (Linux, `ubuntu-26.04`) runs cargo-fuzz 0.13.2 on stable Rust with `-s none` for 60 seconds per target (`markdown_reader`, `front_matter`, `html_reader`, `pandoc_ast_to_ir`, `ir_json`, `limits_validate`), with cached corpus and crash artifact upload on failure or cancellation.
6. **CI passed** is the single result to look at. It fails unless Scope and Static checks passed, and jobs 3, 4 and 5 either passed or were skipped by the scope decision.

### When the build, test and fuzz jobs are skipped

The same tree is never built twice without a reason:

| Event | Build, test and fuzz | Why |
|---|---|---|
| Pull request into `dev` | Run | The change is new |
| Push to `dev` after a merge | Run | Only runs on a base branch may save the Rust cache that later pull requests read, and `dev` may have moved since the pull request ran |
| Promotion pull request into `main`, and the push to `main` after it | Skipped when the tree matches a `dev` commit whose CI push run succeeded | That exact tree was already built and tested on all three OSes |
| Hotfix pull request into `main`, and the push after it | Run | The tree is new |
| Change to `docs/`, `plans/`, or the root `README.md`, `ARCHITECTURE.md`, `AGENTS.md`, `CLAUDE.md` only | Skipped | Static checks still spell-check and lint it |
| Manual run, or a push whose previous commit is unknown | Run | Nothing to compare against |

A newer push to the same pull request cancels its older run. Runs on `dev` and `main` are grouped by commit and are never cancelled.

CodeQL is the one deliberate repeat: GitHub computes pull request alerts against the base branch's analysis, so `dev` and `main` are analysed on push too.

The Rust cache is written only by pushes to `dev` and `main`; pull requests read it but never save their own, so they cannot evict the base branch's cache. The fuzz caches (cargo-fuzz binary, `fuzz/target`, and corpus) follow the same policy: saved on pushes to `dev` and `main`, while pull requests restore caches without saving. The nightly workflow (`fuzz-nightly.yml`) saves caches on schedule, dispatch and its own pull request runs (PR-scoped entries are not visible to `dev` or `main`); crash artifacts upload on failure or cancellation.

## Commit rules

[`scripts/check-commits.sh`](../scripts/check-commits.sh) checks each commit in the range:

- [committed](https://github.com/crate-ci/committed), configured by [`committed.toml`](../committed.toml): Conventional Commits with this repository's types, a subject of at most 72 characters in the imperative mood, without a trailing period, and body lines of at most 72 columns;
- no reference to an AI tool anywhere in the message (`git-workflow.md` forbids them).

Dependabot's own subjects are exempt from the committed rules, not from the AI check.

## Running the checks locally

| Command | Runs |
|---|---|
| `just ci` | Everything CI runs on your OS: `just static`, Clippy, the WASM check, tests, `bench-test`, and `bench-check` |
| `just bench` | Run benchmark harness on fixture suite |
| `just bench-check` | Validate capabilities.json against schema and invariants |
| `just bench-test` | Run unit tests for benchmark harness |
| `just static` | The OS-independent checks |
| `just lint-commits` | Your commits since `origin/dev`; pass a range to check others, for example `just lint-commits main..dev` |
| `just lint-workflows` | `actionlint` and `zizmor`. Set `GH_TOKEN` (for example `GH_TOKEN=$(gh auth token)`) to enable zizmor's online audits, which CI always runs |
| `just deny-advisories` | The daily advisory scan |
| `just fuzz` | Run fuzz targets locally on stable Rust (`target=""` for all, `seconds="60"`) |

`just lint-tools` installs `actionlint` and `committed` into `.tools/bin`, verified against SHA-256 checksums pinned in [`scripts/install-lint-tools.sh`](../scripts/install-lint-tools.sh). `zizmor` runs through `uvx` at the version pinned in the `justfile`.

## Maintaining the pipeline

- Every action is pinned to a full commit SHA with its release tag in a comment. zizmor's online audit fails CI when a comment does not match its SHA.
- Dependabot proposes action and dependency updates into `dev` once a release is seven days old. Dependabot security updates stay off: they would open pull requests against `main` and bypass `dev`.
- To bump `actionlint` or `committed`, change the version in `scripts/install-lint-tools.sh` and replace every checksum with the values from the release assets. To bump `zizmor`, change its version in the `justfile`.
- `.github/actionlint.yaml` declares the `ubuntu-26.04` runner label, which actionlint 1.7.12 does not know yet. Remove it once a newer actionlint lists that label.
- Later workflows plug in as follows:
  - the benchmark and the release workflows are separate files with their own triggers;
  - none of them repeats the gate.
