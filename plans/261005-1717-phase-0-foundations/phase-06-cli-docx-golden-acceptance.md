---
phase: 6
title: "ashift convert, DOCX golden test, acceptance"
status: pending
priority: P1
effort: "9h"
dependencies: [3, 4, 5]
---

# Phase 6: `ashift convert`, DOCX golden test, acceptance

## Goal

Ship the first working route, `ashift convert <file.md> --to docx`, through the real pipeline:

1. core reader
2. host asset resolution
3. workspace
4. runner
5. `ashift __engine pandoc`
6. Pandoc
7. timestamp stamp
8. atomic promote

Lock the output with golden tests over every Phase-0 Markdown fixture, and test the CLI contract recorded in ARCHITECTURE §4. Close roadmap Phase 0 with `just ci` green on three OSes.

## Context links

- [ARCHITECTURE.md](../../ARCHITECTURE.md) §4 (CLI contract), §14, §17
- [Toolchain research](./research/researcher-01-toolchain-report.md) Q4
- Phases [3](./phase-03-fixture-suite.md), [4](./phase-04-core-ir-markdown.md), [5](./phase-05-engine-protocol-host.md)

## Key insights

- **One route.** `convert` resolves it from a fixed route table in `ariad-host`. The planner replaces the table in 1a, and the CLI surface stays the same.
- **Goldens compare pretty-printed XML parts, never zip bytes.**
  - The sorted zip entry list is snapshotted per fixture. Pandoc writes 16 entries for a plain document and 17 with one image (verified), so there is no shared constant.
  - Snapshotted as text: `word/document.xml`, `footnotes.xml`, `numbering.xml`, `docProps/core.xml` (dates fixed by `SOURCE_DATE_EPOCH`), the `_rels` files and `[Content_Types].xml`.
  - `styles.xml`, `theme1.xml`, `fontTable.xml` and `settings.xml` come from Pandoc's template, so only their SHA-256 is snapshotted. Media is snapshotted as hashes too.
- **Goldens are valid only for `PANDOC_GOLDEN_VERSION`.** The test asserts it first. The engine itself accepts any version in `PANDOC_SUPPORTED`.
- **Exit 4 is reachable with unlimited local defaults**, because `max_nesting_depth` is always finite. The CLI test uses a 65-level nested list.

## Requirements

Functional (the contract in ARCHITECTURE §4):

- **Command:** `ashift convert <INPUT> --to <FORMAT> [-o <OUTPUT>] [--overwrite]`.
  - Phase 0 supports `md`/`markdown` → `docx`.
  - The default output is `<input stem>.docx` next to the input.
- **Exit codes:**

  | Code | Meaning |
  |---|---|
  | 0 | ok |
  | 1 | conversion failed |
  | 2 | usage |
  | 3 | unsupported route (prints the supported route) |
  | 4 | limit exceeded |
  | 5 | tool missing or wrong version (prints a hint) |
  | 6 | destination exists |
  | 130 | interrupted |

- **Output streams:**
  - Warnings go to stderr as `warning[<code>]: <message>`.
  - Progress goes to stderr, only when stderr is a TTY.
  - stdout carries only the output path.
- **Ctrl-C** cancels the run: the engine tree is killed, the workspace is deleted, and the destination is left untouched.

Non-functional:
- No partial output on any failure.
- No document content in messages.

## Files

