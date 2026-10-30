---
title: Review native libraries across locked pack platforms
status: in-progress
priority: P1
effort: medium
branch: fix/pack-prebuild-gates
created: 2026-10-28
---

# Review native libraries across locked pack platforms

Outcome: enumerate native libraries from all compatible locked platform wheels,
review every missing named library against its wheel notices, and keep refused
licences/static/modified payloads closed for a Q. Follow A-0100, A-0245 and
A-0700 exactly. No push or unapproved native/licence exception is in scope.

Acceptance: a complete owner/path/hash/upstream/licence table, one-pass missing
name discovery using the production gate on each foreign site, focused
notice/name regression tests, all permitted libraries accepted with full notices
and required source pointers, final exact-head scoped pack gate, and a report-only
successor identifying added libraries and any Q candidates.

- [x] Extract all six compatible locked platform sets and collect every discovery finding.
- [x] Verify OpenEXR/Imath upstream texts and the contradictory FFmpeg binary evidence.
- [x] Add ten regression cases, five exact dylib entries and owning docs.
- [x] Review and commit that safe change at fc3997cf2a0f7360c37165b7fb8fce9e582e0ccb.
- [ ] Complete licence/source reviews for every remaining native companion.
- [ ] Establish zero rejected native libraries on every platform.
- [x] Finish the final exact-head local gate and foreign inventory evidence.
- [x] Finalize the BLOCKED report and commit reports alone.

## Blocking evidence

The locked OpenCV macOS wheel contains eight FFmpeg dylibs whose own embedded
licence strings say GPL version 3 or later, with --enable-gpl --enable-version3.
They remain refused under the explicit stop rule. The wheel also bundles x264,
x265 and Rubber Band with upstream GPL licensing and no commercial entitlement
established. Its wrapper licence and bundled LGPL-2.1 FFmpeg text do not establish
permission to ship these GPL-enabled binaries. Other missing notice/source/name
reviews and platform-specific GNU byte bindings remain unresolved in the full
census. No blanket entry, GNU hash relaxation or licence waiver is added.

The five BSD additions supply upstream notices omitted by the wheel, using the
same existing explicit upstream-notice mechanism as Tokenizers. Ten focused
cases pass after nine fail before the named review. The baseline census contains
2,251 native files; the final census repeats all six sites at the committed head.

The requested all-platform green state is blocked. Required follow-up: replace
or rebuild the incompatible OpenCV macOS input without GPL codec payloads, then
review the remaining exact native files/notices and rerun all platform gates.
The current result and evidence live in
[the canonical report](../reports/d2120-task7-pack-gates.md).

Final evidence: the scoped local gate is ALL GREEN at the source head. The
complete final census has 947 rejected files and one Windows archive error; all
five added BSD objects pass. The two remaining acceptance items are blocked.
