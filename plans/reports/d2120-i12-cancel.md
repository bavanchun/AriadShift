# Browser conversion cancellation handoff

Work is paused under owner policy D-1810. The implementation and its unit tests are committed; the revised WebKit end-to-end test is staged but has not been run. No branch was pushed.

## Changes and state

- Implementation commit: `bb08eca0c9c9fd4404e1c43afd26cf3dd4eca0fa` (`fix(web): cancel in-flight browser conversions`). This is the exact head tested by the main gate below. A report-only commit may follow it.
- Cancel and Start over terminate the active worker, reject its pending requests with non-fatal `cancelled`, and queue its session for the next worker to retire. Recycling rejects pending requests with `worker_replaced`; stale worker events are ignored. Retirement remains queued if an intervening worker is stopped before acknowledging it.
- The UI starts over on cancellation. Device cleanup is queued before another file can be selected. `docs/web.md` describes this behavior.
- `apps/web/e2e/cancel.spec.ts` has a staged, unverified two-line change selecting Word (DOCX) for the initial large conversion. The original Markdown-to-HTML conversion finished before WebKit could click Cancel. The revised route is intended to make the conversion long enough to cancel; it has not been proven. The second file still expects the default HTML output.
- No test gate from this worker remains running or holding the build lock. The queued WebKit retry was stopped with SIGTERM before it started because of D-1810. The load was above 50 when stopped. Do not stop processes owned by other workers.

## Evidence

All heavy runs were detached with `CARGO_BUILD_JOBS=4` and the shared `gate-slot.sh 1` lock. Logs are under `/home/vchun/Codes/ariadshift-logs/`.

- Failing first: `pnpm --filter @ariadshift/web exec vitest run test/client-cancel.test.ts test/converter-app.test.tsx`; `i12-red.log` ends `STOPPED: red rc=1`. Three tests failed on missing cancellation, unsettled recycling, and the absent Cancel button.
- Focused after implementation: `pnpm --filter @ariadshift/web exec vitest run test/client-cancel.test.ts test/converter-app.test.tsx test/client.test.ts`; `i12-focused-final.log` ends `=== focused ALL GREEN` with 11 tests passed.
- The first full attempt, `i12-gate-initial.log`, ends `STOPPED: pnpm --filter @ariadshift/web test rc=1`: the fresh worktree lacked built WASM, and an unrelated account jobs test also failed. After `just wasm-build`, the full web test passed on the next run, including that account test.
- `i12-gate-webkit-failed.log` records `just wasm-build`, `pnpm --filter @ariadshift/web test` (260 passed), `just web-ci` (355 tests passed, typecheck and production build passed), and `CI=1 pnpm --filter @ariadshift/web exec playwright test e2e/cancel.spec.ts --project=chromium --project=firefox` (both passed). Its final line is `STOPPED: WebKit gate failed`.
- `zsh /home/vchun/Codes/ariadshift-ops/webkit-e2e.sh /home/vchun/Codes/01-AriadShift-wt-i12-cancel` ran the whole WebKit suite. `/home/vchun/Codes/ariadshift-logs/webkit-01-AriadShift-wt-i12-cancel-173600.log` ends `STOPPED: webkit rc=1`: 27 passed, 22 skipped, and the cancel test failed on both attempts. Its error context shows the HTML conversion had completed and the Cancel button detached before Playwright clicked it. This does not establish that the staged DOCX change fixes the race.
- `just pandoc` succeeded in the initial gate. `typos` and `just lint-commits` have not run; the gate stopped at WebKit before reaching them.

## Next worker

1. Review the staged DOCX test change and run the new cancel spec on all three browsers, including WebKit through the project's container recipe, under the shared build lock and load guard. The normal WebKit script runs the whole suite and records its own log; inspect its final line because its shell exit status alone does not reliably indicate test failure.
2. If WebKit still misses Cancel, diagnose with its error context and make the real conversion long enough to exercise cancellation without substituting a fake conversion. Verify the second conversion uses a fresh worker.
3. Commit the validated test change, rerun affected checks on the exact resulting head, and run `typos` and `just lint-commits`. Record exact commands, logs, final lines, and head SHA. The previous `just web-ci` and web unit results apply only to the implementation head noted above, before the staged test change.
4. Update and copy this report to `/home/vchun/Codes/ariadshift-ops/reports/`. Do not push or open a PR; the integration review loop requires acceptance first.

Status: BLOCKED
