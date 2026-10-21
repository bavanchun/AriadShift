---
phase: 3
title: "Engine protocol and IR revision"
status: completed
priority: P1
effort: "13h"
dependencies: [1, 2]
---

# Phase 3: Engine protocol and IR revision

## Goal

Apply the protocol, IR, limits and format-registry changes that the spike and the red-team review proved necessary, so that the 1a commands (`engines`, `doctor`, planner, MCP) and the 1b Docling engine build on the same contracts. This phase is the only one in the plan that edits `protocol.rs`, `ir/mod.rs`, `limits.rs`, `format.rs`'s variant list or the two existing schemas.

## Context links

- [Spike report](./reports/docling-spike-report.md): the authoritative list of protocol and IR changes. Re-read it before starting; this file lists the expected candidates plus the red-team items that do not depend on the spike.
- `crates/ariad-core/src/{protocol.rs,ir/mod.rs,format.rs,limits.rs}`, `crates/ariad-host/src/{runner.rs,ir_io.rs,engines/pandoc.rs,convert.rs}`, `crates/ariad-host/src/bin/engine-probe.rs`
- `schemas/engine-protocol.v1.json`, `schemas/ir.v0.json`, `crates/ariad-core/tests/schema_drift.rs` (`ARIAD_BLESS_SCHEMAS=1` regenerates)
- `crates/ariad-cli/tests/{engine_conformance.rs,pandoc_engine.rs}`, `crates/ariad-host/tests/{runner.rs,ir_io.rs}`
- `crates/ariad-core/src/reader/markdown.rs` (`prescan_nesting`: the pattern for an I/O-free depth pre-scan)

## Scout first

1. Re-read the spike report's "protocol changes" and "IR changes" lists.
2. Enumerate every construction and match site you will touch:
   - `rg -n "Request \{" crates`. Today these are `convert.rs:168`, runner tests, `pandoc_engine.rs:24`, `engine_conformance.rs:53` and `protocol.rs:167`.
   - `rg -n "request\.(input|output)" crates`, for example `runner.rs:179` and `pandoc.rs:115,187`.
   - `rg -n "Event::" crates`. The exhaustive match at `convert.rs:191-199` must handle any new variant.
   - `rg -n "Block::[A-Za-z]* \{" crates` (about 91 sites, 19 of them exhaustive patterns) if any field is added to `Block` variants.
   - `rg -n "Limits \{|fn validate" crates` for the new limit fields.

   List the counts in the phase report.

The list below is a forecast; the spike report overrides the spike-dependent items.

## Expected changes

Protocol (`ariad-engine/1`, still draft, so breaking changes are allowed before the 1b freeze):
1. **Op-tagged requests.** Replace the single `Request` struct with a request enum tagged by `op`:
   - `convert`: today's fields;
   - `describe`: only `protocol` and `job`.

   A describe request needs no workspace, input or output. Update every construction site and every `request.input` and `request.output` dereference listed by the scout. The runner's artifact confinement applies to `convert` only.
2. **`describe` op.** The engine answers with a new event `{"type":"capabilities", ...}`, then `result ok`. The event carries:
   - the engine id and version;
   - the tool name, its version, and its status `found | missing | wrong_version` (never a path);
   - the license;
   - the supported `(input, output)` format-id pairs;
   - `enforces_memory_limit`, a boolean;
   - an optional model inventory.

   The Pandoc engine implements it. `convert.rs` treats a `capabilities` event during a convert as a protocol violation.
