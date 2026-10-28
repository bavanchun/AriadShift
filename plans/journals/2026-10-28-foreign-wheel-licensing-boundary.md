---
title: Foreign wheel licensing boundary
date: 2026-10-28
summary: Five BSD dylibs reviewed; GPL-enabled macOS codecs remain refused.
---

# Foreign wheel licensing boundary

The native census expanded from the local Linux fixture to all six compatible
locked-wheel platform sets. It found 2,251 native files, including eight macOS
FFmpeg dylibs that declare GPL version 3 or later and --enable-gpl. Their bundled
LGPL notice conflicts with the binaries; x264, x265 and Rubber Band add GPL
candidates. No forbidden exception was introduced.

Five OpenEXR/Iex/IlmThread/Imath dylibs received exact owner, version and path
entries with complete hash-bound upstream BSD-3-Clause notices omitted by the
wheel. Nine regression cases failed before the repair; all ten pass after it.
The source commit is fc3997cf2a0f7360c37165b7fb8fce9e582e0ccb. The final local
gate and complete foreign inventory rerun are in progress at this checkpoint.

The all-platform success condition remains blocked by incompatible input bytes
and incomplete native-library reviews. Replace the GPL-enabled OpenCV macOS
input, complete the remaining platform reviews, and repeat the gate. No push.
AgentWiki publish skipped.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
