# Seatbelt merge into D0

The license defect is fixed in separate commit `2756103f57a26a61fb10ee05b55f9e451c56081d`, the current gate head. Standalone license verification and the complete static stage are green on that head. The approved Seatbelt merge remains `8f6743ac6a0a8837aa2ea140c747bd116506938b`; its completed Rust, JavaScript, build and real database results remain valid because only the root notice and this report changed. The accepted polish baseline remains listed below. Nothing was pushed.

## Merge and conflicts

- Branch/worktree: `feat/v1-hardening--d0`, `/home/vchun/Codes/01-AriadShift-wt-ws-d0`.
- D0 parent: `d91229679b64985a792f1021a37372ced07734bb`. Its application source is the previously accepted D0 merge; the subsequent commits only updated its report.
- Seatbelt parent: `27d0f497002605692ed3b9ba0d453939bddc663a`, fetched and verified from `origin/feat/v1-hardening--seatbelt`.
- Commands: `git merge --no-ff --no-commit origin/feat/v1-hardening--seatbelt`, then `git commit -m 'chore: merge seatbelt hardening'`.
- **Conflicts: none.** All nine changed file blobs match the approved Seatbelt parent exactly. The diff against D0 is 700 insertions and 29 deletions, confined to the host sandbox/runner/probe code, its macOS tests, and `docs/cli.md`. Both parents are preserved; there was no rebase or amendment.
- Approval evidence: `/home/vchun/Codes/01-AriadShift/.run/mac-link/from-mac/APPROVED-seatbelt.md` and the adjacent `worker-seatbelt-261028-d3-d5-d2.md`. No additional macOS or Windows runtime verification is claimed here.

The notice repair uses `build: refresh third-party notices for the merged lock` and changes only `THIRD_PARTY_LICENSES` relative to its parent `6998f7b780cf1b68e82acf168c02672fc3f7df44`. The merge was not amended. This report is updated in a separate subsequent commit.

## Gate results and source identity

`git diff --stat 8f6743a 2756103f57a26a61fb10ee05b55f9e451c56081d` produced:

```text
 THIRD_PARTY_LICENSES                     |  3 +-
 plans/reports/d2120-d0-seatbelt-merge.md | 71 ++++++++++++++++++++++++++++++++
 2 files changed, 73 insertions(+), 1 deletion(-)
```

The complete path list was asserted to contain only those two files; the repair commit itself contains only the notice, with two insertions and one deletion. No Rust/JavaScript source, manifest, lockfile, test or fixture changed since `8f6743a`. Per the lead's decision, the server DB gate ALL GREEN and the other completed source/build checks are carried forward without rerunning them. Static and license checks below ran again on the exact new head `2756103f57a26a61fb10ee05b55f9e451c56081d`. This proof is also recorded in `ws-d0-seatbelt-notice-refresh.log`.

Heavy general commands used `gate-slot.sh 1`, niceness 19 and load below 12 at command start. The original queued PID `1507053` retained its queue position until it ended. Following the memory-pressure instruction, all later builds used `CARGO_BUILD_JOBS=2` and nextest used two threads; the earlier preliminary run used four compiler jobs. Tool pins were uv `0.12.23`, pnpm `12.9.1`, and the repository Node version. Rust debug information and incremental compilation were disabled.

| Exact command | Result |
|---|---|
| `just pandoc` | Passed; Pandoc 3.12. |
| `cargo fmt --all --check` | Passed. |
| `pnpm install --frozen-lockfile` | Passed. |
| `cargo build --locked -p ariad-cli --bin ashift --features ariad-host/test-probe` | Passed. |
| `cargo nextest run --locked -p ariad-host --features test-probe --lib --test runner --test session --test sandbox --test sandbox_macos --test-threads 2` | 212 passed, 0 skipped. |
| `cargo clippy --locked --workspace --all-targets --features ariad-host/test-probe -- -D warnings` | Passed. |
| `cargo nextest run --locked --workspace --exclude ariad-server --features ariad-host/test-probe --test-threads 2 --no-fail-fast` | 985 passed, 12 default skips. Both doctor snapshot tests passed. |
| `cargo test --locked --workspace --doc` | Passed; no runnable examples. |
| `just licenses-check` | Passed on `2756103`; root and desktop notices match their generators. |
| `just static` | Passed in full on `2756103`, including the license check and both following recipes. |
| `just test-winget-manifest`; `just test-ci-scope` | Passed independently on `8f6743a` and again within the green static stage on `2756103`. |
| `just wasm`; `just wasm-build` | Both passed. |
| `just bench-test`; `just bench-check` | 74 tests passed; data validation passed. |
| `just package-check` | Passed. |
| `pnpm --filter @ariadshift/sdk generate`; `pnpm --filter @ariadshift/sdk check` | Passed with no tracked generated drift. |
| `pnpm -r --if-present typecheck` | Passed. |
| `pnpm exec vitest run --maxWorkers=1` | 446 passed, 1 accepted polish baseline failure. Auth database tests executed. |
| `pnpm --filter @ariadshift/web build` | Passed; 242 pages generated. |
| `just desktop-bindings-check`; `pnpm --filter @ariadshift/desktop build`; `pnpm --filter @ariadshift/desktop test:tools` | Passed; 6 tooling tests passed. |
| `just test-server` | **156 passed, 0 skipped**, with real PostgreSQL and S3. |
| `just lint-commits f9ecfce..27d0f49`; `just lint-commits 8f6743ac6a0a8837aa2ea140c747bd116506938b^!`; `just lint-commits 6998f7b..2756103` | All passed. |