3. **Engine configuration contract.** Implement what the spike's E0 decided. Expected: engine settings travel in request `options` (documented keys per engine). The runner keeps `env_clear()` and its allow-list, and adds only variables the spike proved necessary per OS, for example `USERPROFILE` and `APPDATA` on Windows, with a test.
4. **Stdout discipline.** The spec says an engine writes only protocol lines to stdout (the runner already enforces this). Add a conformance case with a probe mode that prints a stray line.
5. **Progress semantics.** Document `progress {stage, done, total}` units (pages for page-based engines). Add an optional `unit` field only if the spike shows that `done/total` is ambiguous.
6. **Memory rule (no host limiter in 1a).** Document that `max_memory_mb` is enforced by the engine, and that an engine which cannot enforce it reports `enforces_memory_limit: false` in `describe`. The host then refuses a request that sets `max_memory_mb` for that engine with `limit_exceeded` rather than silently ignoring it. The generic host-side limiter is deferred to 1b, using the spike's E3 findings. It conflicts with `#![forbid(unsafe_code)]` (`crates/ariad-host/src/lib.rs:2`), and Linux `RLIMIT_AS` breaks GHC-based Pandoc.
7. **Session mode.** Only if E6 showed it pays off: specify it as an optional extension gated by `describe`. Fix the field names here; the implementation waits for 1b.

Limits (`ariad-core::limits`, part of the protocol request):
1. **Archive limits.** `max_archive_entries` and `max_decompressed_bytes`. Defaults (validation): local 20,000 entries and 4 GiB; cloud 10,000 entries and 2 GiB. Phase 4's ZIP preflight and the 1b Office routes use them.
2. **IR JSON limit.** `max_ir_json_bytes`. Defaults (validation): local 6 GiB, cloud 3 GiB. It caps the engine→host IR file, which carries base64 assets.
3. Each new field is validated in `validate()` (no zero), tested in the defaults tests, and documented in ARCHITECTURE §11.2. These rows are finite locally on purpose, like nesting and blocks: they guard against bombs, not against large legitimate documents.

Bounded JSON reading:
- Add an I/O-free token-level depth pre-scan to `ariad-core`. It counts `[`/`{` outside strings in one pass with no recursion and returns a typed error past a given depth. It mirrors `prescan_nesting` in the Markdown reader.
- `ir_io::read` becomes bounded:
  - a byte cap of `max_ir_json_bytes` via `Read::take`, where the cap being reached is an error, not a truncation;
  - the depth pre-scan, with a JSON depth budget derived from `max_nesting_depth`, before deserializing;
  - then the existing deep deserializer;
  - then a post-parse block count against `max_blocks`.
- Every failure is a typed `limit_exceeded`, and dropping a deep tree can no longer overflow the stack, because depth is bounded before the tree is built.
- Phase 4 reuses the same pre-scan for Pandoc AST JSON. Phase 11's `ir_json` fuzz target asserts this contract.

Shared link policy:
- Move the link-scheme allow-list out of `pandoc/from_ir.rs:466-481` into `ariad_core::links` (pure move; `from_ir` calls it, and the DOCX goldens stay identical). Phases 4, 5 and 6 apply it in every reader and writer. Today the Markdown reader copies `link.url` verbatim (`reader/markdown.rs:363-364`), so a `javascript:` link would otherwise reach the native writers.

Format registry:
- Add `Format::Epub` (`epub`, `application/epub+zip`, `.epub`) and `Format::Pdf` (`pdf`, `application/pdf`, `.pdf`, with no reader in 1a; `inspect` needs it).
- `DoclingJson` is deferred to 1b, because no 1a code reads or writes it.
- One wire identity: every variant serializes as its `Format::id()` (`#[serde(rename = "...")]` per variant, for example `ariad-ir+json`), so the IR, `capabilities.json`, the protocol and CLI JSON all use the same string. The existing snapshots only contain `markdown`, which is unchanged; a test asserts that serde names equal `id()` for every variant.

IR (`ariad-ir/0`, additive):
- The rule for new fields is **omit when empty** (`#[serde(default, skip_serializing_if = ...)]`), recorded in ARCHITECTURE §6.2. Existing fields keep their current `null` serialization. Every snapshot must stay byte-identical.
- Prefer document-level additions over per-variant fields, so that the 91 `Block::` sites do not change:
  1. **Provenance.** Specify `LayoutIndex`: page geometry plus per-block positions keyed by a stable block path, in the shape the spike chose, with a top-left origin for everything. Mappers convert.
  2. **Furniture.** `Document.furniture: Vec<Block>` for page headers and footers, kept out of `body`.
  3. **Tables.** Cell `header` flags and `Table.footnotes`, only if the spike showed losses. These touch `Table`/`TableCell` construction sites only.
  4. Anything else the report marks *required before 1a commands*.

