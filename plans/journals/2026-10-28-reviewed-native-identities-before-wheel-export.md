---
title: Reviewed native identities before wheel export
date: 2026-10-28
summary: Bound native export bytes to reviewed components and bundled notices; the exact-head pack gate passed.
---

# Reviewed native identities before wheel export

## What happened

A renamed real GPL Mach-O object passed the prior export scanner because it
lacked a forbidden filename or marker. Output checksums identified bytes but
never established the component licence. Regressions on the untouched accepted
baseline produced 49 failures and five passes; the real diagnostic accepted six
neutral-name GPL variants there.

## Repair and evidence

The export scanner now hashes complete native payloads and requires exactly one
reviewed component, an allowed SPDX licence and all hash-verified notices inside
the artifact. Five already reviewed BSD OpenEXR/Imath objects have explicit
export receipts. Publication callers scan all final files, including sidecars.
Unknown, changed or ambiguously identified native objects fail closed.

Source commit f63cca840b82f279d108c34db70e1fd6a2c283e2 passed the complete scoped
pack gate through slot 2. The run passed 314 focused tests, 408 Docling tests
with four existing conditional skips, 75 bench tests and 135 reproducible
fixture outputs. Fresh Linux checks covered 805 native files with zero findings.
The real export probe refused 12 GPL variants and accepted five reviewed BSD
objects only with their complete notices. Lock, audit and lint checks passed.

The first detached launcher exited before runner entry. Its empty run was not
used as evidence; after reconciliation the same SHA was requeued and completed.
The final launcher waited for slot 2 without pausing any slot holder.

## Limits and next steps

The five BSD receipts approve individual reviewed objects, not a complete new
source-built toolchain. New native outputs require their own review and bundled
notice receipts before publication. Native macOS execution and an authoritative
video-disabled OpenCV pin remain pending. No push, tag, release or workflow
dispatch occurred. The canonical report and operations mirror carry the result;
report-only closure follows the tested source commit. AgentWiki publish skipped.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
