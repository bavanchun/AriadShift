# Coordinator protocol (Mac PM ↔ Sonnet coordinator ↔ workers)

Standing working model for every phase of this plan, set by the owner on 2026-10-31. Every session reuses it.

## Roles

| Role | Who | Does | Never does |
|---|---|---|---|
| Owner | the user | Product decisions, GO for outward actions (tags, public deploys, store/notarization secrets, Pro list, licence reviews) | — |
| PM / team lead | Mac Claude Opus 5.5 session (left pane) | Brainstorm, plan, per-phase scout-and-detail pass, write briefs, answer questions, review diffs, accept or reject gates, decide on the owner's delegated decisions | Hands-on implementation |
| Coordinator | Mac Claude Sonnet 5.5 session (right Rex pane) | Turns a brief into worker tasks, dispatches workers with `/agent-rex`, reviews each worker diff and test output, runs the local gate, opens/merges PRs, files issues for red CI, reports through the mailbox | Changing contracts, skipping a gate, pushing tags |
| Workers | Rex tabs via `/agent-rex` | Implement one task each, in the files they own, with tests | Touching files they do not own, git push/merge |

Worker models (owner decision D13):
- Web UI/UX and SwiftUI/macOS UI → `--claude --model sonnet --effort high`
- Rust engine, infrastructure, CI, bridge (UniFFI), backend, data → `--agy --model gemini-4-argon-high`
- Git (branch, commit, rebase, push, PR, merge, worktree cleanup) → `--agy --model gemini-3.8-flash-low` with exact commands
- Reviews → the coordinator itself, and the Mac PM for every gate (the PM may spawn a `code-reviewer` subagent for large diffs)

## Mailbox (durable, survives sessions)

`/Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift/.run/coord/` (git-excluded via `.git/info/exclude` `.run/`):
- `from-mac/<phase>-<NN>-<slug>.md` — briefs, directives, answers, approvals. First line `status: brief | directive | answer | approved | rejected`.
- `to-mac/<phase>-<NN>-<slug>.md` — questions, gate reports. First line `status: question | gate-ready | blocked`.
- The PM watches `to-mac/` with a Monitor; the coordinator polls `from-mac/` every 60 s while waiting and is also nudged with `rex send` to its pane.

## Per-phase flow

1. **Scout-and-detail (PM).** Before dispatch, the PM re-reads the phase file, scouts current code, updates file paths/line numbers and versions, and commits the refreshed phase file (docs-only). Deep mode requires this for every phase after phase 3.
2. **Brief (PM → coordinator).** `from-mac/pNN-01-brief.md`: phase file path, branch name, worktree path, task order (waves), model per task, gate list.
3. **Branch + worktree (coordinator, flash-low worker).** `git switch dev && git pull --ff-only`, `git worktree add ../01-AriadShift-wt-<slug> -b <branch> origin/dev`. Each shell starts with `source /Volumes/AriadWork/02-Ariadnev-Eco/.cache/env.sh`.
4. **Dispatch (coordinator).** One `/agent-rex` tab per task with `--cwd` the worktree; tasks in the same wave own disjoint files. Every brief includes: "If a command runs > 10 minutes or a tool hangs, stop and report. Never debug compilers, linkers, Xcode or other toolchains." The coordinator **looks at every worker tab (`rex capture`) at least every 10 minutes** and reports progress to the PM in plain words when asked — waiting only on a result file is not supervision. Worker brief = task section of the phase file + "Context (read first)" + files owned + acceptance + "write your report to `plans/reports/worker-pNN-tK-<model>.md`; commit only your files with a conventional commit, no AI trailers".
5. **Review each task (coordinator).** Read the diff and the worker's test output; reject with concrete notes (max 2 rejections, then gate to PM as `blocked`).
6. **Local gate (coordinator).** Run the phase's Verification block sequentially (one heavy build at a time on the Mac); it is deliberately narrowed to the changed crates/apps — full `just ci` is not run locally. **Integration owner**: only the coordinator regenerates shared/generated files (`schemas/*.json`, `THIRD_PARTY_LICENSES`, `Cargo.lock`, `capabilities.json`, registry dumps) once per wave; workers never commit them. Red local test → back to the owning worker. Unrelated pre-existing failure → note + GitHub issue, continue.
7. **Gate report (coordinator → PM).** `to-mac/pNN-gate.md`: commits, files, test commands + results, screenshots for UI, open risks, acceptance checklist with evidence per item.
8. **Acceptance (PM).** PM reviews diff (or spawns `code-reviewer`), checks acceptance evidence, answers `approved` or `rejected` with reasons.
9. **Merge (coordinator, flash-low worker).** Push branch, `gh pr create --base dev`, rebase-merge (squash only for noisy branches) as soon as GitHub allows (do not wait for CI, owner decision D14); delete the head branch after merge (`gh pr merge --rebase --delete-branch`; the repo no longer auto-deletes because promotions would delete `dev`). Red CI later → deduped `gh issue create` with run URL, failing step, first error lines; continue. **A red `dev` must be fixed before any new phase worktree branches from it.** Never `--admin`, never change branch protection or the `protect-main-dev` ruleset.
10. **Close-out.** Close worker tabs (`--close`), remove the worktree, tick the phase checkboxes and status (`ak plan` CLI), write `to-mac/pNN-done.md`.

**Cleanup after every worker (owner rule, 2026-11-01).** As soon as a worker's task is accepted (or abandoned), the coordinator cleans up everything that worker created, before dispatching the next one — not only at phase end:
- close its Rex tab (`agent-rex.py --close <name>`);
- stop any background process it started (dev servers, watchers, `cargo`/`pnpm` runs) — check its `background-pid:` lines and `ps`/`lsof` for its worktree path;
- remove its worktree once its commits are merged or cherry-picked; delete merged `agent-rex/*` and task branches locally and on the remote;
- `agent-rex.py --prune` when old run records pile up.
Each gate report lists what was cleaned up and anything kept (with the reason). Nothing the worker made may outlive its task without being listed.

## Gates that always stop for the owner

Pushing any tag; public deploys (web preview/prod); Apple signing/notarization secrets and Sparkle keys; the Pro tool list; model-weight licence approvals (LaMa, AI pack weights); Weblate host; promotion `dev → main`; anything deleting data or history.

## Quality bar (every task)

- Tests first-class: unit + golden/snapshot + error cases; fuzz targets for any parser.
- No mocks or fake data to make checks pass; no weakened tests.
- Versions verified on the registry before pinning; lockfiles committed.
- Licences: no AGPL/non-OSI in artifacts; new native libs only out of process; record every new dependency's licence in the task report.
- Docs updated in the same PR when user-visible behaviour, commands or contracts change.