## Requirements

- Every change has a test: serde round-trip, schema regeneration, a conformance case, a runner test or an `ir_io` limit test (byte cap reached, depth over budget, block count over).
- `ARIAD_BLESS_SCHEMAS=1 cargo nextest run -p ariad-core --test schema_drift` regenerates both schemas, and the drift test passes without the variable afterwards.
- `fixtures/golden/*.ir.snap` and `*.docx.snap` are unchanged.
- ARCHITECTURE §6.2, §7 and §11.2 describe every new field, the op-tagged request, `describe`, the configuration contract, the memory rule and the bounded read. Add Decision Log rows for "Engine memory limits are engine-enforced in 0.x; host limiter in 1b" and "Format wire id equals the registry id". AGENTS.md is unchanged.

## Files

- Modify: `crates/ariad-core/src/{protocol.rs,ir/mod.rs,format.rs,limits.rs,lib.rs,pandoc/from_ir.rs}`; create `crates/ariad-core/src/{json_depth.rs,links.rs}`
- Modify: `crates/ariad-host/src/{runner.rs,ir_io.rs,convert.rs,engines/pandoc.rs}`, `crates/ariad-host/src/bin/engine-probe.rs`
- Modify: `crates/ariad-cli/src/main.rs` (only to route `describe`), `crates/ariad-cli/tests/{engine_conformance.rs,pandoc_engine.rs}`, `crates/ariad-host/tests/{runner.rs,ir_io.rs}`
- Modify: `schemas/engine-protocol.v1.json`, `schemas/ir.v0.json`, `ARCHITECTURE.md`
- No manifest or lockfile changes are expected. If one is needed, list it in the phase report.

## Implementation steps

1. Formats first (smallest, unblocks phases 4–5): `Epub`, `Pdf`, and the wire-id rename with its test. Commit `feat(core): register EPUB and PDF formats`. Then the `links` move. Commit `refactor(core): share the link-scheme allow-list`.
2. Limits fields + validation + defaults tests. Commit.
3. JSON depth pre-scan + bounded `ir_io::read` + tests. Commit `fix(host): bound IR JSON reads`.
4. Op-tagged request enum: update every site from the scout list in one commit, behaviour unchanged. Commit `refactor(core): tag engine requests by op`.
5. `describe` op, `capabilities` event, the Pandoc engine support and the conformance test. Commit.
6. The engine configuration contract (per E0), the stdout-discipline case and the probe mode. Commit.
7. The memory rule (the host refuses an unenforceable `max_memory_mb`). Commit.
8. IR additions with serde round-trip tests; regenerate the schema. Commit.
9. ARCHITECTURE. Commit `docs(architecture): record protocol and IR revisions`.
10. `just ci` locally; the coordinator pushes and confirms three-OS CI.

## Success criteria

- Every item in the spike report's change list is either implemented here with a test, or listed in ARCHITECTURE §7 as "decided, implemented in 1b" with its field names.
- `ashift __engine pandoc` answers a `describe` request with no workspace, reporting the Pandoc version, its status and the format pairs.
- `ir_io::read` rejects an over-cap file, an over-deep file and an over-count file with typed errors.
- CI is green on all three OSes, and every existing golden is byte-identical.

## Risk assessment

| Risk | Signal | Response |
|---|---|---|
| An IR change breaks existing snapshots | Any `.snap` diff | The new field is not skipped when empty; fix the serde attributes, never re-bless |
| The JSON depth budget rejects legitimate IR at nesting 64 | `ir_io` test at depth 64 fails | Derive the budget from the measured JSON levels per IR level (2–4), with margin; keep the existing depth-64 end-to-end test |
| The op-tagged enum ripples into more files than listed | Compile errors outside the scout list | Update them in the same refactor commit and record them; the change is mechanical |

## Security

- `describe` reports tool status and version only, never paths, so MCP clients cannot learn the user's directory layout.
- Archive and IR limits are finite by default, unlike the workload limits, because they guard against bombs.
