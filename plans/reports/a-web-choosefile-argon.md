# Worker report: engine-ready wait in converter.spec.ts chooseFile()

Branch: `fix/web-e2e-epub-docx-flake--choosefile` (from `origin/fix/web-e2e-epub-docx-flake` at `26333f4`).

## Summary and diff

Applied the same 60 s engine-ready wait (`ENGINE_BOOT = { timeout: 60_000 }`) on `device-indicator` (`/browser storage|In memory/`) from `1851f9c` (`apps/web/e2e/heavy-routes.spec.ts`) to `chooseFile()` in `apps/web/e2e/converter.spec.ts`. Because `apps/web/e2e/` has no existing shared helper file and `converter.spec.ts` picks via the `"Choose a file"` button + `filechooser` event whereas `heavy-routes.spec.ts` uses `locator("input[type=file]").setInputFiles(...)`, the minimal 4-line wait and `ENGINE_BOOT` constant were duplicated directly in `apps/web/e2e/converter.spec.ts`.

All test assertions, timeouts on `input-name`, and product code are unchanged. No retries, `test.slow()`, or skips were added.

Diff in `apps/web/e2e/converter.spec.ts` (`0e6cd8d`):

```ts
const ENGINE_BOOT = { timeout: 60_000 };
```

```ts
async function chooseFile(page: Page, name: string, mimeType: string, content: string | Buffer) {
  await expect(page.getByTestId("device-indicator")).toContainText(
    /browser storage|In memory/,
    ENGINE_BOOT,
  );
  const chooser = page.waitForEvent("filechooser");
  await page.getByRole("button", { name: "Choose a file" }).click();
  await (await chooser).setFiles({ name, mimeType, buffer: Buffer.from(content) });
  await expect(page.getByTestId("input-name")).toHaveText(name);
}
```

## Other fresh-page pick sites in converter.spec.ts

1. **Sites using `chooseFile()` on a fresh page (now covered by the engine-ready wait):**
   - Line 60 (`converts Markdown to HTML on the device`)
   - Line 75 (`converts files with backslashes in name and retains the download name`)
   - Line 92 (`converts HTML to Markdown`)
   - Line 103 (`round-trips Markdown through IR JSON`; also calls `chooseFile()` after reset at line 111)
   - Line 122 (`preselects the target from a format-pair link`)
   - Line 128 (`offers the desktop app for legacy Office input`)
   - Line 160 (`file drops outside the picker preserve the current document`; also has a direct `expect(page.getByTestId("device-indicator")).toContainText(/storage|In memory/)` assertion with the default 5 s timeout at line 139 before `chooseFile()`)
   - Line 192 (`reset deletes the stored files`)
   - Lines 210–211 (`two tabs keep their files apart`, on both `first` and `second` fresh pages)
   - Line 236 (`files of a closed tab are deleted when the converter starts again`, on `closing`; line 242 also asserts `next.getByTestId("device-indicator")` contains `"browser storage"` with the default 5 s timeout on a fresh page without picking a file)
   - Line 289 (`the target and profile selects are keyboard operable`)

2. **Direct fresh-page file-pick sites that bypass `chooseFile()` (unchanged per brief):**
   - Lines 246–261 (`refuses files above the browser limit`): opens `/app`, clicks `"Choose a file"`, passes a 17 MB buffer via `(await chooser).setFiles(...)`, and asserts `expect(page.getByTestId("converter-error")).toContainText("up to 16 MB")` with the default 5 s timeout. Because `handleFiles` in `apps/web/src/components/converter/converter-app.tsx` awaits `start()` (worker init) before sending `{ type: "ingest", file }` (where `apps/web/src/converter/session.ts` enforces `inputLimitBytes`), a cold worker boot on a fresh page is included inside the 5 s `converter-error` assertion budget.
   - Lines 263–275 (`offers a compatible Desktop route for oversized IR JSON`): navigates to `/app?to=docx&profile=private`, sets a 17 MB buffer directly on `page.locator("input[type=file]").setInputFiles(...)`, and asserts `expect(page.getByTestId("converter-error")).toContainText("up to 16 MB")` with the default 5 s timeout. This hits the same `await start()` before `ingest` path on a fresh page.
   - Lines 277–285 (`the file picker is keyboard operable`): opens the file chooser with `Enter` (`await chooser;`) on a fresh page, but does not call `setFiles(...)`, so `handleFiles` and worker boot are not triggered.

## Commits

- `0e6cd8d` (`0e6cd8d7d7121618ab8003d599a9dbc767936d6d`): `test(web): wait for the engine before the first file pick`

## Commands run and verification

No build or test was run (build-free task per brief).

- `git status && git branch -vv && git show 1851f9c` — inspected branch state and `1851f9c`.
- `rg "up to 16 MB|limit_exceeded|inputLimitBytes" apps/ packages/` — verified that `inputLimitBytes` is checked inside the worker `ingest` handler after `await start()`.
- `typos apps/web/e2e/converter.spec.ts plans/reports/a-web-choosefile-argon.md` — exit 0.
- `sh scripts/check-commits.sh origin/fix/web-e2e-epub-docx-flake..HEAD` — exit 0.

## Exact command for the later 20x WebKit run

Run one `ashift-webkit-e2e` container at a time under the global build lock. In `~/Codes/ariadshift-ops/webkit-inner.sh`, edit line 6 so the Playwright invocation includes both specs and the 20x repeat flags:

```sh
pnpm exec playwright test --project=webkit e2e/converter.spec.ts e2e/heavy-routes.spec.ts --repeat-each=20 --retries=0
```

Then run from the target worktree (once built):

```sh
docker ps --filter name=ashift-webkit
flock ~/Codes/01-AriadShift/.run/build.lock nice -n 19 zsh ~/Codes/ariadshift-ops/webkit-e2e.sh <worktree>
```

## Open concerns

- In `apps/web/e2e/converter.spec.ts`, the two oversized-input tests (`refuses files above the browser limit` at lines 246–261 and `offers a compatible Desktop route for oversized IR JSON` at lines 263–275) pick files directly without `chooseFile()` and still await `start()` before the worker rejects with `limit_exceeded`, plus lines 139 and 242 assert `device-indicator` on fresh pages with the default 5 s timeout. If those flake under parallel WebKit load, they will need the same `ENGINE_BOOT` wait on `device-indicator`.

Status: DONE
