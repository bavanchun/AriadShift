---
phase: 9
title: "inspect, plan, engines, doctor"
status: pending
priority: P1
effort: "10h"
dependencies: [7]
---

# Phase 9: `inspect`, `plan`, `engines`, `doctor`

## Goal

Ship the remaining 1a CLI commands with human output and stable `--json` output, reusing the planner, the readers and the `describe` op. The command logic lives in `ariad-host`, so the MCP server in phase 10 calls the same functions (Principle 1: no conversion logic in surfaces).

## Context links

- ARCHITECTURE §4 (commands, exit codes, stderr/stdout rules), §6.3 (the `ashift plan` output format)
- Phases 3 (`describe`), 6 (readers, executor), 7 (planner, embedded capabilities)
- `crates/ariad-cli/src/main.rs`, `crates/ariad-cli/tests/cli_smoke.rs`

## Scout first

Read `main.rs` and the phase 7 additions; list the existing clap structure and exit code mapping. Check that `ConvertError::exit_code` is the single source of exit codes and extend it rather than adding a parallel table.

## Requirements

Format detection, split so that core stays I/O-free:
- `ariad-core::format::classify(prefix: &[u8], zip_entries: Option<&[&str]>, extension: Option<&str>) -> Option<Format>`.
- The host reads the first 1 KiB and, for a ZIP (`PK\x03\x04`), the central directory entry names with the `zip` crate. A prefix sniff alone cannot find `word/document.xml`: in our own fixtures it sits about 15 KB into the file (entry 9 of 19 in `vi-styled-report.docx`).
- Rules:
  - a ZIP whose first entry is `mimetype` with content `application/epub+zip` is EPUB;
  - a ZIP with `[Content_Types].xml` and `word/document.xml` is DOCX;
  - `%PDF-` is PDF (`Format::Pdf` comes from phase 3);
  - a leading `<!doctype html` or `<html`, ignoring case and after a BOM or whitespace, is HTML;
  - otherwise the extension decides.
- Tests include a large DOCX whose `word/document.xml` sits after several MB of media.

`convert` uses the classifier, and a mismatch between the extension and the content gives a warning.

Commands (logic in `ariad-host::commands::*`, rendering in `ariad-cli`):

| Command | Behaviour | JSON shape |
|---|---|---|
| `ashift inspect <INPUT> [--json]` | Format (sniffed), bytes, metadata, counts (headings by level, paragraphs, tables, images, links, footnotes, words), languages from metadata, warnings from reading, and the targets the planner reaches. PDF: format, bytes, page count from the trailer only if trivially available (else omitted), and "requires the docling engine (roadmap 1b)"; exit 0. Unknown format: exit 3 | `{format, bytes, meta, counts, warnings[], reachable[]}` |
| `ashift plan <INPUT> --to <FMT> [--profile P] [--json]` | The ARCHITECTURE §6.3 layout: `input`, `route`, `score` (fidelity, editability, estimated time from `p50_ms`, "runs locally ✓"), `alt`; unmeasured edges are marked. No conversion happens | `{input, route[], score, measured, alternatives[]}` |
| `ashift engines [--json]` | One row per engine: id, version, license, status (`ready`, `missing`, `wrong version`, `not installed`), the routes it serves, all from phase 7's single `availability()` source. `ariad-core` is native. `docling` shows "not installed (available in v0.2)" | `[{id, version, license, status, routes[]}]` |
| `ashift doctor [--json]` | Checks with ✓/✗ and a fix hint each: Pandoc found and in `>=3.12,<4`; the temp directory is writable and a workspace can be created and removed; the Pandoc engine passes a `describe` round trip. Exit 0 when all required checks pass, 5 when a required tool is missing | `{checks: [{id, ok, required, message, hint}]}` |
| `ashift convert … --json` | Prints the report as JSON instead of the bare path: output, route, warnings, elapsed | `{output, route[], warnings[], elapsed_ms}` |

Rules:
- Human output goes to stdout, warnings and progress to stderr (unchanged). `--json` prints exactly one JSON document to stdout.
- JSON shapes are serde types (with `schemars::JsonSchema`, so phase 10's MCP tools derive their output schemas from the same types). No committed `schemas/cli/*` files in 1a; the snapshot tests pin the shapes.
- No full filesystem paths of tools appear in JSON (`describe` already reports status, not paths). `doctor` human output may show the Pandoc path, because it is local and is the hint the user needs.
- `docs/cli.md` documents every command, flag, exit code and JSON shape. The README links to it.
- Manifests: `crates/ariad-cli/Cargo.toml` moves `serde_json` from dev-dependencies to dependencies; `crates/ariad-host/Cargo.toml` adds `schemars`. Both are already in `Cargo.lock`, so the lockfile must not change; if it does, record why.

## Files

- Modify: `crates/ariad-core/src/format.rs` (`classify`; the variants come from phase 3)
- Create: `crates/ariad-host/src/commands/{mod.rs,inspect.rs,plan.rs,engines.rs,doctor.rs}`; modify `crates/ariad-host/src/lib.rs`
- Modify: `crates/ariad-cli/src/main.rs`; create `crates/ariad-cli/src/render.rs`
- Modify: `crates/ariad-cli/Cargo.toml`, `crates/ariad-host/Cargo.toml` (this phase owns them in its wave; phase 12 now waits for this phase)
- Create: `crates/ariad-cli/tests/commands_cli.rs` (insta snapshots of human and JSON output with versions and paths redacted)
- Create: `docs/cli.md`; modify `README.md`, `AGENTS.md` ("Current state"), `ARCHITECTURE.md` §4

## Implementation steps

1. The host-side reader plus the core `classify`, with tests (truncated ZIPs, BOM HTML, misnamed files, a DOCX with the document entry after large media). Commit.
2. `inspect` (host + CLI + snapshots). Commit.
3. `plan`. Commit.
4. `engines`. Commit.
5. `doctor`, including a test with `ASHIFT_PANDOC` pointing at a missing file (exit 5, hint). Commit.
6. `convert --json`, docs. Commit. `just ci`; three-OS CI.

## Success criteria

- Each command works on three OSes, and the snapshot tests cover human and JSON output.
- `ashift plan fixtures/docx/vi-styled-report.docx --to md` prints a route with measured scores (once phase 8 has landed, otherwise "unmeasured").
- `docs/cli.md` covers every command and flag shown by `--help` (reviewed by the coordinator).

## Risk assessment

| Risk | Signal | Response |
|---|---|---|
| Phase 8 lands after this phase | Plan output shows "unmeasured" in snapshots | Snapshots use a fixed test capabilities file, not the embedded one, so they do not depend on phase 8 |
| Windows console cannot print ✓/✗ | Garbled output in Windows CI | Use `[ok]`/`[x]` when stdout is not a UTF-8 console; JSON unaffected |
