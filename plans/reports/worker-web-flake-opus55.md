# Worker report: flaky WebKit e2e "converts epub to docx through IR"

Branch: `fix/web-e2e-epub-docx-flake` (from `origin/dev` at `68ac304`).

## Result

The flake is a test timing assumption, not an app race. `pick()` in
`apps/web/e2e/heavy-routes.spec.ts` gave the file-name assertion the default
5 s, but on a fresh page that step also includes the converter engine's cold
boot. The fix waits for the engine's own ready signal before picking a file,
and makes `reset()` wait for the worker's answer. With the fix, 26 of 26
repeated WebKit runs passed before the coordinator paused the box. Without it,
8 of 25 failed under the same conditions.

## Cause and evidence

1. The failure is in `pick()`, not in the target select. The trace
   (`heavy-routes-converts-epub-to-docx-through-IR-webkit/trace.zip`) shows
   `expect(getByTestId('input-name')).toHaveText("document.epub")` started at
   160.74 s and failed at 165.78 s. The page snapshot shows the status "Reading
   document.epub on this device." and a disabled "Reading…" button. The call
   came from the first `pick` at `heavy-routes.spec.ts:151`.
2. The page was waiting for the engine. `handleFiles` in
   `apps/web/src/components/converter/converter-app.tsx` awaits `start()`, the
   worker `init`, before it sends `ingest`. Init means spawning the worker,
   downloading and compiling `ariad_wasm_bg.wasm` (6.6 MB), opening OPFS and
   warming cached engines (`apps/web/src/converter/worker.ts`).
3. The trace network log shows that boot. The worker chunk loaded at 160.55 s,
   about 1 s after the page load event (the idle preload). The
   `ariad_wasm_bg.wasm` request started at 161.19 s and took 4005 ms because
   four workers were converting in parallel. `/sw.js`, which `enableOffline()`
   registers only after init resolves, was requested at about 165.98 s. That
   is already past the assertion deadline, and `ingest` still had to run.
4. The baseline repeat run reproduced this. It used the unmodified spec,
   `--repeat-each=20 --retries=0` and 4 workers. 8 of the 25 tests that ran
   failed, and all 8 failed at `pick` line 17 in the first pick of a fresh page
   (spec lines 71, 88, 112, 135 and 151). Each failing trace shows the
   `ariad_wasm` download taking 2.0 to 3.4 s, with init resolving 2.7 to 4.5 s
   after that request started. No failure happened after a target was chosen
   or after a reset.

No user-facing race exists. Until the engine is ready, the UI announces
"Reading …" and disables the picker, and the target select renders only after
`ingest` returns. So the app does not change.

## Fix

The change is test-only and is in commit `1851f9c` in
`apps/web/e2e/heavy-routes.spec.ts`.

- `pick()` first asserts that `device-indicator` shows the storage badge
  (`/browser storage|In memory/`). The app renders that badge only once
  `init` has resolved, so it is the engine-ready signal. The file-name
  assertion keeps its default 5 s, which now covers only reading the file.
- `reset()` now waits for the status "Files were removed from this device.",
  which the app sets only after the worker answers `reset`. A worker past
  `RECYCLE_MEMORY_BYTES` is replaced at reset, so that answer can include a
  fresh boot. Before this change the next `pick` would absorb that boot.
- Both waits use `ENGINE_BOOT = { timeout: 60_000 }`. This deserves a note
  because the brief rules out longer timeouts. The failing assertion keeps its
  default timeout. The boot is a separate phase whose latency depends on load
  by design (a multi-megabyte wasm download and compile), the same class of
  work as the conversion step the spec already waits for with
  `{ timeout: 120_000 }`. It does not mask a race: the wait targets the state
  the UI exposes once the engine is ready. Every Playwright assertion has a
  bound, so an explicit budget for this phase is unavoidable.

`pnpm exec biome check apps/web/e2e/heavy-routes.spec.ts` passes (run in the
web-heavy worktree with the patch applied).

## Repeat-run results

Both runs used a copy of `webkit-e2e.sh` (the original is unchanged), container
`ashift-webkit-flake`, `CI=1`, `--project=webkit e2e/heavy-routes.spec.ts
--repeat-each=20 --retries=0`, against the existing build in
`~/Codes/01-AriadShift-wt-web-heavy`, with 4 workers and 240 tests scheduled.

| Run | Log | Completed | Passed | Failed |
| --- | --- | --- | --- | --- |
| Baseline (unmodified spec) | `~/Codes/ariadshift-logs/webkit-flake-baseline-221526.log` | 25 | 17 | 8 |
| Fixed spec | `~/Codes/ariadshift-logs/webkit-flake-fixed-222134.log` | 26 | 26 | 0 |

I stopped the baseline early once the failure was reproduced. The coordinator
stopped the fixed run at 26 tests when the machine was paused, so the full
240-test run is still outstanding. The 26 passing tests covered every WebKit
test in the spec at least twice, including both through-IR routes.

## Verify (after RESUME)

The fix patch is still applied, uncommitted, in
`~/Codes/01-AriadShift-wt-web-heavy/apps/web/e2e/heavy-routes.spec.ts`, so the
command can run as is. These are copies of the ops scripts with only the
container name, test arguments and output directory changed:

```sh
docker ps --filter name=ashift-webkit   # must be empty
zsh /tmp/claude-1000/-home-vchun-Codes-01-AriadShift-wt-web-flake/b0334857-fb3f-4f39-b3cd-e3691ebaba3b/scratchpad/flake-e2e.sh \
  ~/Codes/01-AriadShift-wt-web-heavy fixed
# expect 240 scheduled, 0 failed (WebKit skips the 2 Chromium-only tests)
```

If the scratchpad is gone, run the normal `webkit-e2e.sh` flow with the inner
command `pnpm exec playwright test --project=webkit e2e/heavy-routes.spec.ts
--repeat-each=20 --retries=0`. After that run, restore the web-heavy worktree
with `git -C ~/Codes/01-AriadShift-wt-web-heavy checkout --
apps/web/e2e/heavy-routes.spec.ts` and delete its untracked
`apps/web/test-results-flake-baseline/` and `apps/web/test-results-flake-fixed/`
directories. On this branch, `just ci` covers the spec.

## Open concerns

- The full 240-test fixed run was not completed because of the pause.
- The brief rules out longer timeouts. The coordinator should confirm that a
  separate, explicit boot budget on the engine-ready signal (with the original
  assertion left unchanged) is acceptable.
- The same cold-boot pattern exists elsewhere but did not flake here:
  `converter.spec.ts` `chooseFile()` asserts `input-name` with the default
  timeout. It is outside this branch's verified failure, so it is unchanged.
  It may need the same wait if it shows up in a WebKit run.

Status: DONE_WITH_CONCERNS
