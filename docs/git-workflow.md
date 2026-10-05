# Git workflow

How changes move into this repository. It applies to people and AI agents alike. [AGENTS.md](../AGENTS.md) holds the short rules. This file explains them and gives the exact steps.

## Model

- `main` is the only long-lived branch. It must always build, and `just ci` must stay green on Linux, macOS and Windows.
- Small, verified steps land directly on `main`. Use a short-lived branch and a pull request when you want CI to pass before the change merges: a risky refactor, a toolchain or CI change, or anything you cannot verify on your own OS.
- Branch names follow `<type>/<short-kebab-topic>`, for example `feat/markdown-reader`, `fix/windows-temp-cleanup` or `ci/pin-actions`. Delete the branch after it merges.
- Never force-push `main`, and never rewrite history that has already been pushed.

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

Scopes follow the code layout: `core`, `ir`, `host`, `engine`, `cli`, `fixtures`, `schemas`, `brand`, `deps`. Leave the scope out when a change spans the whole repository.

Examples:

```text
feat(core): parse YAML front matter into document metadata
fix(host): kill the engine process tree on Ctrl-C
build(deps): pin comrak 0.50.0
docs: record the ashift binary name in ARCHITECTURE.md
```

## Before you commit

1. Run the narrowest check for what you touched, then broaden it:
   - Rust: `cargo fmt --all`, then `cargo clippy --workspace --all-targets` and `cargo nextest run -p <crate>`.
   - Brand: `pnpm --dir brand build`, then check `brand/preview.png` (see AGENTS.md).
   - Shared contracts (IR, engine protocol, schemas, CI): `just ci`.
2. Before a push, run `just ci`. It is the same gate CI runs.
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

- Push after a group of green commits, and at least at the end of each working session, so CI checks your work early.
- After the push, watch the run with `gh run watch`. If CI turns red, fixing `main` comes first. Push a fix, or revert the commit with `git revert <sha>`. Do not stack new work on a red `main`.
- To fix an unpushed commit, use `git commit --amend` for the latest one. Once a commit is pushed, fix it with a new commit.
- Commit hooks are never skipped with `--no-verify`.

## Pull requests

When you use a branch:

- Keep the PR to a single purpose. Its title follows the commit-message format.
- In the description, state what changed, why, and how it was verified.
- Merge only when all three OS jobs are green. Rebase onto `main` instead of merging `main` in. Squash only when the branch commits are noise; keep the commits when each one stands alone.

## Working through a plan

Plans live under `plans/`. When a plan phase is implemented:

- Commit after each logical step of the phase, following the rules above. A phase normally produces several commits, not one.
- Commit the phase status update (`plan.md` table row, ticked phase checklist) after the phase is verified, as `docs(plans): …`.
- Run artifacts under `plans/**/reports/herdr-cook-runs/` are ignored by their own `.gitignore` and are never committed.

## AI agents

AGENTS.md says agents commit or push only when the user asks. Running an authorized plan counts as that request for commits inside the plan's scope. Pushes, repository settings and anything else outward-facing still need the user's explicit go-ahead. Agents follow every rule in this file. They stage explicit paths only, and they never put AI or tool references in commit messages.
