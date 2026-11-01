# Worker report: p04 s5 (host and server registration for the pdfium pack)

Branch `p04/s5` (worktree `/home/vchun/Codes/01-AriadShift-wt-p04-s5`). Status: **incomplete, handed over to continue on another worker.** The implementation, tests and docs are committed. Several of the required checks were not run to completion after a project-manager stop; they are listed below with exact commands.

## What is implemented

1. **Manifest `engine_protocol`** (`packs/manifest.rs`, `packs/author.rs`). Optional field; absent means `ariad-engine/1`. The check lives in `Manifest::validate`, which `Manifest::parse` calls, so install, resolve of an installed pack, the `ASHIFT_PACK_<NAME>` override and `ashift __pack manifest` all refuse a pack whose value differs from `EngineSpec.protocol` of the engine of the same name, or whose value is unknown. Docling manifests without the field stay valid. `ashift __pack manifest` emits the field from the `--base` template; it has no flag of its own (see open points). Install failure is `InvalidManifest` (exit 1); at resolve it surfaces as `tool_missing` (exit 5, reinstall hint).
2. **Stub gating** (`engines/mod.rs`). `serve_v2_pack_engine`, its probe hooks and `tools_for_engine` moved into an inline `#[cfg(feature = "test-probe")] mod test_pack_engine`. Without the feature `ashift __engine <pack id>` exits 2 (unknown engine). Unit tests cover both builds.
3. **Resolution and sandbox** (`engines/resolve.rs`, `engines/sandbox.rs`). `pdfium` now gets `private_tmp: true` (read-only pack dir and writable job dir come from `install_root` and the workspace as for every engine; no network from the base policy; no new env var). `resolve.rs` refuses an entrypoint of a `sandbox_required` pack that leaves the pack root after following links (`VerificationFailed`, exit 5).
4. **Missing pack.** Every pdfium tool id fails with `ToolError::ToolMissing`, exit 5, code `tool_missing`, hint `run 'ashift engines install pdfium' to install the pdfium engine pack` (existing wording in `EngineSpec.install_hint`). Tested through a custom `ToolSpec` with `engine: "pdfium"` for all eight phase-4 tool ids.
5. **`doctor` and `ashift engines`** (`commands/doctor.rs`, `commands/engines.rs`). `doctor` gains the optional check `pdfium_engine` (not installed plus hint; installed: engine version and protocol after a v2 `describe` round trip; wrong protocol, bad verification and sandbox failures reported with the cause). `engines` always lists a `pdfium` row: `not installed` with the hint as note, or `ready` with the pack version and note `protocol ariad-engine/2`.
6. **Server** (`ariad-server/src/packs.rs`, `routing.rs`, `api/{mod,plan,jobs}.rs`). `Pack::for_engine` and `Pack::for_route` return `Result<_, UnknownEngine>`; only `ariad-core`, `pandoc`, `pdf`, `typst` (Core), `libreoffice` (Office) and `docling` (Docling) are mapped. Unmapped engines are not deployable, their capability edges are skipped, and a route through one is a 422 `unsupported_route` (helper `api::route_pack`). The pdfium cloud mapping is not added.
7. **`packs.toml`** not touched. The entry the release will need (add to `crates/ariad-host/data/packs.toml` when the signed `pack-pdfium-v<version>` release exists):

   ```toml
   [[packs]]
   name = "pdfium"
   version = "<released version>"
   description = "PDFium: page rendering, thumbnails, images and text with positions (ariad-engine/2)"
   manifest = "https://github.com/bavanchun/AriadShift/releases/download/pack-{name}-v{version}/pack.toml"
   ```

   The index test `embedded_index_is_valid` accepts it as is.
8. **Docs.** `docs/packs.md` (field row, new "Engine protocol" section, pdfium pack section, resolution paragraph, exit codes, signing note), `docs/cli.md` (exit 1 and 5 rows, `engines` and `doctor` behaviour, `ASHIFT_PACK_<NAME>` note), `ARCHITECTURE.md` (§2.4 and §7.1 pdfium rows, resolution bullet, three Decision Log rows: manifest field, stand-in gating, server fail-closed).

## Commits (3, on `p04/s5`)

- `a78fe652 feat(host): register the pdfium pack engine and fail closed`
- `2773c720 fix(server): fail closed for engines without a worker pack`
- `aeabdf97 docs: describe the pdfium pack and the engine_protocol field`

This report is committed on top as a fourth commit. `sh scripts/check-commits.sh origin/dev..HEAD` has **not** been run.

## Checks: what was run

All through `build-lock.sh`, without `--locked`, `ASHIFT_PANDOC` set where it matters.

| Command | Result |
|---|---|
| `cargo clippy -p ariad-host -p ariad-server -p ariad-cli --all-targets --features ariad-host/test-probe -- -D warnings` | PASS, but run before the final edits to `pdfium_pack.rs` (import fix and test-name fix) |
| `cargo clippy -p ariad-host --all-targets -- -D warnings` (no test-probe) | PASS on the code before the new tests existed; a later run flagged an unused `PathBuf` import in `pdfium_pack.rs`, fixed, **not re-run** |
| `cargo test -p ariad-host --features test-probe --lib` | PASS, 173 passed |
| `cargo test -p ariad-host --features test-probe --test pdfium_pack` | PASS, 11 passed (first run had 2 failures from a wrong `--exact` test path in my own child re-run; fixed and re-run green) |
| `cargo test -p ariad-host --features test-probe --test pdfium_sandbox` | PASS, 3 passed (Linux: Required mode, Landlock and seccomp active on this host, no skip confirmed only by absence of failure; the `--nocapture` skip count was not read) |
| `cargo fmt --all` | applied; `cargo fmt --all --check` after the last edit **not re-run** |