The server recipe ran plain, outside `gate-slot.sh`, after the general slot was released. This checkout's recipe has no internal flock, so its launcher held only `build.lock.2`, at niceness 19, with two compiler jobs and two nextest threads. Command-start load was 7.00. PostgreSQL used `127.0.0.1:55442`, and S3 used `127.0.0.1:58342`. The launcher required a real database execution marker, preventing the recipe's Docker-unavailable skip from counting as success. Backups were taken before tests and disposal. The auth Vitest database used a separate throwaway PostgreSQL on port `55490`. All owned test containers, volumes and temporary backup directories were removed.

## Resolved issues and remaining limits

**License defect fixed.** D0 integrated the dev lock containing `sse-stream 0.2.6` without regenerating the root notice. This was an integration defect, not a waived baseline. Following the lead's repair decision, `just licenses` regenerated the notices through the repository recipe. Verification allowed exactly the Apache component count change from 590 to 591 and the missing `sse-stream 0.2.6` entry already required by `Cargo.lock`; desktop notices were unchanged. The repair was committed separately, then `just licenses-check` and `just static` both passed on that commit. All three stages used `gate-slot.sh 1`, two compiler jobs and niceness 19.

**Accepted polish baseline.** Root Vitest ran the entire desktop polish file: 14 passed and the notice-expiry assertion at `apps/desktop/test/polish.test.tsx:247` failed. Its pre-existing status was accepted in the D0 review; comparison evidence is in [the D0 report](d2120-d0-merge-dev.md). The lead reports a fix on the separate desktop branch. That fix is not part of this Seatbelt merge, and no test was edited here.

**Doctor retry record.** The preliminary workspace run stopped at a snapshot difference reporting four stale workspaces in the shared temporary directory. A single isolated retry used a fresh owned `TMPDIR`, after separate compilation, at load 11.18. It failed with CLI exit 5 before the snapshot assertion; its output does not identify the failed doctor check. A subsequent diagnostic CLI invocation passed every required check, and both doctor tests passed in the full 985-test run with a private temporary directory. The lead classified the isolated retry as a `TMPDIR` artifact. Its failed log remains preserved; no snapshot was accepted and no other session's temporary files were removed.

The historical general rerun's static stage initially stopped on a spelling error in this report draft. It was corrected, and the complete static stage now passes on the repaired head. Historical failed logs remain unchanged. Release-workflow mutation tests recorded 78 passed and two PowerShell syntax checks skipped because `pwsh` is unavailable. Native nextest retained 12 default skips, including optional Docling pack/engine and AppImage checks. Linux cannot execute the macOS Seatbelt runtime tests; the approved Mac evidence is linked above.

## Logs

All logs are retained under `/home/vchun/Codes/ariadshift-logs/`. New green results are listed first. Historical failed command statuses remain preserved.

| Log | Ending/result |
|---|---|
| `ws-d0-seatbelt-notice-gate.log` | `=== SEATBELT NOTICE GATE ALL GREEN head=2756103f57a26a61fb10ee05b55f9e451c56081d inherited_server_head=8f6743ac6a0a8837aa2ea140c747bd116506938b accepted_desktop_polish_baseline=recorded`. |
| `ws-d0-seatbelt-notice-refresh.log` | `=== NOTICE REGENERATION ALL GREEN head=2756103f57a26a61fb10ee05b55f9e451c56081d`; includes the exact generated diff, separate commit and source-identity proof. |
| `ws-d0-seatbelt-license-notices.log` | `=== NOTICE licenses GATE ALL GREEN head=2756103f57a26a61fb10ee05b55f9e451c56081d`. |
| `ws-d0-seatbelt-static-notices.log` | `=== NOTICE static GATE ALL GREEN head=2756103f57a26a61fb10ee05b55f9e451c56081d`. |
| `ws-d0-seatbelt-general.log` | Preliminary run: snapshot and license failures; `STOPPED`, two failed checks. |
| `ws-d0-seatbelt-doctor-retry.log` | `STOPPED: doctor retry head=8f6743ac6a0a8837aa2ea140c747bd116506938b rc=100`. |
| `ws-d0-seatbelt-doctor-diagnostic.log` | `=== Doctor diagnostic exit=0`. |
| `ws-d0-seatbelt-general-final.log` | `STOPPED: general gate head=8f6743ac6a0a8837aa2ea140c747bd116506938b rc=0 failed_checks=1`; draft spelling failure subsequently resolved by the final static run. |
| `ws-d0-seatbelt-js-final.log` | `STOPPED: JavaScript tests head=8f6743ac6a0a8837aa2ea140c747bd116506938b rc=1`; only the accepted polish baseline failed. |
| `ws-d0-seatbelt-server-final.log` | `=== 2026-10-29T05:28:24+07:00 SERVER DB GATE ALL GREEN head=8f6743ac6a0a8837aa2ea140c747bd116506938b`. |
| `ws-d0-seatbelt-static-final.log` | `STOPPED: static gate head=8f6743ac6a0a8837aa2ea140c747bd116506938b static_rc=1 desktop_notice_rc=0`. |

No blocking issue remains. The accepted polish baseline and the platform/default-skip limits above remain recorded. Both repair and report stay local; no push was performed.

Status: DONE_WITH_CONCERNS
