---
title: Validate every exported wheel member before directory exclusion
status: in-progress
branch: fix/pack-prebuild-gates
created: 2026-10-28
---

# Validate every exported wheel member before directory exclusion

Outcome: exported wheels refuse directory entries containing any payload, and
all readable member content reaches the native scanner regardless of the name.
Only genuinely empty directories may be omitted from content scanning. Member
paths must be unambiguous, relative POSIX paths; duplicate/case-colliding names
and file/directory collisions fail. Native component/licence/notice receipts and
video denials remain required. No GPL waiver, stripping, push or release.

Cause: `ZipInfo.is_dir()` depends only on a trailing slash. Both the duplicate
list and scan loop excluded those entries without checking sizes or contents.
The publication caller correctly propagated the checker, so the repair belongs
inside `check_wheel_files`, not the workflow or installed-pack licence policy.

- [x] Read the brief/review, trace the scanner and inspect caller contracts.
- [ ] Add regressions and execute them on the unchanged accepted source.
- [x] Validate all paths and directory payloads; retain content-based reviews.
- [ ] Run focused checks, review the delta and commit the source.
- [ ] Run the complete scoped pack gate at the exact source head through slot 2.
- [ ] Archive/update/mirror the report, reconcile this plan and commit closure.

Acceptance: the unchanged, hash-verified x264/x265/Rubber Band objects are
refused under neutral and original trailing-slash names; unknown native bytes
are refused; empty genuine directories and reviewed ordinary BSD objects pass;
duplicate, case-colliding, absolute, backslash and traversal paths fail. All pack
tests, real native inventories, lock/audit, fixture reproduction and lint checks
pass at the final source head. Native macOS release prerequisites remain pending.

Execution checkpoint: detached PID 33804 waits for slot 2 at load below 12.
Its sequential runner is `/tmp/t7-rework11-red.sh` (also copied into the logs
directory as `d2120-t7-rework11-runner.sh`). It checks out the untouched baseline
in the owned verification worktree, copies only the new tests, runs expected-red
pytest plus the unchanged reviewer real-directory probe, then restores the test
copy. It runs `/tmp/t7-rework11-focused.sh` against this fixed worktree. If those
checks pass, it stages exactly the six planned files, creates the source commit,
writes `/tmp/t7-rework11-final-head`, and runs the complete final pack gate in
the same slot without nested locks. Do not create another runner or edit source
while it runs. Red, focused and final logs use `d2120-t7-rework11-` prefixes in
`/home/vchun/Codes/ariadshift-logs/`. The report writer and closure scripts are
prepared under `/tmp/t7-rework11-`; invoke the writer only after final ALL GREEN.
After final ALL GREEN, `/tmp/t7-rework11-finish.sh` checks the recorded counts,
updates/archives/mirrors the report, writes the local journal, commits Markdown
records and verifies closure. Any failed check stops the runner; it never pushes.