## Open: checks not run or not green (exact commands)

Run each as `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh <command>`, narrowly and one at a time.

1. `cargo fmt --all --check`
2. `cargo clippy -p ariad-host -p ariad-server -p ariad-cli --all-targets --features ariad-host/test-probe -- -D warnings`
3. `cargo clippy -p ariad-host --all-targets -- -D warnings`
4. `cargo test -p ariad-host` (no feature; includes `production_builds_never_answer_for_a_pack_engine`). Use `--lib` first.
5. `ASHIFT_PANDOC=/home/vchun/Codes/01-AriadShift/.tools/pandoc/bin/pandoc cargo test -p ariad-host --features test-probe` for the remaining host test binaries that exercise the moved stub and the changed `PackBuilder`: `tool_runner_limits`, `tool_secrets`, `tool_publish_atomic`, `pack_resolve_verify`, `packs`, `runner`, `session`, `sandbox`. Expect most to pass; `pack_resolve_verify` and `packs` install packs named `qpdf`, `pdfium` and `tesseract` and now rely on `PackBuilder::new` defaulting `engine_protocol` for registered v2 names.
6. `cargo test -p ariad-server` (not run at all; the `Pack` signature change is only compile-checked by clippy).
7. `cargo test -p ariad-cli --features ariad-host/test-probe --test engine_conformance_v2`. I added a `[[test]]` entry with `required-features = ["ariad-host/test-probe"]` to `crates/ariad-cli/Cargo.toml` so the test is not built without the probe; not run.
8. macOS: `cargo test -p ariad-host --features test-probe --test pdfium_sandbox` on a macOS runner (the macOS test is `#[cfg(target_os = "macos")]`, never executed here).
9. `sh scripts/check-commits.sh origin/dev..HEAD`.

## Known failures (expected, not yet fixed)

- **`pack_schema` will fail**: `Manifest` gained a field, so `schemas/pack.v1.json` drifted. It is an integrator-owned shared file and was not edited. Fix: `ARIAD_BLESS_SCHEMAS=1 cargo test -p ariad-host --test pack_schema`, then commit `schemas/pack.v1.json`.
- **`ariad-cli` snapshot tests will fail**: `tests/snapshots/commands_cli__engines_human_output.snap` and `commands_cli__engines_json_output.snap` (and possibly a doctor snapshot) gain a `pdfium` row or check. Run `cargo test -p ariad-cli --features ariad-host/test-probe --test commands_cli`, review, and accept with `INSTA_UPDATE=always` or `cargo insta accept`. These files are outside my ownership list, so I did not touch them.

## Changes the integrator must make to shared files

- `schemas/pack.v1.json`: regenerate (command above).
- Nothing else in the shared list. No new dependency, `Cargo.toml` of the workspace and `Cargo.lock` untouched.
- Outside my ownership list but committed because they follow directly from the change: `crates/ariad-cli/Cargo.toml` (`engine_conformance_v2` test entry) and `crates/ariad-host/tests/support/mod.rs` (`PackBuilder` gets `engine_protocol`, defaulting from the registry, and builds from a `--base`-style template when it is set).
- Once the phase-4 `ToolSpec`s are registered, add a loop in `crates/ariad-host/tests/pdfium_pack.rs` (`a_missing_pdfium_pack_fails_every_pdfium_tool_...`) over `ariad_core::tools::all()` filtered by `engine == "pdfium"`, using valid minimal params so params validation (exit 9) does not precede the missing-pack error (exit 5). Today the test uses a permissive custom spec per phase-4 tool id.

## Deviations and gaps

- No `--engine-protocol` flag on `ashift __pack manifest`: adding it needs a field on `ManifestRequest` and a change in `crates/ariad-cli/src/main.rs`, which I do not own. The field is emitted from the `--base` template and covered by tests in `author.rs`. Slice s4's report was not on `p04/s4` (no `plans/reports/worker-p04-s4.md`, no commits on that branch), so its exact flag, build recipe and signing text are unknown. `docs/packs.md` therefore describes the pack from the phase file and does not name the build recipe or workflow; check it against s4's final scripts (`engines/pdfium/pack/`, `.github/workflows/pack-pdfium.yml`) when merging, and confirm the `engine_protocol` template mechanism matches.
- `doctor` adds a `pdfium_engine` check in addition to the existing generic `pack_pdfium` line for installed packs.
- The `ashift engines` not-installed note for pdfium is the install hint; docling keeps `available in v0.2` so its snapshot text is unchanged.
- Private `/tmp` for pdfium means the engine needs user and mount namespaces on Linux; where they are denied the engine fails to start under `best-effort` (refuses to degrade), with the existing namespace hint.

## Skills used

Planning and review by reading the source (no skill was invoked as a tool call): the scenario, security, test and docs steps were done inline. `/ak-git` rules followed by hand (explicit paths, conventional commits, no trailers).

## Unresolved questions

- Should `ashift __pack manifest` get a first-class `--engine-protocol` flag (needs `main.rs`), or is the `--base` template route enough for s4's pack script?
- Should `doctor` also list the other v2 packs (`qpdf`, `tesseract`, ...) before they have releases, or only pdfium as now?
