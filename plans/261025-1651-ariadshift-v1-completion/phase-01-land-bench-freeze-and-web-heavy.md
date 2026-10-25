---
phase: 1
title: "Land #49 (bench, contract freeze) then #48 (web heavy routes)"
status: pending
depends_on: []
branches: [feat/heavy-route-bench-and-freeze, feat/web-heavy-routes]
---

# Phase 1: Land #49 then #48

Closes roadmap phases 5 and 7. The two branches already exist with their PRs, so they keep their own branches instead of one phase branch.

## Steps and tasks

| Step / task | Owner (model) | Files owned | Work |
|---|---|---|---|
| C1 | Coordinator | git, `docs/ci.md` on #49 | Rebase #49 onto `origin/dev` and resolve the `docs/ci.md` conflict by keeping both sides' content. Run `ak-code-review` on #49 (critical/high focus). |
| T1 | Opus 5.5 High | #49's diff paths: `bench/**`, `fixtures/**`, `crates/*/tests/**`, `crates/ariad-core/src/{protocol,pipeline}.rs`, `crates/ariad-core/src/ir/**`, `crates/ariad-core/data/capabilities.json`, `docs/bench*.md` | Fix #49's critical and high review findings. Skip if there are none. |
| C2 | Coordinator | git | Build/test #49, squash-merge it, then rebase #48 onto the new `dev` and run `ak-code-review` on #48. |
| T2 | Opus 5.5 High | #48's diff paths: `apps/web/**`, `crates/ariad-wasm/**`, `packages/**`, `scripts/ci-scope.sh`, `docs/web.md` | Fix #48's critical and high review findings. Skip if there are none. |
| C3 | Coordinator | git, roadmap `plan.md` rows 5 and 7 | Build/test #48, squash-merge it, and set roadmap rows 5 and 7 to Done (the v0.2.0 release itself stays in phase 3). |

## Build and test pass (coordinator, sequential, per PR after rebase)

```sh
source /Volumes/AriadWork/02-Ariadnev-Eco/.cache/env.sh
just pandoc
just ci          # static, clippy, wasm, tests, bench-test, bench-check, package-check, web-ci, desktop-ci
just web-e2e     # #48 only
```

## Acceptance

- [ ] #49 is merged, then #48, each green locally with no open critical or high finding.
- [ ] The schema drift tests show `ariad-engine/1` and `ariad-ir/1` frozen; `docs/bench-report.md` covers the heavy routes.
- [ ] The browser runs the DOCX, EPUB and PDF routes (#48 E2E).
