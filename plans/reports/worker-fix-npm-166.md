status: incomplete

The notice assertion now matches Next 16.4.3. The desktop picker failure remains
unexplained and was not changed. The full licence gate requires bundle analysis
that is absent locally; a production build was outside the light-build limit.

## Evidence and assumptions

- The cited CI run failed `opens the picker and starts conversion from the
  keyboard`: it called `inspect` for the selected file but never `enqueue`.
- The desktop app, its tests, and UI package match `origin/dev`; only the npm
  workspace and lockfile differ in the relevant paths. The desktop suite passed
  with the PR dependencies and again with the `origin/dev` workspace and
  lockfile. This comparison uses identical desktop source in this worktree; it
  does not reproduce the failure on either dependency graph.
- The picker test passed ten additional individual runs on the PR graph. A
  timing-dependent failure is possible but not proven; these passes do not
  establish whether a bumped dependency caused the CI failure.
- Web notice files are generated and ignored. `just web-licenses` regenerated
  them; the committed test assertion is not generated.
- Assumption: the local desktop comparison is representative of `origin/dev`
  for this test because the desktop source and UI package are identical.

## Checks

All pnpm and just commands ran serially through the shared build lock.

- PASS: frozen pnpm install for PR and dev dependency graphs, then PR restore.
- PASS: `just wasm-build`.
- PASS: `pnpm exec vitest run`: 56 files passed, 3 skipped; 693 tests passed,
  33 skipped. This includes the web notice and desktop picker tests.
- PASS: desktop suite on both dependency graphs: 25 tests each.
- PASS: the individual picker test repeated ten times on the PR graph.
- PASS: web and auth typechecks; `pnpm exec biome check` (264 files).
- PASS: `just web-licenses`; `git diff --check`.
- FAIL: `just licenses-check`: missing
  `apps/web/.next/diagnostics/analyze/history/history.json`; it requires a
  Next.js analyzed production build. Earlier attempts also hit a local mise
  pnpm shim configuration error. An unsupported Vitest `--repeat-each`
  attempt was replaced by ten separate invocations.

CI on the updated PR must still verify all platforms and the complete licence
check. Investigate the intermittent keyboard failure with a reproducible test
before claiming its cause or applying a fix.

Suggested PR title: `test(web): update Next license notice expectation`

Suggested PR body: Update the generated notice assertion for Next 16.4.3.
The picker failure seen in CI remains under investigation; it passed locally
on both dependency graphs. Re-run CI and the complete licence gate.
