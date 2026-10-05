---
phase: 5
title: "Engine protocol draft and ariad-host runner"
status: pending
priority: P1
effort: "16h"
dependencies: [4]
---

# Phase 5: Engine protocol draft and ariad-host runner

## Goal

This phase delivers five pieces:

- **The protocol.** Define `ariad-engine/1` (draft) as typed messages with a generated schema and an explicit outcome rule.
- **The runner.** Build an `ariad-host` runner that starts an engine and streams bounded NDJSON. It kills the whole process tree on every non-success path (timeout, error, Ctrl-C) and deletes the workspace afterwards.
- **Assets.** Resolve images on the host side, with lexical confinement checked before any filesystem call.
- **The Pandoc engine.** Implement it, reachable through the hidden `ashift __engine pandoc` subcommand.
- **The DOCX timestamp rewrite.** Rewrite the timestamps after Pandoc runs.

## Context links

- [ARCHITECTURE.md](../../ARCHITECTURE.md) §6.1, §7, §11 (contracts written in phase 1)
- [Toolchain research](./research/researcher-01-toolchain-report.md) Q4–Q7
- [Phase 4](./phase-04-core-ir-markdown.md): `Limits`, `Warning`, IR, `pandoc::from_ir`

## Key insights

- **Outcome rule** (ARCHITECTURE §7, phase 1):
  - `result ok:true` + exit 0 → `Success`.
  - `result ok:false` + any exit → `EngineFailed { code, message }`.
  - No `result`, or `ok:true` with a non-zero exit → `Crash`.
  - A line after `result` → `ProtocolViolation`.

  This keeps typed errors such as `tool_missing` and `limit_exceeded` intact all the way to CLI exit codes 4 and 5.
- **process-wrap 10.0.1 facts** (from its source):
  - `KillOnDrop` only kills the leader PID.
  - On Windows, `JobObject::wait()` can return on the first completion-port message.

  So the runner calls `start_kill()` explicitly on **every** non-success path and never relies on Drop. On Windows, the workspace is then removed with a bounded retry loop (up to 5 s, backoff) rather than in a silent Drop. A failed removal is reported as a warning, never swallowed.
- **Ctrl-C.** `ProcessGroup::leader()` moves the engine out of the terminal's foreground group, so Ctrl-C reaches only `ashift`. The runner takes a cancellation signal. The CLI wires `tokio::signal::ctrl_c` to it, kills the tree, closes the workspace and exits with 130.
- **Limits are optional** (local defaults are unlimited, by user decision):
  - `timeout_s: None` means no timer.
  - `max_memory_mb: None` means no `+RTS -M` flag.
  - The engine's internal margin uses `checked_sub`.
- **Workspace layout:** `in/`, `out/`, `tmp/`, `log/`.
  - The request carries `work_dir` (`tmp/`).
  - The host sets `TMPDIR`, `TMP` and `TEMP` to it for the engine, and the engine passes them to Pandoc. GHC reads `TMPDIR` on Unix.
  - The Pandoc log goes to `log/pandoc-log.json`.
- **Pandoc stdio inside the engine:**
  - stdin: piped, written from a separate thread.
  - stdout: `null`, so Pandoc can never corrupt the NDJSON stream.
  - stderr: piped with a concurrent bounded drain of 64 KiB.
- **Deep IR JSON.** serde_json stops at depth 128 by default, while one IR nesting level costs 2–4 JSON levels. Every IR read (engine and host) uses serde_json's `unbounded_depth` feature together with `serde_stacker` (pinned and verified). An end-to-end test at exactly `max_nesting_depth = 64` proves it.

## Requirements

Functional:

- **Protocol** (`ariad-core::protocol`):
  - `Request { protocol, job, op, input { path, format }, output { dir, format }, work_dir, options: Map, limits: Limits }`.
  - `Event`, tagged by `type`, has four variants: `progress`, `warning`, `artifact`, `result { ok, metrics?, error? }`.
  - Error codes form a closed enum: `invalid_request`, `unsupported_route`, `limit_exceeded`, `engine_failure`, `tool_missing`, `tool_version`, `io`.
  - Constant: `PROTOCOL = "ariad-engine/1"`.
  - Schema: `schemas/engine-protocol.v1.json` (`"x-status": "draft"`), covered by the drift test.
- **Runner** (`runner::run`):
  - Signature: `run(program, args, request, cancel, sink) -> Result<RunOutcome, RunError>`, where `RunError` is `Timeout | Cancelled | EngineFailed | Crash { exit, stderr_tail } | ProtocolViolation | Spawn | Io`.
  - The program path is a parameter. Production passes `current_exe()`; tests pass `engine-probe`.
