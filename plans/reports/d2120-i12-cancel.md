# Browser conversion cancellation

Cancellation is complete and the requested gates passed on the exact implementation HEAD recorded in the Sol continuation below. The original handoff is preserved as history; its pending-work notes are superseded. No branch was pushed and no PR was opened.

## Original handoff (historical)

Work is paused under owner policy D-1810. The implementation and its unit tests are committed; the revised WebKit end-to-end test is staged but has not been run. No branch was pushed.

### Changes and state

- Implementation commit: `bb08eca0c9c9fd4404e1c43afd26cf3dd4eca0fa` (`fix(web): cancel in-flight browser conversions`). This is the exact head tested by the main gate below. A report-only commit may follow it.
- Cancel and Start over terminate the active worker, reject its pending requests with non-fatal `cancelled`, and queue its session for the next worker to retire. Recycling rejects pending requests with `worker_replaced`; stale worker events are ignored. Retirement remains queued if an intervening worker is stopped before acknowledging it.
- The UI starts over on cancellation. Device cleanup is queued before another file can be selected. `docs/web.md` describes this behavior.
- `apps/web/e2e/cancel.spec.ts` has a staged, unverified two-line change selecting Word (DOCX) for the initial large conversion. The original Markdown-to-HTML conversion finished before WebKit could click Cancel. The revised route is intended to make the conversion long enough to cancel; it has not been proven. The second file still expects the default HTML output.
- No test gate from this worker remains running or holding the build lock. The queued WebKit retry was stopped with SIGTERM before it started because of D-1810. The load was above 50 when stopped. Do not stop processes owned by other workers.

### Evidence

All heavy runs were detached with `CARGO_BUILD_JOBS=4` and the shared `gate-slot.sh 1` lock. Logs are under `/home/vchun/Codes/ariadshift-logs/`.

- Failing first: `pnpm --filter @ariadshift/web exec vitest run test/client-cancel.test.ts test/converter-app.test.tsx`; `i12-red.log` ends `STOPPED: red rc=1`. Three tests failed on missing cancellation, unsettled recycling, and the absent Cancel button.
- Focused after implementation: `pnpm --filter @ariadshift/web exec vitest run test/client-cancel.test.ts test/converter-app.test.tsx test/client.test.ts`; `i12-focused-final.log` ends `=== focused ALL GREEN` with 11 tests passed.
- The first full attempt, `i12-gate-initial.log`, ends `STOPPED: pnpm --filter @ariadshift/web test rc=1`: the fresh worktree lacked built WASM, and an unrelated account jobs test also failed. After `just wasm-build`, the full web test passed on the next run, including that account test.
- `i12-gate-webkit-failed.log` records `just wasm-build`, `pnpm --filter @ariadshift/web test` (260 passed), `just web-ci` (355 tests passed, typecheck and production build passed), and `CI=1 pnpm --filter @ariadshift/web exec playwright test e2e/cancel.spec.ts --project=chromium --project=firefox` (both passed). Its final line is `STOPPED: WebKit gate failed`.
- `zsh /home/vchun/Codes/ariadshift-ops/webkit-e2e.sh /home/vchun/Codes/01-AriadShift-wt-i12-cancel` ran the whole WebKit suite. `/home/vchun/Codes/ariadshift-logs/webkit-01-AriadShift-wt-i12-cancel-173600.log` ends `STOPPED: webkit rc=1`: 27 passed, 22 skipped, and the cancel test failed on both attempts. Its error context shows the HTML conversion had completed and the Cancel button detached before Playwright clicked it. This does not establish that the staged DOCX change fixes the race.
- `just pandoc` succeeded in the initial gate. `typos` and `just lint-commits` have not run; the gate stopped at WebKit before reaching them.

### Original continuation checklist

1. Review the staged DOCX test change and run the new cancel spec on all three browsers, including WebKit through the project's container recipe, under the shared build lock and load guard. The normal WebKit script runs the whole suite and records its own log; inspect its final line because its shell exit status alone does not reliably indicate test failure.
2. If WebKit still misses Cancel, diagnose with its error context and make the real conversion long enough to exercise cancellation without substituting a fake conversion. Verify the second conversion uses a fresh worker.
3. Commit the validated test change, rerun affected checks on the exact resulting head, and run `typos` and `just lint-commits`. Record exact commands, logs, final lines, and head SHA. The previous `just web-ci` and web unit results apply only to the implementation head noted above, before the staged test change.
4. Update and copy this report to `/home/vchun/Codes/ariadshift-ops/reports/`. Do not push or open a PR; the integration review loop requires acceptance first.

## Sol continuation

### Outcome and commits

Reviewed the inherited client, UI, retirement protocol, tests and browser failure logs. Cancel and Start over terminate the worker, settle pending requests with non-fatal `cancelled`, and queue its session for retirement. Recycling settles old requests with `worker_replaced`. No open cancellation finding remains after local review.

The new e2e file now cancels a real large Markdown-to-DOCX conversion, then converts another Markdown file to HTML and verifies a fresh worker was created. The DOCX route keeps the conversion active long enough for WebKit to click Cancel, without a fake conversion or an artificial delay.

Review also found a regression in parallel cleanup: a device-reset failure hid a retained server copy. The correction records the server outcome independently. A new UI regression test proves the copy remains visible and retryable while the device error is shown.

- Inherited implementation: `bb08eca0c9c9fd4404e1c43afd26cf3dd4eca0fa`.
- Inherited handoff: `03255992ba477c5df50fd73b9ac75207c1022084`.
- Cleanup correction: `2852692e7be9028b275d42f51c01e7b2b9d773a8` (`fix(web): retain server copy notices when device cleanup fails`).
- Validated e2e change and exact tested implementation HEAD: `6828382bd4bcbbfa172ee6b885b91f60398834ab` (`test(web): cancel a DOCX conversion before restarting`).
- A report-only commit follows that tested HEAD. Its exact SHA and report checks are recorded in `/home/vchun/Codes/ariadshift-logs/i12-sol-closeout.log`.

