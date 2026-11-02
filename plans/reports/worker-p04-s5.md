# Worker report: p04 s5 (host and server registration for the pdfium pack)

Branch `p04/s5` (worktree `/home/vchun/Codes/01-AriadShift-wt-p04-s5`). Status: **complete.** All required host/server registrations, sandbox policies, manifest flags, conformance gates, checks and docs are implemented, verified through the lock, and documented.

## What is implemented

1. **Manifest `engine_protocol`** (`packs/manifest.rs`, `packs/author.rs`, `crates/ariad-cli/src/main.rs`). Optional field; absent means `ariad-engine/1`. The check lives in `Manifest::validate`, which `Manifest::parse` calls, so install, resolve of an installed pack, the `ASHIFT_PACK_<NAME>` override and `ashift __pack manifest` all refuse a pack whose value differs from `EngineSpec.protocol` of the engine of the same name, or whose value is unknown. Docling manifests without the field stay valid. `ashift __pack manifest` supports `--engine-protocol <PROTOCOL>` directly and emits the top-level line before `[[targets]]`, also accepting the field in or overriding a `--base` manifest. Install failure is `InvalidManifest` (exit 1); at resolve it surfaces as `tool_missing` (exit 5, reinstall hint).
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
8. **Docs.** `docs/packs.md` (field row, `--engine-protocol` flag on `ashift __pack manifest`, "Engine protocol" section, pdfium pack section, resolution paragraph, exit codes, signing note), `docs/cli.md` (exit 1 and 5 rows, `engines` and `doctor` behaviour, `ASHIFT_PACK_<NAME>` note), `ARCHITECTURE.md` (§2.4 and §7.1 pdfium rows, resolution bullet, three Decision Log rows: manifest field, stand-in gating, server fail-closed).

## Commits (on `p04/s5`)

- `a78fe652 feat(host): register the pdfium pack engine and fail closed`
- `2773c720 fix(server): fail closed for engines without a worker pack`
- `aeabdf97 docs: describe the pdfium pack and the engine_protocol field`
- `ef9f068e docs(plans): report the host and server pdfium registration slice`
- (final commit): `feat(host): add --engine-protocol flag to __pack manifest and finalize s5 checks`

## Checks: what was run

All run through `build-lock.sh`, without `--locked`, narrowly and one test binary at a time, with `ASHIFT_PANDOC` set where required.

| Command | Result |
|---|---|
| `cargo fmt --all --check` | PASS |
| `cargo clippy -p ariad-host -p ariad-server -p ariad-cli --all-targets --features ariad-host/test-probe -- -D warnings` | PASS |
| `cargo clippy -p ariad-host --all-targets -- -D warnings` | PASS (proves stub is absent from production) |
| `ASHIFT_PANDOC=... cargo test -p ariad-host --lib` | PASS, 175 passed |
| `cargo test -p ariad-host --lib packs::author` | PASS, 5 passed |
| `cargo test -p ariad-host --lib production_builds_never_answer_for_a_pack_engine` | PASS, 1 passed |
| `ASHIFT_PANDOC=... cargo test -p ariad-host --features test-probe --test pdfium_pack` | PASS, 11 passed |
| `ASHIFT_PANDOC=... cargo test -p ariad-host --features test-probe --test pdfium_sandbox` | PASS, 3 passed |
| `ASHIFT_PANDOC=... cargo test -p ariad-host --features test-probe --test tool_runner_limits` | PASS, 9 passed |
| `ASHIFT_PANDOC=... cargo test -p ariad-host --features test-probe --test tool_secrets` | PASS, 8 passed |
| `ASHIFT_PANDOC=... cargo test -p ariad-host --features test-probe --test tool_publish_atomic` | PASS, 12 passed |
| `ASHIFT_PANDOC=... cargo test -p ariad-host --features test-probe --test pack_resolve_verify` | PASS, 7 passed |
| `ASHIFT_PANDOC=... cargo test -p ariad-host --features test-probe --test packs` | PASS, 33 passed |
| `ASHIFT_PANDOC=... cargo test -p ariad-host --features test-probe --test runner` | PASS, 25 passed |
| `ASHIFT_PANDOC=... cargo test -p ariad-host --features test-probe --test session` | PASS, 19 passed |
| `ASHIFT_PANDOC=... cargo test -p ariad-host --features test-probe --test sandbox` | PASS, 10 passed |
| `cargo test -p ariad-server --lib packs::tests` | PASS, 4 passed |
| `cargo test -p ariad-server --lib routing::tests` | PASS, 8 passed |
| `cargo test -p ariad-server --lib api::` | PASS, 13 passed |
| `cargo test -p ariad-cli --features ariad-host/test-probe --test engine_conformance_v2` | PASS, 5 passed |
| `cargo run -p ariad-cli --bin ashift -- __pack manifest --help` | PASS, shows `--engine-protocol` |
| `sh scripts/check-commits.sh origin/dev..HEAD` | PASS (using `.tools/bin/committed` symlink to `/home/vchun/Codes/01-AriadShift/.tools/bin/committed`) |

macOS sandbox test (`#[cfg(target_os = "macos")]`) runs on macOS runners in CI as designed.

## Changes the integrator must make to shared files

- `schemas/pack.v1.json`: regenerate with `ARIAD_BLESS_SCHEMAS=1 cargo test -p ariad-host --test pack_schema` (reflects the new optional `engine_protocol` field).
- `crates/ariad-cli/tests/snapshots/`: update `commands_cli__engines_human_output.snap` and `commands_cli__engines_json_output.snap` when integrating `ashift engines` with the `pdfium` row.
- Once the phase-4 `ToolSpec`s are registered, add a loop in `crates/ariad-host/tests/pdfium_pack.rs` (`a_missing_pdfium_pack_fails_every_pdfium_tool_...`) over `ariad_core::tools::all()` filtered by `engine == "pdfium"`, using valid minimal params so params validation (exit 9) does not precede the missing-pack error (exit 5). Today the test uses a permissive custom spec per phase-4 tool id.

## Deviations and decisions

- `--engine-protocol <PROTOCOL>` added to `ashift __pack manifest` (`crates/ariad-cli/src/main.rs`) and `ManifestRequest` (`crates/ariad-host/src/packs/author.rs`), satisfying slice s4's requirement. It serializes before `[[targets]]`, and accepts or overrides `--base`.
- `doctor` adds a `pdfium_engine` check in addition to the existing generic `pack_pdfium` line for installed packs.
- The `ashift engines` not-installed note for pdfium is the install hint; docling keeps `available in v0.2` so its snapshot text is unchanged.
- Private `/tmp` for pdfium means the engine needs user and mount namespaces on Linux; where they are denied the engine fails to start under `best-effort` (refuses to degrade), with the existing namespace hint.
- Worktree `.tools/bin/committed` was missing; symlinked to `/home/vchun/Codes/01-AriadShift/.tools/bin/committed` to execute `scripts/check-commits.sh origin/dev..HEAD`.

## Skills used

- `/ak-cook`: implemented `--engine-protocol` in CLI and author modules, updated docs and tests.
- `/ak-test`: verified each test binary through the build lock without --locked.
- `/ak-git`: git checks and conventional commits.
