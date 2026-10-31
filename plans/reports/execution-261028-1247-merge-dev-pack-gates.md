# Merge dev and repair the Windows RECORD fixture

Outcome: bring the pack branch up to date with dev and repair every observed
Windows engine failure without weakening the production licence checks.

Constraints: merge without rebasing, preserve Pillow 13 and all upstream work,
run tests detached through a gate slot with nice 19, and do not push.
No new licensing exceptions, native-pack work or full Rust matrix are in scope.

Acceptance: reproduce the Windows path failure on Linux, pass the scoped pack
gate at the exact merged and repaired code head, report every red check and
the three Verify states at the inspected PR head, and commit the report alone.

- [x] Fetch and merge origin/dev; no conflicts, lock unchanged.
- [x] Read the complete Windows failure and Cloud Scope/Compose logs.
- [x] Reproduce Windows path serialization, normalize the test helper, verify.
- [x] Commit the repair and run the full pack gate at that exact head.
- [x] Refresh CI evidence and update the canonical report for its report-only commit.

Evidence and results belong in plans/reports/d2120-task7-pack-gates.md.

## Completed execution

Merge 6faaf5b retains dev 47afeb4 without conflicts; uv.lock remains unchanged
with Pillow 13. Repair 6bbba2e changes only two test files. The first Linux
attempt without a path-format assertion passed because metadata hides
nonexistent literal backslash paths. A probe established that platform
behavior. The strengthened fixture invariant fails the Windows-path case with
the original helper; the one-line normalization fixes it. All 22 pywin32 and
76 scope table cases pass, with no production licence check changed.

Detached gate PID 4140901 completed and exited through slot 1, nice 19, at
6bbba2e777122ff2e86c87ee1d5667cf3f06e5a8. Final log:
/home/vchun/Codes/ariadshift-logs/d2120-t7-merge-dev-final-gate.log
Last line: === PACK GATE ALL GREEN HEAD=6bbba2e777122ff2e86c87ee1d5667cf3f06e5a8

Results: 173 focused tests, 264 engine tests (4 skips), three locked wheel/uv
installations, two encoding probes, 256-file native inventory, lock/audit,
UTF-8/Ruff, the current CLI build, 75 bench tests, 135 generated fixtures in two
identical runs, typos and commit lint all pass. Latest inspected remote head
96b10b5 has three successful Verify jobs and only the three diagnosed red
checks. Report-only successor is validated separately. No push is authorized.

Status: DONE_WITH_CONCERNS