`ARCHITECTURE.md` and `apps/web/src/converter/engines.ts` were not changed. Existing `docs/web.md` cancellation documentation remains accurate. No merge, push or PR was performed; the branch awaits lead review.

### Failing-first evidence and earlier attempts

Continuation test and build runs were detached through `/home/vchun/Codes/ariadshift-ops/gate-slot.sh 2`, applying `nice -n 19` and load/memory guards, with `CARGO_BUILD_JOBS=4`. Tool pins were `MISE_UV_VERSION=0.12.23`, `MISE_PNPM_VERSION=12.9.1`, and `MISE_NODE_VERSION=$(cat .node-version)`. The initial queued slot-1 check was terminated before acquiring its lock and was not used as evidence. No other worker's process was stopped.

- Original cancellation red evidence is preserved above; its implementation base was `bcfac8c2c07ee3b0162d57ef7b21ec654fed166f`.
- `/home/vchun/Codes/ariadshift-logs/i12-sol-regression-red.log`: `pnpm --filter @ariadshift/web exec vitest run test/client-cancel.test.ts test/converter-app.test.tsx test/client.test.ts` passed 11 tests; `pnpm --filter @ariadshift/web exec vitest run test/cloud-start-over.test.tsx` then failed the new retained-copy assertion before the correction. Last line: `STOPPED: cloud regression rc=1`.
- `/home/vchun/Codes/ariadshift-logs/i12-sol-focused-green.log`: `pnpm --filter @ariadshift/web exec vitest run test/client-cancel.test.ts test/converter-app.test.tsx test/client.test.ts test/cloud-start-over.test.tsx` passed 15 tests after the correction. Chromium and Firefox cancellation passed. The raw container log `/home/vchun/Codes/ariadshift-logs/webkit-01-AriadShift-wt-i12-cancel-184321.log` ends `=== webkit ALL GREEN` (28 passed, 22 existing skips). The wrapper ended `STOPPED: WebKit gate failed` because the shell's `ls` alias broke log discovery. The temporary runner was corrected to use native glob ordering; the final gate reran verification successfully.
- `/home/vchun/Codes/ariadshift-logs/i12-sol-gate-account-failed.log`: the first full gate on the final implementation HEAD passed 260 tests and failed the unchanged Jobs test `does not list account jobs from the API when signed out` because `anonymous.pdf` was absent. Last line: `STOPPED: pnpm --filter @ariadshift/web test rc=1`. No account files or assertions were changed. The final gate ran Jobs alone (20 passed), then both unchanged full suites passed.

### Final gate

Exact tested implementation HEAD: `6828382bd4bcbbfa172ee6b885b91f60398834ab`.

Gate log: `/home/vchun/Codes/ariadshift-logs/i12-sol-gate-final.log`. The runner was launched detached as `gate-slot.sh 2 zsh -li /tmp/i12-sol-gate.zsh`, with output redirected to that log. It started at `2026-10-28T20:19:37+07:00` and completed at `2026-10-28T20:26:02+07:00`.

The exact checks, in order, were:

```sh
pnpm --filter @ariadshift/web exec vitest run test/account-jobs.test.tsx
pnpm --filter @ariadshift/web test
just web-ci
CI=1 pnpm --filter @ariadshift/web exec playwright test e2e/cancel.spec.ts --project=chromium --project=firefox --workers=2
zsh /home/vchun/Codes/ariadshift-ops/webkit-e2e.sh /home/vchun/Codes/01-AriadShift-wt-i12-cancel
typos
pnpm exec biome check apps/web/src/converter/client.ts apps/web/src/components/converter/converter-app.tsx apps/web/test/client-cancel.test.ts apps/web/test/converter-app.test.tsx apps/web/test/cloud-start-over.test.tsx apps/web/e2e/cancel.spec.ts
just lint-commits
git diff --check
```

All checks passed: 20 isolated Jobs tests, 261 web tests, 356 workspace tests, workspace typechecks, production build, both Chromium/Firefox cancellation tests, full WebKit, spelling, formatting and commit lint. Last line: `=== browser cancellation ALL GREEN`.

Final WebKit log: `/home/vchun/Codes/ariadshift-logs/webkit-01-AriadShift-wt-i12-cancel-202240.log`. Last line: `=== webkit ALL GREEN` (28 passed, 22 skipped). The cancellation test passed and was not skipped. Skips are existing cloud/storage/offline/browser conditions in the device-only build; no skips were added. Verification ran on Linux, with WebKit in the prescribed container.

### Closeout and concern

Report checks are `typos plans/reports/d2120-i12-cancel.md`, `git diff --check`, and `sh scripts/check-commits.sh origin/dev..HEAD`. Their outcome and the report commit HEAD are recorded in `/home/vchun/Codes/ariadshift-logs/i12-sol-closeout.log`. The additional attempt `pnpm exec biome check plans/reports/d2120-i12-cancel.md` returned "No files were processed": `biome.json` excludes plans from its include patterns. No configuration was changed; the TypeScript files passed Biome in the final gate. This report is copied to `/home/vchun/Codes/ariadshift-ops/reports/d2120-i12-cancel.md`. All owned gate processes completed and port 3100 was free after the final browser run.

The existing Jobs test intermittency remains outside this change. Its isolated run and unchanged full suites passed on retry, but its underlying cause was not established here. This is the only open concern; no cancellation acceptance criterion remains unmet.

Status: DONE_WITH_CONCERNS
