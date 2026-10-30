---
title: Complete Linux native reviews and resolve OpenCV pack inputs
status: in-progress
priority: P1
effort: large
branch: fix/pack-prebuild-gates
created: 2026-10-28
---

# Complete Linux native reviews and resolve OpenCV pack inputs

Outcome: complete every Linux native review using the approved named permissive,
unmodified shared weak-copyleft and GNU-runtime rules. Keep forbidden components
closed. Resolve OpenCV in the master's order: prove whether the shipped pipeline
can omit it, then inspect compatible macOS wheel binaries, then implement a
source-built video-disabled macOS input only if the first two routes fail.
No GPL waiver, wheel stripping, unsupported release decision or push is allowed.

Acceptance: complete Linux inventory with no unreviewed native file; exact
owner/path/licence/notices/source and required hashes; meaningful negative tests;
OpenCV runtime dependency proof and selected safe input; architecture/decision
updates; exact-head full pack gate and report-only successor with evidence.

- [x] Prove OpenCV dependency/extras and required runtime import paths without cv2.
- [x] Inspect compatible macOS candidates and select the first permitted clean route.
- [x] Classify all Linux native files and verify upstream notice/source evidence.
- [x] Add regression tests first, implement exact entries and owning docs.
- [x] Run both full Linux inventories and the available real CI artifact check.
- [x] Review the complete diff, commit source and run the exact-head pack gate.
- [x] Update the canonical report, mirror it and commit reports alone.

Starting head: c93481d9173bbee433c19130db7946d173779964.
Baseline evidence and the earlier macOS stop are preserved in
[the previous plan](../261028-1440-foreign-native-review/plan.md).

Implementation: Linux reviews committed at abc5f08. OpenCV cannot be omitted:
RapidOCR and TableFormer hard-import cv2. All 14 compatible macOS releases have
GPL binary declarations; implement the native, video-disabled source route.
The two initial macOS regressions fail before implementation. Native macOS
compilation/output pinning cannot be executed on this Linux host and must remain
an explicit release prerequisite, not fabricated evidence.

Final notice review found the common OpenCV sidecar still contained the
inapplicable x86-only IPP notice. The final vendor gate reproduced the refusal;
the established reviewed removal preserves every other section byte-for-byte.
A dedicated regression and the complete 70-test integrity suite pass. The final notice commit records the repair; the final gate restarts at that new source head.

A retained glibc notice used form-feed separators but its digest described the
normalized variant. A failing regression reproduced the mismatch; the digest
now matches the original retained bytes. All 14 upstream sidecar digests and
71 integrity tests pass. The amended final source head is 8b84f0a; final gate
starts from scratch, with no previous platform census result reused.

Final local gate: ALL GREEN at 8b84f0ab8282e82e165e41144dd8a70b229c2617. Both complete Linux production
inventories have zero findings (402/403 files, 200/201 named). All 205 focused
and 299 full Docling tests pass, with four platform skips. Lock/audit, workflow
lint, spelling, commit lint, 75 bench tests and all 135 reproducible fixtures
pass. Remote x86 CI still has no artifact at the recorded final check.

- [ ] Native macOS compilation/probes, actual output pin and complete macOS pack inventory.

Status remains in-progress because the last item needs a native macOS host.
The code/workflow route is implemented; no release-target fallback is selected.
The canonical report records the BLOCKED release validation and source gate log.
