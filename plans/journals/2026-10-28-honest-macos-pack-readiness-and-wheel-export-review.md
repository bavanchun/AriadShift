---
title: Honest macOS pack readiness and wheel export review
date: 2026-10-28
summary: Require explicit release omissions and reject video exports and reviewed wheel hash drift.
---

# Honest macOS pack readiness and wheel export review

## What happened

Pack CI scheduled unpinned macOS source builds and expected its receipt, while
export auditing accepted isolated GPL video libraries by name. Permissive native
reviews also lacked a guard against same-version locked-wheel hash replacement.

## Changes and evidence

A shared planner keeps both Linux targets, annotates missing macOS PR pins and
requires explicitly scoped release omissions. Assembly checks actual receipts
and writes only built targets. Export auditing rejects video family names and
nonfree flags while retaining installed-pack LGPL FFmpeg reviews. Ten permissive
components bind reviewed carrying-wheel hashes plus installed RECORD checks;
these are not separate reviewed hashes of every object.

The exact source head 149e4cb55dacc7fe1387a4d459bbd94c0a808129 ends ALL GREEN:
262 focused tests; 356 whole-engine tests passed, 4 existing conditional skips;
805 native Linux files, zero findings; three real GPL dylibs refused; 75 bench
tests and 135 twice-generated fixture files passed. Lock, audit, workflow lint,
Ruff, typos and commit lint passed. The canonical report holds the log and draft
master release request. AgentWiki publish skipped.

## Next steps

Review the local source and report commits. Native macOS validation and actual
release pinning remain prerequisites under a separate master GO. No push, tag,
workflow dispatch or release was performed.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
