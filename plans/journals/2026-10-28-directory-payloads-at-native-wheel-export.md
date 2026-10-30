---
title: Directory payloads at native wheel export
date: 2026-10-28
summary: Closed the directory-entry bypass and passed the exact-head scoped pack gate.
---

# Directory payloads at native wheel export

## Cause and repair

ZIP directory entries carrying unchanged native bytes bypassed the export audit
because the duplicate list and scan loop both trusted a trailing slash. The
checker now validates every member name, refuses any directory payload and
still scans those bytes. Directory attributes, unsafe paths, duplicate/case
collisions and file/directory conflicts are checked. Empty genuine directories
remain allowed but cannot satisfy an expected OpenMP replacement.

## Evidence

The frozen baseline produced 27 failed and 57 passed tests and accepted all six
real GPL directory variants. The fixed source 4740b4d5a4be15d576deaccb16de042065707505 passed 84 export tests,
260 adjacent tests and the exact-head scoped pack gate: 344 focused tests,
438 whole-engine tests with four existing conditional skips, 75 bench tests,
135 reproducible fixture outputs and fresh Linux checks covering 805 native
files without findings. Six real GPL directory variants and twelve ordinary
renames were refused; five reviewed BSD objects passed only with full notices.
The first gate attempt stopped at OSV after a TLS connection failure; no test
failed. Completed checks were retained at the unchanged source head. The
continuation retried audit and used CARGO_BUILD_JOBS=2 and nextest threads 2.
Lock, audit and lint checks passed through slot 2 without nested locks or pauses.

## Limits

Native macOS validation and an actual video-disabled OpenCV pin remain pending.
New source-built native outputs still require explicit licence/hash/notice
reviews. No push, tag, release or dispatch occurred. The canonical report and
operations mirror retain the evidence. AgentWiki publish skipped.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
