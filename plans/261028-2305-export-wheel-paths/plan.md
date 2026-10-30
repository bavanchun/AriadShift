---
title: Validate every exported wheel member before directory exclusion
status: completed
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
- [x] Add regressions and execute them on the unchanged accepted source.
- [x] Validate all paths and directory payloads; retain content-based reviews.
- [x] Run focused checks, review the delta and commit the source.
- [x] Run the complete scoped pack gate at the exact source head through slot 2.
- [x] Archive/update/mirror the report, reconcile this plan and commit closure.

Acceptance: the unchanged, hash-verified x264/x265/Rubber Band objects are
refused under neutral and original trailing-slash names; unknown native bytes
are refused; empty genuine directories and reviewed ordinary BSD objects pass;
duplicate, case-colliding, absolute, backslash and traversal paths fail. All pack
tests, real native inventories, lock/audit, fixture reproduction and lint checks
pass at the final source head. Native macOS release prerequisites remain pending.

Completion: source `4740b4d5a4be15d576deaccb16de042065707505` passed the complete scoped slot-2 pack gate.
The final log ends `=== PACK GATE ALL GREEN HEAD=4740b4d5a4be15d576deaccb16de042065707505`.
The canonical report records red/green counts, real-object checks, fresh Linux
inventories and release limits. Markdown-only report closure is checked separately.
