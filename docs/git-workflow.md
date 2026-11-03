# Git workflow

How changes move into this repository. It applies to people and AI agents alike. [AGENTS.md](../AGENTS.md) holds the short rules. This file explains them and gives the exact steps.

## Model

- Two long-lived branches:
  - `dev` is the integration branch. All work lands here.
  - `main` is the stable branch. It receives `dev` only through a promotion, and urgent fixes through a hotfix (see [Promotion and hotfixes](#promotion-and-hotfixes)). Release tags (`v*`) are cut on `main` only.
- Both must always build, and `just ci` must stay green on Linux, macOS and Windows.
- Work in a plan run or feature uses a short-lived branch from `origin/dev` (one branch and one pull request per phase) and merges into `dev` as soon as GitHub allows. Small, verified standalone steps may also land directly on `dev` when authorized.
- Branch names follow `<type>/<short-kebab-topic>`, for example `feat/markdown-reader`, `fix/windows-temp-cleanup` or `ci/pin-actions`.
- Repository ruleset and branch settings:
  - The `protect-main-dev` ruleset blocks branch deletion and force-pushes on `main` and `dev`.
  - Repository `delete_branch_on_merge` is `false` so that promoting `dev` into `main` can never delete `dev`.
  - Task and phase branches are therefore deleted explicitly when merging into `dev` with `gh pr merge --rebase --delete-branch`. Promotions never pass `--delete-branch`.
- GitHub's default branch stays `main`, so always pass the base explicitly: `gh pr create --base dev`.
- Never force-push `main` or `dev`, never pass `--admin`, never weaken or edit branch protection or rulesets, and never rewrite history that has already been pushed.

## Commit often

Commit at every logical step that leaves the tree working. Do not save everything for one large commit at the end.

A good step is one of these:

- one crate, module or recipe that compiles and passes its tests;
- one decision recorded in a document;
- one test suite added together with the code it covers;
- one dependency or toolchain bump, with its lockfile;
- one plan phase status update, after the phase is verified.

Rules for every commit:

- **Atomic.** One purpose per commit. If the message needs "and" to join two unrelated changes, split it.
- **Green.** It builds and passes the checks relevant to what it touched (see [Before you commit](#before-you-commit)). A red intermediate commit breaks `git bisect` and CI.
- **Complete.** Lockfiles go in with their manifests. Generated outputs go in with their sources: `brand/` outputs, JSON Schemas under `schemas/` and `insta` snapshots.
- **Reviewed.** Read `git diff --staged` before every commit.

## Commit messages

Use [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/):

```text
<type>(<scope>): <subject>

<body: why the change was made and what it affects, wrapped at 72 columns>

<footer: BREAKING CHANGE: …, Refs: #123>
```

- **Subject:** imperative mood, lower case, no trailing period, at most 72 characters. Write "add Markdown reader", not "added" or "adds".
- **Body:** optional for a trivial change, expected otherwise. Explain the reason and any trade-off. The diff already shows what changed.
- **Never** mention AI tools, agents, plan IDs, phase numbers or audit labels. Describe the behavior or invariant instead.

| Type | Use for |
|---|---|
| `feat` | New user-visible behavior |
| `fix` | A bug fix |
| `docs` | Documentation only |
| `test` | Tests only |
| `refactor` | A code change with no behavior change |
| `perf` | A performance improvement |
| `build` | Cargo, pnpm, uv, `justfile` or toolchain pins |
| `ci` | GitHub Actions workflows |
| `chore` | Repository housekeeping that fits nothing else |

Scopes follow the code layout: `core`, `ir`, `host`, `engine`, `cli`, `web`, `wasm`, `ui`, `fixtures`, `schemas`, `brand`, `deps`. Leave the scope out when a change spans the whole repository.

Examples:

```text
feat(core): parse YAML front matter into document metadata
fix(host): kill the engine process tree on Ctrl-C
build(deps): pin comrak 0.50.0
docs: record the ashift binary name in ARCHITECTURE.md
```

## Before you commit

1. Run only one heavy build or test command at a time on the host: every `cargo`, `just`, `pnpm`, `uv` or `fixtures` command run by a worker or the coordinator must go through `.run/coord/build-lock.sh`.
2. Run the narrowed local gate for the crates and apps you touched instead of running full `just ci` locally:
   - Rust: `cargo fmt --all -- --check`, then `cargo clippy` and `cargo nextest run` scoped to the changed crates.
   - Web / brand: the touched package's build, lint and test recipes (for `brand/`, run `pnpm --dir brand build` and check `brand/preview.png`; see AGENTS.md).
   - Spelling and commits: `just spell` and `just lint-commits`. `typos` also checks Markdown, so a documentation, plan or report edit alone can turn CI red.
3. Check `git status` for stray files: editor backups, `.tools/`, `target/`, `node_modules/`.

## Staging

Stage explicit paths:

```bash
git add crates/ariad-core/src/ir.rs crates/ariad-core/Cargo.toml Cargo.lock
git diff --staged
git commit
```

Never run `git add -A`, `git add .` or `git commit -a` at the repository root. They pick up unrelated files and local run artifacts.

Never commit `.env*` files, credentials, private keys, tokens, `node_modules/`, `target/`, `.tools/` or any user or private document. `fixtures/` only takes freely distributable documents with their source and license recorded.

## Pushing

- Push after the narrowed local gate passes so GitHub Actions runs on the branch.
- CI is not awaited before merging or moving on. If a CI run turns red after merge, file a deduplicated GitHub issue (label `v1.x`) with the run URL, the failing step and the first error lines, and continue current work.
- Fixing a red `dev` or `main` comes before any new stacked work: a red `dev` must be repaired (push a fix or revert with `git revert <sha>`) before any new phase worktree branches from it. Do not stack new work on a red branch.
- To fix an unpushed commit, use `git commit --amend` for the latest one. Once a commit is pushed, fix it with a new commit.
- Commit hooks are never skipped with `--no-verify`.

## Pull requests

When you use a branch:

- Keep the PR to a single purpose (one branch and one pull request per plan phase). Its title follows the commit-message format.
- In the description, state what changed, why, and how it was verified locally.
- Do not wait for CI jobs to finish. Rebase onto the base branch instead of merging it in, and merge into `dev` as soon as GitHub allows using a rebase-merge (`gh pr merge --rebase --delete-branch`, squash only when the branch commits are noise) so each commit stays on its own and the task branch is deleted. Never pass `--admin` and never change branch protection or rulesets.
- If the post-merge CI run is red, record a deduplicated GitHub issue (`v1.x` label) with the run URL, the failing step and the first error lines, and fix `dev` before branching the next phase worktree from it.

## Promotion and hotfixes

- **Promotion.** When `dev` is stable and the maintainer gives written go-ahead, open a pull request from `dev` into `main` titled `chore: promote dev to main`. Merge it with a merge commit (`gh pr merge --merge`, never with `--delete-branch`), never squash or rebase, so the promoted commits keep their SHAs and the next promotion stays a clean merge. Verify immediately afterwards that `dev` still exists on the remote and that `main^2` equals `origin/dev` (`git rev-parse main^2` matches `git rev-parse origin/dev`).
- **Release.** Bump the version on `dev` through the usual flow, promote, then tag `vX.Y.Z` on the resulting `main` commit. The release workflow runs only from that tag.
- **Hotfix.** Branch `fix/<topic>` from `main`, open the pull request into `main`, and merge it. Then merge `main` back into `dev` (`git switch dev && git merge main`, then push) so the fix is not lost at the next promotion.
- Promotions, hotfixes into `main`, release tags and public deploys are outward-facing and always require the maintainer's written go-ahead.

## Working through a plan

Plans live under `plans/`. A plan run uses one branch and one pull request per phase:

### Roles and mailbox

- **Mac PM session (Opus):** makes design and scope decisions, scouts and updates phase files before dispatch, writes phase briefs, and approves or rejects each phase gate.
- **Coordinator session (Sonnet):** turns each brief into worker tasks, dispatches workers, inspects worker tabs, reviews every diff and test output, runs the narrowed local gate sequentially, and reports through the mailbox.
- **Implementation workers (Rex tabs):** implement one task each inside the files they own, commit their changes locally, and never push or merge.
- **Git worker (low-effort model):** runs exact git commands for branch creation, rebasing, pushing, pull request creation, merging and worktree/branch cleanup.
- **Mailbox:** lives at `.run/coord/from-mac/` (briefs, directives, answers and approvals from the PM) and `.run/coord/to-mac/` (questions and gate reports from the coordinator), excluded from git via `.git/info/exclude`. Every mailbox message starts with a `status: <state>` first line.

### Per-phase execution and cleanup

1. **Check `dev`.** Before branching a new phase, check `dev` CI. If `dev` is red, fix `dev` first before creating a new phase worktree from it.
2. **One worktree per phase.** Branch from `origin/dev` into a dedicated phase worktree at `../01-AriadShift-wt-<slug>` (for example `git worktree add ../01-AriadShift-wt-<slug> -b <type>/<topic> origin/dev`).
3. **Waves and file ownership.** Tasks inside a wave own disjoint files. Workers commit after each logical step (`docs(plans): …` for the phase status and checklist update once verified). Run artifacts under `plans/**/reports/herdr-cook-runs/` and `.run/` are git-excluded and never committed.
4. **Build serialization and narrowed local gate.** Every `cargo`, `just`, `pnpm`, `uv` or `fixtures` command run by a worker or the coordinator executes through `.run/coord/build-lock.sh` so only one heavy build runs at a time. The coordinator regenerates shared or generated files once per wave and runs the phase's narrowed verification commands for the changed crates and apps; full `just ci` is not run locally.
5. **Cleanup after each worker.** As soon as a worker's task is accepted (or abandoned):
   - close its Rex tab;
   - stop only the background processes that worker started (using its reported PIDs and processes bound to its worktree path);
   - remove its worktree once its commits are merged, and delete merged task and `agent-rex/*` branches locally and on the remote;
   - check unmerged branches with `git cherry origin/dev <branch>`: any branch that is not provably merged is never deleted automatically and is instead recorded on a delete list for the maintainer.
6. **Gate report and merge.** Each gate report lists what was cleaned up and what was kept (with the reason). Once the Mac PM approves the phase gate, the git worker pushes the phase branch, opens a pull request into `dev`, and merges immediately with `gh pr merge --rebase --delete-branch` as soon as GitHub allows (never `--admin`). If the post-merge CI run fails, the coordinator files a deduplicated GitHub issue (`v1.x` label) with the run URL, failing step and first error lines, and work continues.

A plan run never promotes `dev` to `main`, never tags a release, and never triggers a public deploy on its own.

## AI agents

[AGENTS.md](../AGENTS.md) summarises these rules. Commit and push permissions (including opening and rebase-merging pull requests into `dev`) follow an authorized plan run, where the plan or the maintainer's directive records the grant. Promotions to `main`, release tags, public deploys and repository or ruleset settings always require the maintainer's explicit written go-ahead. Agents stage explicit paths only, never commit secrets or private documents, and never put AI or tool references in commit messages.