| Action | File | Size | Test impact |
|---|---|---|---|
| Create | `crates/ariad-host/src/convert.rs` | Route table and pipeline (Architecture of phase 5); `ConvertReport { output, warnings, elapsed }`; `ConvertError` kinds map 1:1 to exit codes | ~200 |
| Modify | `crates/ariad-host/src/lib.rs` | Export `convert` | — |
| Create | `crates/ariad-cli/src/commands/convert.rs` + modify `main.rs`, `commands/mod.rs` | clap command, Ctrl-C wiring, exit codes, stderr rendering | ~180 |
| Modify | `crates/ariad-cli/Cargo.toml` | ariad-host, ariad-core, tokio (signal); dev: zip, quick-xml, sha2, hex, insta, jsonschema, tempfile, assert_cmd | — |
| Create | `crates/ariad-cli/tests/docx_golden.rs` | Goldens over the manifest fixtures with `routes ∋ "md->docx"` and `phase = "0"` | ~220 |
| Create | `crates/ariad-cli/tests/engine_conformance.rs` | Protocol conformance of `ashift __engine pandoc` | ~150 |
| Create | `crates/ariad-cli/tests/convert_cli.rs` | CLI contract tests | ~200 |
| Create | `fixtures/golden/<id>.docx.snap` | One combined snapshot per fixture | generated |
| Modify | `README.md` | Usage section | ~25 |
| Modify | `AGENTS.md` | Current state: MD → DOCX works; run `just pandoc` before tests | ~5 |
| Modify | `plans/…/plan.md` | Record Phase 0 completion and the manual check result (not ARCHITECTURE.md) | ~5 |

## Implementation steps

1. **`ariad-host::convert`.**
   1. Detect the format, then look up the route.
   2. Check the input size against `max_input_bytes` when it is set.
   3. Run `markdown::read`, then `assets::resolve(base_dir = parent or ".")`.
   4. Create the workspace and write the IR. The job id is a random 128-bit hex value.
   5. Run the engine through `runner::run(engine_program, ["__engine","pandoc"], request, cancel)`.
   6. On success, run `docx_meta::stamp`, then `promote(dest, overwrite)`. Always `close()` the workspace.
   - The engine program is a parameter; the CLI passes `current_exe()`.
2. **CLI `convert`.**
   - Install the Ctrl-C handler (tokio signal) that triggers the cancellation token.
   - Map errors to exit codes, and print one-line messages with hints, for example "run `just pandoc` or set ASHIFT_PANDOC".
3. **Golden test (`docx_golden.rs`).**
   1. Load the manifest through `tests/support`, select the fixtures, and **assert the count is ≥ 20**.
   2. Assert that `pandoc --version` equals `PANDOC_GOLDEN_VERSION`.
   3. For each fixture, run `ashift convert` (assert_cmd) into a temp dir with `SOURCE_DATE_EPOCH=1700000000` and `ASHIFT_PANDOC`.
   4. Build one combined snapshot: the sorted entry list, the pretty-printed XML parts (quick-xml, 2-space indent; text inside `w:t` is never re-flowed), hashes for the template parts, and media hashes.
   5. Add explicit assertions on the **raw** XML, so that a wrong golden cannot be blessed by accident:

      | Fixture | Assertion |
      |---|---|
      | NFD twin | `document.xml` contains NFC "Cộng hòa" and no U+0300–U+036F marks; its document XML equals that of the NFC twin |
      | CRLF twin | Document XML equals that of the LF twin |
      | Image | Exactly one `word/media/*` entry, hash equal to the companion PNG |
      | Table | Contains `w:tbl` |
      | Footnote | `footnotes.xml` contains the note text |
      | Math | Contains `m:oMath` |
      | Front matter | `core.xml` has `dc:title` and `dc:creator` from the YAML |
      | Emoji | Contains "😄" |
      | Raw HTML | Absent from the output, and a `raw_dropped` warning was printed |
      | All fixtures | No `w:instrText`, no `fldSimple`, no external relationship targets other than allowed link schemes (injection guard) |

4. **Conformance (`engine_conformance.rs`).**
   - Every stdout line validates against `schemas/engine-protocol.v1.json`.
   - Exactly one `result`, and it comes last. Artifact paths lie inside `output.dir`.
   - These requests produce `result ok:false`:

     | Request | Code |
     |---|---|
     | Invalid request | `invalid_request` |
     | Wrong protocol | `invalid_request` |
     | Missing input | `io` |

     The runner maps each one to `EngineFailed`.