- **Workspace:**
  - `Workspace::new()` creates `in/`, `out/`, `tmp/`, `log/` under the system temp dir.
  - `close()` removes it, retries on Windows, and reports failures.
  - `promote(artifact, dest, overwrite)`:
    - Write to `NamedTempFile::new_in(dest_dir)`, then `persist_noclobber`, or `persist` when `overwrite` is set. This is atomic on the same filesystem as the destination.
    - Check `dest` with `symlink_metadata`: an existing file or a symlink (even a dangling one) → `DestinationExists`, unless `overwrite` is set.
- **Assets** (`assets::resolve(&mut Document, base_dir, &Limits) -> Result<Vec<Warning>, AssetError>`):
  1. **base_dir.** An empty parent means `.`. Canonicalize `base_dir` once; failure is an error, not a warning.
  2. **Lexical validation (no filesystem calls).**
     - Percent-decode the href.
     - Reject any of: a scheme (`http:`, `file:`, `data:`, …), `\`, `:`, root, prefix or drive components, `..`, or empty segments.
     - Allow only `Component::Normal`.
     - A rejected href is left as `Url` with a warning that names the reason.
  3. **Open relative to the base directory handle** with `cap-std` (no escape, no symlink following outside the base).
     - Require a regular file: reject FIFOs, devices and directories.
     - Enforce `max_asset_bytes` when set.
     - Try the NFC form of the name, then the NFD form.
  4. **Sniff the magic bytes** (PNG, JPEG, GIF, WebP). Anything else is refused with a warning.
  5. **Store** the result as `Asset { id: sha256 }`.
- **Pandoc engine** (`engines::pandoc::serve(stdin, stdout) -> ExitCode`):
  1. Read and validate the request: protocol, `op = "convert"`, input `ariad-ir+json`, output `docx`.
  2. Read the IR with the deep-JSON reader.
  3. Run `from_ir`, emitting a `warning` event for each warning.
  4. Locate Pandoc through `ASHIFT_PANDOC` (an empty value means unset), then `PATH`. Check `PANDOC_SUPPORTED` (≥ 3.12, < 4).
  5. Run `pandoc [+RTS -M{n}M -RTS] --sandbox --log=<ws>/log/pandoc-log.json -f json -t docx -o <out>/document.docx`, with the stdio and env described above.
  6. Map log entries to warnings, and treat `CouldNotFetchResource` as an error.
  7. Emit `artifact`, then `result`.
  On any failure it emits `result ok:false` with a typed code and exits 1.
- **Hidden CLI entry.** `ashift __engine pandoc` lives in `crates/ariad-cli/src/commands/engine.rs` (moved here from phase 6). This makes the end-to-end check below possible.
- **DOCX metadata rewrite** (`docx_meta::stamp(path, time)`):
  - Rewrite only `docProps/core.xml` `dcterms:created`/`modified` to the conversion time. Use `SOURCE_DATE_EPOCH` when it is set (goldens), else the wall clock.
  - Copy every other entry raw with `zip`'s `raw_copy_file`, so the bytes are unchanged.
  - Runs on `out/document.docx` before promotion.
- **Shared constants:** `PANDOC_SUPPORTED` and `PANDOC_GOLDEN_VERSION = "3.12"` live in `ariad-host`. Phase 6's goldens use the second.

Non-functional:
- No `unsafe` code in our crates.
- Engines get a minimal environment: `PATH`, `SYSTEMROOT` (Windows), `ASHIFT_PANDOC`, and `TMPDIR`/`TMP`/`TEMP` set to the workspace.
- No document content in logs or errors.

## Architecture

```text
ashift convert ─ ariad-host::convert (phase 6)
  ├─ Workspace::new()  in/ out/ tmp/ log/
  ├─ assets::resolve   lexical check → cap-std open → sniff → embed
  ├─ write in/document.ir.json
  ├─ runner::run(current_exe, ["__engine","pandoc"], Request, cancel)
  │     stdin 1 line ▶ │ ◀ stdout NDJSON (≤1 MiB/line) │ ◀ stderr 64 KiB ring
  │     ashift __engine pandoc ─ engines::pandoc::serve
  │          └─ pandoc (stdin AST, stdout null, stderr drained; TMPDIR=ws/tmp) ← killed with the tree
  ├─ docx_meta::stamp(out/document.docx)
  ├─ Workspace::promote(dest, overwrite)  temp-in-dest + persist_noclobber
  └─ Workspace::close()  (also on every error / Ctrl-C path)
