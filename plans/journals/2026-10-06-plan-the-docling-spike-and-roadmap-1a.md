---
title: Plan the Docling spike and roadmap 1a
date: 2026-10-06
summary: 13-phase plan for a Docling protocol spike plus v0.1 CLI+MCP; red team found 15 real defects before any code was written.
---

# Plan the Docling spike and roadmap 1a

## What happened

Wrote `plans/261006-0544-docling-spike-and-roadmap-1a/` (13 phases, about 152h): a time-boxed Docling spike through the draft `ariad-engine/1`, then roadmap 1a (MD↔DOCX/HTML/EPUB, planner over bench scores, `inspect`/`plan`/`engines`/`doctor`/`mcp`, fuzzing, a v0.1.0 release).

Research changed several assumptions:
- Docling 2.134.0 already ran offline on CPython 3.14 on this host.
  - TableFormer logs to stdout, which breaks JSON Lines.
  - A killed engine orphans its `tesseract` child.
  - There is no per-page progress API.
- cargo-fuzz runs on stable with `-s none`.
- dist 0.33.0 skips `publish = false` crates unless `dist = true` is set.
- winget-releaser cannot create a new package and is AGPL-3.0.

## Red team

39 raw findings were deduplicated into 15, all accepted. The ones that would have hurt most:
- **ZIP bombs.** The preflight trusted the declared ZIP sizes. Fix: copy the input first, then stream-inflate with a real byte count.
- **HTML nesting.** A depth cap in the html5ever `TreeSink` cannot bound the builder's quadratic scope walks. Fix: feed in chunks and stop at the cap.
- **Data loss.** A Markdown sidecar asset directory could lose data, and `md→md --overwrite` could replace the source.
- **Injection.** The link allow-list existed only in `pandoc/from_ir.rs`, so the native writers would have emitted `javascript:` links and raw `<script>`.
- **Memory limiter.** A host memory limiter needs `unsafe` (`ariad-host` forbids it), and `RLIMIT_AS` breaks GHC-based Pandoc.
- **Unbounded reads.** `ir_io::read` disables the recursion limit and has no byte cap.
- **Misleading spike.** `runner::run` clears the environment, which would have invalidated the spike's results.
- **Release.** The release version bump would break the `0.0.0` path dependencies.

## Decisions (user)

- Markdown images are embedded as `data:` URIs.
- The planner is built to the full ARCHITECTURE §6.3 spec.
- Raw HTML in HTML output is sanitized with ammonia 4.2.1.
- Bomb limits are finite (local 20k entries, 4 GiB decompressed, 6 GiB IR).
- Legacy HTML charsets go through `encoding_rs`.
- A nightly ASan fuzz job is the sole exception to the pre-release toolchain rule.
- The three crates are published to crates.io from CI.
- The winget id is `VChun.AriadShift`.
- Publishing waits at a stop in phase 13.

## Next steps

- The plan is uncommitted.
- Execute with herdr-cook-plan, starting with phases 1 and 2 in parallel.
- Research goes to agy workers on Gemini 3.8 Flash High, per the user's rule.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