5. **CLI tests (`convert_cli.rs`):**
   - output and exit status:
     - default output path; `-o`
     - a bare filename with `current_dir` set to the fixture's folder, so its image is embedded
     - stdout carries only the path
   - exit-code contract:
     - existing destination → exit 6, file untouched; a dangling symlink destination → exit 6 (Unix); `--overwrite` replaces it
     - unsupported route → 3
     - 65-level nesting → 4
     - missing Pandoc → 5 with the hint
   - warnings and interruption:
     - a remote image gives a warning and exit 0
     - Unix: SIGINT during a slow run (the probe engine via a test-only engine override env var `ARIAD_TEST_ENGINE`, read only in debug builds) gives exit 130, with no workspace and no destination left behind
6. **Bless and review.** Run `cargo insta review` and **read every snapshot before accepting it**: Vietnamese text, tables, footnotes, math, image, metadata.
7. **Acceptance.** Run `just ci` locally, push, and confirm all three OS jobs are green. Pay special attention to Windows: paths, the CRLF twin, and workspace cleanup.
8. **Manual check.** Open the vi kitchen-sink DOCX in LibreOffice (and Word if available). Record the result in plan.md.
9. **Docs.** Update README usage and AGENTS.md "Current state". Phase completion is recorded in plan.md, not ARCHITECTURE.md.

## Todo

- [ ] `ariad-host::convert` with route table and full pipeline
- [ ] CLI `convert` with Ctrl-C and exit-code contract
- [ ] DOCX goldens (per-fixture entry lists, raw-XML assertions, injection guard)
- [ ] Engine conformance tests
- [ ] CLI contract tests (incl. exit 4, 6, 130)
- [ ] Goldens reviewed and blessed
- [ ] `just ci` green on ubuntu-26.04, macos-26, windows-2025
- [ ] Manual LibreOffice/Word check recorded
- [ ] README, AGENTS.md, plan.md updated

## Test scenario matrix

| Priority | Scenario | Expected |
|---|---|---|
| Critical | Every Phase-0 Markdown fixture → DOCX on 3 OSes | Goldens match |
| Critical | NFD and CRLF twins | XML identical to their twins |
| Critical | Image fixture via a bare filename | Image embedded |
| Critical | Engine stdout lines | Schema-valid |
| Critical | Injection guard across fixtures | No field codes, no unexpected external targets |
| High | Destination exists or dangling symlink | Exit 6, untouched |
| High | Pandoc missing | Exit 5 + hint |
| High | 65-level nesting | Exit 4 |
| High | SIGINT (Unix) | Exit 130, clean |
| Medium | `html → docx` | Exit 3 |

## Success criteria (roadmap Phase 0 acceptance)

- `just ci` is green on all three OSes in GitHub Actions on the public repo.
- The MD → DOCX golden test passes for every Phase-0 Markdown fixture.
- The manual open check is recorded: correct Vietnamese text, tables, footnotes, math, image and metadata in LibreOffice/Word.

## Risk assessment

| Risk | Mitigation |
|---|---|
| A wrong golden gets blessed | Raw-XML assertions + human review |
| Pretty-printing changes significant whitespace | Pretty-print only for the snapshot view; assertions read the raw XML; `w:t` text is untouched |
| A test-only engine override leaks into release | It is read only under `cfg(debug_assertions)`, and a test asserts the release build ignores it |

## Security considerations

- Output is atomic and never overwrites silently, including through symlinks.
- Injection guard tests prove that no field codes or unexpected external targets reach the DOCX.
- Ctrl-C leaves no temp data behind.

## Next steps

Roadmap Phase 1a: the planner over `capabilities.json`, the remaining commands (reusing the §4 exit-code table), MD↔HTML/EPUB routes, Pandoc AST deserialization, and fuzzing seeded from `fixtures/md`.
