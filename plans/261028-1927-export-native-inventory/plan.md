---
title: Reviewed native identities at the toolchain export boundary
status: in-progress
branch: fix/pack-prebuild-gates
created: 2026-10-28
---

# Reviewed native identities at the toolchain export boundary

Outcome: every detected native payload in an exported wheel or sidecar must
match one reviewed component by SHA-256, with an allowed licence and complete,
hash-verified notice bundled in the wheel/artifact. Unknown or altered bytes
fail even under neutral filenames. Video/Intel/GPL checks remain independent.
No waiver, stripping, push, tag, dispatch or release is authorized.

Root cause: `_scan_stream` previously returned success for native bytes with no
forbidden basename or sentinel; the export caller never required positive
component identification. The installed-pack inventory is a separate boundary.
A renamed real GPL Mach-O object therefore reaches export without a native
licence identity, while its generated output checksum proves only its bytes.

Design: reuse the named native-component inventory, adding explicit export
object hash receipts for genuinely reviewed bytes. Stream hashes across the
complete native payload; require exactly one matching component and its reviewed
licence/notices. Export notices must be bundled, not merely available in the
source checkout. Never generate approvals from the candidate being audited.
Initially reviewed clean OpenEXR/Imath objects use the existing BSD reviews and
full upstream notices. New source-build outputs require their own reviewed
receipts before toolchain publication; macOS native validation remains pending.

- [x] Read the brief, review, source callers and existing policy/evidence.
- [x] Add and run red unknown-native/receipt/notice regressions and real renames.
- [x] Implement positive inventory enforcement and precise owning docs.
- [x] Run focused checks and review the complete delta.
- [ ] Commit source and run the scoped full pack gate at the exact head.
- [ ] Archive/update/mirror the canonical report and record remaining limits.
- [ ] Commit report records and verify clean closure without pushing.

Acceptance: unchanged real GPL dylibs are refused under original, neutral and
image-codec names; unidentified native payloads fail; reviewed clean real bytes
with complete notices pass; hash/licence/notice drift fails; all scoped tests,
real inventories, lock/audit, workflow lint, typos and commit lint pass.

Verification checkpoint: baseline `149e4cb` accepts all six real neutral-name
GPL variants; new regressions fail there with 49 failed and 5 passed. The fixed
working source passes 272 focused tests with four existing conditional skips.
The strict real-object probe refuses 12 GPL variants and five notice omissions;
all five reviewed real BSD objects pass with complete notices. Ruff, actionlint,
typos and whitespace checks pass. Both queued runners have exited.

The full gate runner is `/tmp/t7-rework10-final-pack-gate.sh`, using the owned
verification worktree and a source-head pointer written after commit. It removes
only the owned untracked baseline test copy before checkout. Report draft and
strict real-object probe are prepared under `/tmp` with `t7-rework10-` prefixes.
No candidate output checksum creates an approval; no publication is authorized.