```

## Files

| Action | File | Size | Test impact |
|---|---|---|---|
| Create | `crates/ariad-core/src/protocol.rs` | ~220 | unit + schema |
| Modify | `crates/ariad-core/tests/schema_drift.rs` | Adds the protocol schema | drift |
| Create | `schemas/engine-protocol.v1.json` | generated | drift |
| Modify | `crates/ariad-host/Cargo.toml` | ariad-core, tokio, tokio-util, process-wrap, tempfile, cap-std, zip, serde_json (unbounded_depth), serde_stacker, unicode-normalization, thiserror | — |
| Create | `crates/ariad-host/src/{runner.rs,workspace.rs,assets.rs,pandoc_bin.rs,docx_meta.rs,ir_io.rs}` | ~800 | integration |
| Create | `crates/ariad-host/src/engines/{mod.rs,pandoc.rs}` | ~280 | conformance |
| Create | `crates/ariad-host/src/bin/engine-probe.rs` | Test engine. Ops: `echo`, `crash`, `hang`, `spawn-grandchild-and-hang`, `flood-stdout`, `flood-stderr`, `no-result`, `bad-json`, `event-after-result`, `fail-typed`, `ok-but-nonzero`. Never packaged | ~180 |
| Create | `crates/ariad-host/tests/{runner.rs,workspace.rs,assets.rs,docx_meta.rs}` | ~500 | integration |
| Create | `crates/ariad-cli/src/commands/{mod.rs,engine.rs}` + modify `src/main.rs` | Hidden `__engine` | ~60 |
| Create | `crates/ariad-cli/tests/pandoc_engine.rs` | End-to-end through `CARGO_BIN_EXE_ashift`; `pandoc -f json` acceptance | ~150 |

## Implementation steps

1. **Protocol types.** Write them with struct variants, `#[serde(tag = "type")]`, and `skip_serializing_if = "Option::is_none"` on optional limits. Unit-test the updated §7 examples.
2. **Schema drift.** Extend the drift test to the protocol schema.
3. **Workspace.** Implement the layout, `close()` with retry, and `promote` with `persist_noclobber` and the `symlink_metadata` check.
4. **Runner.**
   - Wrap the command with `TokioCommandWrap`, adding `ProcessGroup::leader()` (Unix) and `JobObject` (Windows). Use the env allow-list.
   - Write the request line, then close stdin.
   - `select!` over: the events (`LinesCodec` max 1 MiB), the concurrent stderr ring, the optional timeout, and the cancellation signal.
   - Enforce the event grammar, then apply the outcome rule.
   - On every non-success path: call `start_kill()`, then `wait()`.
   - The public API is synchronous (an internal current-thread runtime). Cancellation is passed in as a token. Document both.
5. **Assets.** Implement the five steps above.
6. **Pandoc lookup.** `pandoc_bin.rs` handles `ASHIFT_PANDOC` (an empty or nonexistent path → `tool_missing` with the `just pandoc` hint), then `PATH`. It parses the version and gates on `PANDOC_SUPPORTED`.
7. **Engine.** Implement `engines/pandoc.rs` as specified. The engine's own Pandoc timeout is `timeout_s.and_then(|t| t.checked_sub(margin))`. The host still owns the hard kill.
8. **DOCX stamp.** Implement `docx_meta.rs` and test it:
   - only `core.xml` changes;
   - all other entries are byte-identical;
   - `SOURCE_DATE_EPOCH` is honored;
   - a zip whose entries exceed a size cap of 64 MiB per entry is refused (zip-bomb guard).
9. **Hidden CLI entry.** Add `__engine` to `ariad-cli`; it dispatches `pandoc` → `serve`, and anything else exits 2.
10. **Runner tests**, each run on all three OSes:

    | Probe op | Expected |
    |---|---|
    | `echo` | Success |
    | `fail-typed` | `EngineFailed { code: tool_missing }` |
    | `ok-but-nonzero` | `Crash` |
    | `crash` | `Crash` with the stderr tail |
    | `no-result` | `Crash` |
    | `bad-json` | `ProtocolViolation` |
    | `event-after-result` | `ProtocolViolation` |
    | `flood-stdout` (2 MiB line) | `ProtocolViolation`, tree killed |
    | `flood-stderr` (50 MiB) | No deadlock |
    | `hang` with a 2 s timeout | `Timeout` within 5 s |
    | `spawn-grandchild-and-hang` with timeout **and** with cancellation | The grandchild's heartbeat file stops growing within 1 s, and the workspace no longer exists |
    | `hang` with no timeout (unlimited), then cancellation | `Cancelled` |

