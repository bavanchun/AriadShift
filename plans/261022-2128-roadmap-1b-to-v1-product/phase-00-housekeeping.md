---
phase: 0
title: "Housekeeping"
status: pending
priority: P1
effort: "3h"
dependencies: []
runs_on: Mac
---

# Phase 0: Housekeeping

## Goal

Start the run from a clean `dev`: land the v0.1.0 close-out documents and settle the open dependency pull requests, so later phases do not rebase over a moving lockfile.

## Requirements

1. **Release close-out.** The local branch `docs/release-0.1.0-closeout` (worktree `../01-AriadShift-wt-closeout`) holds two unpushed commits: the release procedure (`docs/releasing.md`, README, AGENTS.md) and the v0.1.0 evidence report. Rebase it onto `origin/dev`, check the documents against the live state (`gh release view v0.1.0`, `gh api repos/bavanchun/AriadShift/environments/release/secrets`), correct anything stale, run `typos` and the link check, push, open a pull request into `dev`.
2. **Old plan close-out.** In `plans/261006-0544-docling-spike-and-roadmap-1a/plan.md`, tick the acceptance criteria that the evidence proves, leave the publishing criteria open with the reason (missing `HOMEBREW_TAP_TOKEN` and `CARGO_REGISTRY_TOKEN`, no winget submission), mark phase 13 Done with that note, and set the plan status to `completed`. Same pull request.
3. **Dependency pull requests** (#18, #20, #22, #25, #27, #28, #29 at the time of writing; re-list with `gh pr list --author app/dependabot`):
   - For each: read the diff and its CI result. Rebase with `@dependabot rebase` when stale.
   - Merge green, policy-compliant ones into `dev` with `gh pr merge --rebase --delete-branch`.
   - Repair red ones on a branch of their own (for example an API change in `saphyr-parser` 0.2 or `comrak` 0.56), keeping AGENTS.md "Versions": stable releases only, registry-verified, lockfile committed with its manifest. Update the version tables in ARCHITECTURE.md §2 when a pinned version changes.
   - Close a pull request only with a comment that explains why (for example superseded by a grouped update).
4. Leave `dev` green on all three OS jobs.

## Files

- Modify: `docs/releasing.md`, `README.md`, `AGENTS.md`, `plans/261006-0544-docling-spike-and-roadmap-1a/plan.md`, `plans/261006-0544-docling-spike-and-roadmap-1a/phase-13-release-gate-and-acceptance.md`, manifests and lockfiles touched by dependency updates, ARCHITECTURE.md §2 when pins change.

## Acceptance criteria

- The close-out pull request is merged into `dev`.
- Every dependency pull request is merged or closed with a reason.
- The latest CI run on `dev` is green.

## Risks

| Risk | Response |
|---|---|
| A dependency bump changes snapshot output | Review the `insta` diff; accept only when the change is correct, and say why in the commit body |
| `comrak` 0.56 changes parsing | Run the Markdown reader tests and fuzz seeds; pin back if behaviour regresses |
