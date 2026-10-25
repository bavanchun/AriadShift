# Distribution review delivery

The requested source review found no introduced critical or high issues.
The detailed [review report](code-review-261025-phase8.md) records the evidence,
scope limits, exact coordinator verification commands and merge verdict.

## Files changed

- `plans/reports/code-review-261025-phase8.md`.
- `plans/reports/worker-8-review-gpt61sol.md`.

No implementation, workflow or installation documentation was edited.

## Revision and commits

- Reviewed `origin/dev...origin/feat/v1-distribution`, from
  `e0f68a820c457ee729ee64b748e21097ce719d0a` to
  `1790d4546212a3d6d695b7a388e8955f3d0f522e`.
- Both reports are delivered together on `feat/v1-distribution--review` with
  commit subject `docs: record distribution review evidence`.
- Resolve the delivery commit with:

  ```sh
  git log -1 --format='%H %s' -- plans/reports/worker-8-review-gpt61sol.md
  ```

## What was checked

Reviewed desktop checksum coverage, Minisign trust, platform-signing gates,
PR and publishing permissions, artifact handoffs, Homebrew generation and
archive verification, optional publishing-token behavior, guard changes and
installation instructions. Checked the upstream sources pinned by the project
for formula structure and Tauri asset naming. `git diff --check` passed.
No builds or tests were run under the ASUS no-build/no-test rule.

## Coordinator verification commands

Run these sequentially in the shared build queue:

```sh
node --test scripts/desktop-release-sums.test.mjs
python3 scripts/release/test-verify-homebrew.py
just check-release-workflow
just lint-workflows
just package-check
just ci
just desktop-ci
just desktop-build
just lint-commits origin/dev..HEAD
```

The detailed review also supplies exact commands for signing and verifying a
local bundle with a throwaway key and rejecting it against the production
public keys, plus the release-rehearsal formula verification procedure.

## Open concerns

- The existing guard mutation fixtures at
  `scripts/test-check-release-workflow.sh:802` and line 808 target a tar command
  absent from both base and reviewed workflows. Source inspection predicts a
  `Target string not found` failure in the coordinator's guard run. This is
  inherited from `origin/dev`, not introduced by the reviewed diff.
- `docs/install.md:209` adds a version to the macOS updater archive name; the
  pinned action publishes `AriadShift_aarch64.app.tar.gz`. The DMG name,
  checksum collection and automatic updater remain correct. This is below
  the requested critical/high severity; the owning docs worker should fix it.
- Build, signing rehearsal, real formula and release behavior remain untested
  here. GHCR/Compose require the owning cloud branch; this review does not
  certify sibling implementation or live publication.

Unresolved product questions: none.

Verdict: MERGEABLE

Status: DONE_WITH_CONCERNS
