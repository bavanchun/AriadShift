# Code Review: Deleted Job Output Race

## Scope

- `apps/web/src/components/account/job-row.tsx`
- `apps/web/test/account-jobs.test.tsx`
- `/tmp/p11-int-evidence/web-delete-repro.log`
- `/tmp/p11-int-evidence/web-delete-fixed.log`
- `/tmp/p11-int-evidence/web-final-retest.log`
- `/tmp/p11-int-evidence/web-final-lint.log`

This was a read-only review. No package-manager, test, build, engine, release, or Git mutation was run by the reviewer.

## Verdict

PASS. The production repair is minimal, cause-aligned, and fully covered by a deterministic regression test. No blocking or informational finding remains.

## Cause and Repair

The completed output request could resolve after job deletion because the API promise was not required to reject on `AbortSignal` cancellation. Its fulfillment handler unconditionally called `setOutputs`, reviving the Download button after `onDelete` had cleared outputs and rendered the focused tombstone. The pre-fix log reproduces exactly this failure: the tombstone and deletion state were present, but `Download report.docx` reappeared after the stale promise resolved.

The repair closes both ordering windows:

- The output fulfillment handler checks `controller.signal.aborted` before calling `setOutputs`, matching the established abort-guard pattern in `JobsPanel`.
- Output rendering now requires `!deleted` and `current.status === "succeeded"`. A successful deletion sets the row status to `expired` and then marks it deleted, so either state transition suppresses stale outputs even if React has already queued an earlier output-state update.

No deletion threshold, API contract, polling behavior, retry behavior, focus behavior, or unrelated lifecycle state changed.

## Regression-Test Quality

The existing deletion test was strengthened without weakening its assertions:

- `Promise.withResolvers` holds the initial outputs request open.
- The test waits until that request has started before deleting the job.
- It waits for the deletion tombstone and verifies focus remains on it.
- It resolves the stale outputs response inside `act`.
- The existing checks still require the Deleted state, absence of the Delete button, and absence of every Download button.

This sequence deterministically reproduces the original race instead of relying on timer timing. The separate successful-job test still requires the normal Download button, performs a download, and verifies fresh output URLs are fetched, so the new render guard does not erase normal output behavior.

## Evidence

- Pre-fix deterministic reproduction: 1 failed test, with the late `Download report.docx` button observed after deletion.
- Focused repaired suite: 23/23 tests passed.
- Full web suite: 308/308 tests passed.
- Biome check over the five scoped files: passed with no fixes.
- `git diff --check` over the two reviewed files: clean.
- Concurrency: the asynchronous completion path is guarded both before state mutation and at rendering.
- Error boundary: rejected output requests retain the existing abort-aware error handling.
- API contract: `JobsApi.outputs` and its result shape are unchanged.
- Backward compatibility: successful jobs still render and refresh downloadable outputs.
- Input/auth/data exposure: no boundary, authorization, or external-data handling changed.
- Query efficiency: no database or list-query path is involved.

## Unresolved Questions

- None.

Status: DONE
Summary: The repair prevents an ignored-abort output response from reviving downloads after deletion, preserves the focused tombstone, and retains normal successful-job downloads. The deterministic regression and full 308-test web suite pass.
Concerns/Blockers: None.