11. **Asset tests:**
    - a relative PNG is embedded;
    - a bare filename with an empty parent works;
    - `../x.png`, `/abs.png`, `C:\x.png`, `\\host\share\x.png`, `file:///x`, `%2e%2e/x.png` are refused **with no filesystem access** (lexical);
    - an http URL is kept with a warning;
    - an oversize file (with a limit set) is refused;
    - a non-image file is refused;
    - an NFD-named file is found from an NFC href;
    - Unix only: a symlink leaving the base is refused, and a FIFO is refused without blocking;
    - Windows only: device names (`CON.png`, `NUL`) are refused.
12. **Promote tests:** an existing destination gives `DestinationExists`; a dangling symlink destination gives `DestinationExists` (Unix); `--overwrite` replaces atomically; a failure leaves no partial file.
13. **End-to-end engine test** (`ariad-cli/tests/pandoc_engine.rs`):
    - For every Markdown fixture: run `from_ir`, pipe the result into `pandoc -f json -t json`, and require exit 0. This shows Pandoc accepts our AST.
    - Run the 64-level nested fixture through `runner::run(CARGO_BIN_EXE_ashift, ["__engine","pandoc"])` and expect success. This proves the deep-JSON path.
    - Expect `tool_missing` when `ASHIFT_PANDOC` points nowhere and `PATH` is emptied.
    - Expect `limit_exceeded` with `max_memory_mb: Some(8)`.
14. Run `just ci` on three OSes.

## Todo

- [ ] Protocol types, error codes, outcome rule, schema + drift
- [ ] Workspace (layout, close with retry, atomic no-clobber promote, symlink check)
- [ ] Runner (tree kill on every non-success path, cancellation, optional timeout, bounded I/O, grammar)
- [ ] Assets (lexical → cap-std → regular file → sniff → NFC/NFD)
- [ ] Pandoc lookup + shared version constants
- [ ] Pandoc engine (stdio, env, optional `-M`, log → warnings)
- [ ] Deep-IR JSON reads
- [ ] DOCX timestamp stamp
- [ ] Hidden `ashift __engine pandoc`
- [ ] Probe, asset, promote, stamp and end-to-end tests green on 3 OSes

## Test scenario matrix

| Priority | Scenario | Expected |
|---|---|---|
| Critical | Typed engine failure | `EngineFailed` with its code (not `Crash`) |
| Critical | Timeout or cancel with a grandchild, all OSes | Tree dead, workspace gone |
| Critical | UNC or `..` image path | Rejected lexically, no filesystem or SMB access |
| Critical | 64-level nesting end to end | Success (no serde depth error) |
| Critical | Our AST into `pandoc -f json` | Accepted for every fixture |
| High | Dangling symlink at the destination | `DestinationExists` |
| High | Pandoc missing or 3.11 | `tool_missing` / `tool_version` + hint |
| High | `max_memory_mb: Some(8)` | `limit_exceeded` |
| High | Unlimited timeout, then Ctrl-C | `Cancelled`, clean workspace |
| Medium | DOCX stamp | Only `core.xml` changed |

## Success criteria

- Runner, workspace, assets, stamp and end-to-end tests are green on Linux, macOS and Windows.
- `ashift __engine pandoc` converts every Markdown fixture's IR to DOCX through the runner.
- Both schemas are generated and drift-checked.

## Risk assessment

| Risk | Likelihood | Mitigation |
|---|---|---|
| Job object semantics differ from expectations on CI | Medium | Grandchild and cancellation tests run on Windows in CI; explicit `start_kill` + workspace retry; replaceable by `windows-sys` if needed |
| cap-std behaves differently on Windows | Low | Lexical validation already blocks the dangerous inputs; cap-std is the second layer |
| serde_stacker grows the stack on wasm | Low | It is only used in `ariad-host` (native) |
| Env allow-list breaks Pandoc on macOS/Windows | Medium | Proven by the CI end-to-end test |

## Security considerations

- No file outside the input's directory is ever read, and no network or SMB path is touched (lexical check first).
- Pandoc runs as a killable grandchild with `--sandbox`, an optional heap cap, a private temp dir, a minimal environment and a discarded stdout.
- The DOCX stamp only touches `core.xml`, and it bounds entry sizes.
- The workspace is deleted on success, error, timeout and Ctrl-C.
- Known gap, documented for 1b: no OS sandbox yet (Landlock, seccomp, Seatbelt, AppContainer).

## Next steps

Phase 6 wires `ashift convert` and adds the DOCX goldens and CLI contract tests.
