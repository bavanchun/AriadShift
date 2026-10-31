# Hardening squash result

PR [#103](https://github.com/bavanchun/AriadShift/pull/103) remains open into
dev. The CI repairs are ready for the single authorized publication.
The selected macOS repair preserves warning denial: the two existing
library checks now run in the library test harness. No assertion was
removed, no linker lint was suppressed, and panic/unwind behavior is unchanged.

Verified source head: `d5c986a8dcc93c503e82b8a129d2100af1d6075d`.
The report/journal-only delivery commit and final published head are
recorded in the [publication receipt](/home/vchun/Codes/ariadshift-logs/hardening-squash-macos-publication.json).
The ordinary push and experiment deletion logs are linked there. The PR
is not merged, and auto-merge is disabled. The operations copy appends
publication metadata after the single push; no second report push is needed.

## Changes and validation

| Change | Cause and repair | Evidence |
| --- | --- | --- |
| `645529e` Windows startup test | A Unix-only file URL lacked a Windows drive. Construct a URL from a native temporary directory and keep its guard through both configuration checks. | 11 focused tests passed; the native-path constructor preserves Windows drive/escaping semantics. Native Windows confirmation awaits PR CI. |
| `645529e` CodeQL diagnostics | Configuration Debug output and a public signing-key ID reached assertion messages. Use static messages; preserve `!debug.contains(sentinel)` and `keys.ids().contains(&id)` exactly. | Redaction regression passed; locked `pack_manifest` target builds. No rule or production validation changed; no alert dismissed. New remote analysis remains required. |
| `064271a` Windows MSI | WiX rejected automatic GUIDs on file-bearing registry-keypath components. Use a fixed executable GUID and Tauri's supplied additional-binary GUIDs; keep HKCU keypaths and per-user scope. | Regression failed before repair, then 10 MSI/generator tests passed; full tool suite has 17 passing tests. Native MSI install/upgrade remains unproven locally. |
| `d5c986a` desktop tests | The new integration target unnecessarily built the full unoptimized app. Move both pure library checks to a `cfg(test)` module; only the crate import changes. | Exact content comparison, 29/29 Linux desktop tests, and native C results below. All tests and warning denial retained. |

The owning follow-up note is in [desktop development documentation](../../docs/desktop.md#development-and-checks):
reduce the ariad-desktop link size so the compact-unwind limit is not exceeded.
Unoptimized macOS app builds can still hit this existing limitation.
Installer builds continue to link the full app.

## What the native measurements established

Paired ordinary-app links corrected the earlier attribution. Dev was
already over the limit. The previous successful dev Verify runs did not
link the ordinary desktop executable because dev had no integration-test
target for this crate. The PR added one for assertions that only call
library APIs. [Cargo documents the automatic binary build](https://doc.rust-lang.org/cargo/reference/cargo-targets.html#integration-tests).

The previous report, written before paired measurements, is retained in
[the historical snapshot](/home/vchun/Codes/ariadshift-ops/reports/hardening-squash-before-macos-81522fb.md).
The new measurements supersede its claim that PR code first crossed the limit.

Both revisions in each pair used the same runner, SDK, Rust 1.99.0 and
warning policy. A linked executable and map were produced, then Cargo
returned 101 because the linker diagnostic was denied. The threshold is
16,777,216 bytes (16 MiB).

| Image / graph | Revision | App bytes | `__eh_frame` bytes | Cargo |
| --- | --- | --- | --- | --- |
| `20261026.0455.2`, standalone desktop | Dev `3c7f058` | 221,102,624 | 16,908,800 | 101 |
| Same runner and graph | PR `947ea93` | 221,361,904 | 16,929,056 | 101 |
| `20261013.0424.1`, host test-probe feature | Dev `3c7f058` | 221,121,648 | 16,909,456 | 101 |
| Same runner and graph | PR `947ea93` | 221,363,264 | 16,929,688 | 101 |

The PR adds about 20 KB of unwind data; dev already exceeds the limit by
more than 131 KB. Feature graphs differ between the two controls, so the
small cross-control differences are not an isolated image effect.

Largest PR live-symbol contributors in the standalone map are
`typst_library` (14,257,850 bytes), `ariad_desktop` (7,097,522),
`hayagriva` (5,547,863), `ariad_core` (4,247,988) and `ariad_host`
(3,886,974). These totals cover all live symbols, not unwind attribution
per crate. There is no accidental new dependency to remove. The avoidable
addition was the integration target for two pure library checks.

### Run and image record

Every image below comes from its job's Runner Image block. PR-source
experiment `947ea93eea852812ef17282d7637c294490003dd` starts at local
`81522fb57488732c8743a0ceb297edcc7980dbf2`. A changes no warning policy;
B adds only the approved macOS binary lint allowance at runtime.
C `e1c2d49531b88e50d1b2a6ff495864d0a7015df5` moves the tests with no allowance.

| Run / job | Variant | Image `macos-26-arm64` | Result |
| --- | --- | --- | --- |
| [41180636333 / 124477153832](https://github.com/bavanchun/AriadShift/actions/runs/41180636333/job/124477153832) | A Verify | `20261013.0424.1` | Original test build failed; paired app measurements and map upload passed. |
| [41180637297 / 124477156488](https://github.com/bavanchun/AriadShift/actions/runs/41180637297/job/124477156488) | A Desktop | `20261026.0455.2` | Same control failure; paired measurements and map upload passed. |
| [41180636333 / 124477153758](https://github.com/bavanchun/AriadShift/actions/runs/41180636333/job/124477153758) | B Verify | `20261026.0455.2` | Passed: 1,255 tests, 10 existing ignored, doctests and packaging. |
| [41180637297 / 124477156331](https://github.com/bavanchun/AriadShift/actions/runs/41180637297/job/124477156331) | B Desktop | `20261013.0424.1` | Passed: 25 UI, 17 tool, 29 Rust tests; installer/signature and packaged PDF to IR to Typst PDF. |
| [41181772827 / 124480474863](https://github.com/bavanchun/AriadShift/actions/runs/41181772827/job/124480474863) | C Verify | `20261026.0455.2` | Passed: 1,255 tests, 10 existing ignored, doctests and packaging; both relocated tests ran. |
| [41181772904 / 124480478724](https://github.com/bavanchun/AriadShift/actions/runs/41181772904/job/124480478724) | C Desktop | `20261026.0455.2` | Passed: 25 UI, 17 tool, 29 Rust tests; installer/signature and packaged PDF to IR to Typst PDF; no warning allowance. |

Historical dev controls are retained accurately:

| Run / job | Image `macos-26-arm64` | Meaning |
| --- | --- | --- |
| [41057851204 / 124069852778](https://github.com/bavanchun/AriadShift/actions/runs/41057851204/job/124069852778) | `20261013.0424.1` | Verify passed on the OLD image, with the old test graph. |
| [41060733754 / 124079599567](https://github.com/bavanchun/AriadShift/actions/runs/41060733754/job/124079599567) | `20261021.0445.8` | Verify passed on the requested new image, still with the old test graph. |

B satisfies the lead's conditional approval of the narrow lint exception.
The paired measurements identify a smaller cause-aligned repair: retain
the assertions in the unit harness and keep every warning denied.
C validates this preferred repair before publication. No compact-unwind
encoding flag, workspace warning relaxation or panic-strategy change is used.

## Ubuntu desktop smoke

The previous failure remains a transport rerun candidate. The summary
click target and its full input subtree match the earlier green job;
the smoke script is unchanged. Logs show a connection reset and
`RemoteDisconnected` while the app, WebKit and driver listeners remain.
No UI assertion establishes a deterministic PR defect.

| Run / job | Image | Result |
| --- | --- | --- |
| [40784863931 / 123155395189](https://github.com/bavanchun/AriadShift/actions/runs/40784863931/job/123155395189) | `ubuntu-26.04 20261025.179.1` | Prior smoke passed. |
| [41055230497 / 124060965411](https://github.com/bavanchun/AriadShift/actions/runs/41055230497/job/124060965411) | `ubuntu-26.04 20261025.179.1` | WebDriver transport failure. |
| [41055230497 / 124060965616](https://github.com/bavanchun/AriadShift/actions/runs/41055230497/job/124060965616) | `windows-2025-vs2026 20261021.278.1` | Independent MSI GUID authoring defect, repaired by `064271a`. |

Both Ubuntu jobs use WebKitGTK 2.52.6 and tauri-driver 2.1.0. No retries
were added. The ASUS environment lacks Xvfb, tauri-driver and WebKitWebDriver,
so no local AppImage WebDriver smoke is claimed. The new PR run is the
rerun candidate on the decided revision.

## Local gates and evidence

The detached ASUS gate used nice 19, four Cargo jobs, two nextest threads,
the prescribed tool pins and `gate-slot.sh 2` for every heavy command.
Runner: [hardening-squash-macos-final-gate.bash](/home/vchun/Codes/ariadshift-logs/hardening-squash-macos-final-gate.bash).
Log: [hardening-squash-macos-final-gate.log](/home/vchun/Codes/ariadshift-logs/hardening-squash-macos-final-gate.log).
It prints exact `d5c986a8dcc93c503e82b8a129d2100af1d6075d` at entry
and completion and ends:

```text
=== hardening-squash macOS final ALL GREEN
```

| Exact command | Result |
| --- | --- |
| `cargo nextest run --locked -p ariad-desktop` | 29 passed, zero skipped, including both relocated checks. |
| `cargo clippy --locked -p ariad-desktop --all-targets -- -D warnings` | Passed. |
| `just static` | Passed: formatting, typos, workflow guards, licences and repository static checks. |
| `just lint-commits origin/dev..HEAD`; `git diff --check` | Passed. |

Source integrity checks confirm that `main.rs`, manifests, lockfile and
permanent workflows are unchanged, and the Windows/CodeQL/MSI repairs
match the earlier combined gate. The final app/dependency tree exactly
matches native C `e1c2d49`. The two relocated tests match their original
contents except `use ariad_desktop` becoming `use crate`.

The final report/journal-only commit is checked with
`typos plans/reports/hardening-squash.md plans/journals/2026-10-31-native-desktop-link-measurements.md`,
`bash scripts/check-commits.sh origin/dev..HEAD` and `git diff --check`.
Its [delivery log](/home/vchun/Codes/ariadshift-logs/hardening-squash-macos-delivery-gate.log)
prints the delivery head at entry and completion and ends
`=== hardening macOS delivery ALL GREEN`.

Independent review found no correctness or regression blocker. The
macOS packaging logs also contain existing Cargo unused-patch and Actions
cache messages; their jobs completed successfully. They are not new Rust
code warnings or a warning-policy relaxation.

The original exact-squash gate at `65f20aa` ended
`=== hardening-squash ALL GREEN`: 234 real server tests, 12 auth database
tests, the full workspace, desktop/web builds and all 18 fuzz targets.
The subsequent combined source gate at `064271a` ended
`=== hardening-squash CI combined ALL GREEN`, including 1,262 workspace
tests with required disposable PostgreSQL/S3 services and desktop checks.
Those application and security fixes are unchanged by this test-only repair.
Twelve existing Linux ignored tests and two missing-PowerShell workflow
mutation controls remain documented; none of the moved checks is skipped.

Evidence directory: `/home/vchun/Codes/ariadshift-logs/`.

- `macos-link-experiment-results.json` and `macos-link-coverage-results.json`
  record every job, status and image; job logs have `.clean.log` copies.
- `macos-link-desktop-A-artifact/paired-results.json` and
  `macos-link-ci-A-artifact/paired-results.json` retain build exits,
  section sizes, binary hashes, full maps and crate summaries.
- `macos-link-experiment-validation-c-static-144200.log` and
  `macos-link-experiment-validation-c-nextest-144208.log` prove exact
  relocation and 29 passing tests before dispatch.
- The initial helper smoke failed because its workspace layout did not
  match the engine contract. Correcting it to per-leg `in/`, `out/`, `tmp/`
  made the real fixture conversion pass; both logs are preserved.
- The experiment worktree initially lacked the commit linter. The
  repository's already pinned tools were copied, then commit lint passed.
- Native and local gates are reviewed against their actual revision;
  the control A failure is not reported as a passing verification job.

The complete post-push `gh pr checks 103` table is retained in
[hardening-squash-macos-pr-checks.txt](/home/vchun/Codes/ariadshift-logs/hardening-squash-macos-pr-checks.txt).
These are the fresh PR checks; the experiment results above are separate
native evidence. The publication receipt records the check command's
exit code and timestamp. Pending PR checks do not imply a new failure.

## Remaining limits

Unoptimized macOS app links still exceed the compact-unwind offset range;
reducing link size remains the recorded v1.x follow-up. Native Windows
and new CodeQL results need confirmation from the single new PR run.
No Linux runtime proof of macOS Seatbelt or complete cloud WebKit coverage
is claimed. The earlier optional non-UTF-8 DB URL and ZIP name-byte
follow-ups and later security re-land's CSP note remain outside this task.

The experimental workflow/helper changes never enter the PR branch.
Their source is preserved in
[the experiment patch](/home/vchun/Codes/ariadshift-logs/macos-link-experiment.patch).
The publication receipt records deletion of the remote experiment branch
and removal of its local worktree/ref after evidence collection. The
working tree is clean, owned gate/monitor processes have exited, and no
foreign process or persistent database was changed.

Status: DONE_WITH_CONCERNS
