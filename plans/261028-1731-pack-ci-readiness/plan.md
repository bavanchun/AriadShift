---
title: Honest pack CI readiness and wheel export checks
status: completed
branch: fix/pack-prebuild-gates
created: 2026-10-28
---

# Honest pack CI readiness and wheel export checks

Outcome: unpinned macOS pack builds are visibly omitted on pull requests;
release runs fail unless the omission is explicitly declared for that run/tag.
Both Linux targets remain mandatory. Manifests/checksums contain only built
targets. Exported toolchains reject video families even without markers; newly
reviewed permissive components reject locked-wheel hash drift. No tag, release,
dispatch, push, licence waiver or release-target drop is authorized here.

Root causes: CI unconditionally selects macOS and expects its build record;
native OpenCV falls back to compilation when its release pin is absent. The
export scanner lacks family-name refusals. Permissive component version/path
and installed RECORD checks do not detect same-version locked-wheel replacement.

Design: one small CI planner owns the matrix and explicit omissions, warnings
and step summaries. Tag exceptions are keyed by exact pack tag; manual exceptions
are explicit inputs. No exception is populated. Assembly requires every planned
record and writes checksums only for selected components. Release-pin validation
is shared with the downloader. Export name refusals retain the installed pack's
reviewed LGPL FFmpeg allowance. Component receipts bind reviewed wheel inputs,
not per-object reviewed hashes; weak-copyleft/GNU object bindings stay intact.

- [x] Read both briefs, review and owning contracts.
- [x] Add failing plan/release/assembly, export and lock-drift regressions.
- [x] Implement the policy, metadata guards and owning documentation.
- [x] Run focused tests/workflow lint and review the complete diff.
- [x] Commit source; run one full exact-head pack gate through gate-slot.sh.
- [x] Prepare the unsent master release request in the canonical report.
- [x] Commit reports alone, mirror and verify report closure.

Acceptance: PR omission is explicit/non-red; invalid configured pins fail;
release omissions require explicit macOS declarations, never Linux omissions;
missing/stale/unexpected artifacts fail; manifest/checksums match actual planned
builds; required negatives and the full gate pass at the recorded source head.
Native macOS build/pinning remains a separate release prerequisite.

Verified source: `149e4cb55dacc7fe1387a4d459bbd94c0a808129`. The full gate ends ALL GREEN;
the report closure records the unsent release request and the remaining native
macOS release prerequisite. No push, tag, dispatch or release was made.
