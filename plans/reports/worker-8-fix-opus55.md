# Worker report: p8-fix (release guard fixtures and install doc name)

Branch: `feat/v1-distribution--fix` (from `origin/feat/v1-distribution` at `1790d45`).

## Outcome

Both review items are fixed. The full guard suite runs to completion and
passes: 80 tests, 78 PASS, 2 SKIP (the two PowerShell syntax cases skip
because `pwsh` is not on PATH on this box), 0 FAIL.

## Finding on the review's prediction

The review predicted from source alone that `just check-release-workflow`
would stop at mutation 58. A run before any change showed that it did not stop:
the string `${DIST_ARCHIVE%.tar.xz}/dist` still appears once in
`.github/workflows/release.yml`, in the build-local "Install dist (Unix)" step
at line 164. Only the plan job (line 95) switched to the literal path
`cargo-dist-x86_64-unknown-linux-musl/dist`. The real defect was smaller.
Mutation 58 is described as a plan-job case, but because the plan job no longer
contains that string, it silently mutated the build-local job instead. Mutation
59 would also break as soon as the build-local step moves to literal paths.

## Changes

- `scripts/test-check-release-workflow.sh`
  - Mutation 58 (`plan-curl-pipe-sh`) now anchors on the plan job's literal
    member path, so it again exercises the plan job as described.
  - Mutation 59 (`run-sh-without-checksum`) keeps its build-local anchor, which
    is now the only parameter-expansion line in the workflow and therefore
    unambiguous. Its comment and description now name the build-local job. As
    a result, the dist install steps in both jobs are covered by a mutation.
  - No case was deleted, and `scripts/check-release-workflow.sh` is unchanged.
- `docs/install.md`: the macOS updater artifact in the desktop table now reads
  `AriadShift_aarch64.app.tar.gz` (`.sig`).

## Commits

- `291f692` test(release): anchor dist install mutations to current steps
- `2d27f7d` docs(install): correct the macOS updater archive name
- plus the commit that adds this report

## Verification (coordinator)

```sh
just check-release-workflow
# equivalently
sh scripts/check-release-workflow.sh && sh scripts/test-check-release-workflow.sh
```

Expected result: `SUCCESS: All mutations were rejected and all controls passed.`
`plan-curl-pipe-sh` and `run-sh-without-checksum` should both show PASS.
Running with `pwsh` installed also exercises the two skipped cases.

## Open concerns

- Line 13 of the header comment in `release.yml` says the build-local dist install
  uses a "literal member path", but the Unix step still uses
  `${DIST_ARCHIVE%.tar.xz}/dist`. Fixing that file was outside my scope. If
  someone later converts that step to literal paths, mutation 59 must be
  re-anchored at the same time.
- I took the updater archive name from the review and the brief, and did not
  check it against a real release run.

Status: DONE_WITH_CONCERNS
