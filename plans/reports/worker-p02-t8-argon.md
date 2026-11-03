# Worker Report: Phase 2 — T8 Release and CI Wiring

## Status

`DONE`

## Files Modified (Owned Scope Only)

1. [`.github/workflows/publish-crates.yml`](file:///Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift-wt-p02-tools/.github/workflows/publish-crates.yml)
2. [`scripts/check-release-workflow.sh`](file:///Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift-wt-p02-tools/scripts/check-release-workflow.sh)
3. [`scripts/ci-scope.sh`](file:///Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift-wt-p02-tools/scripts/ci-scope.sh)
4. [`scripts/test-ci-scope.sh`](file:///Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift-wt-p02-tools/scripts/test-ci-scope.sh)
5. [`fuzz/Cargo.toml`](file:///Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift-wt-p02-tools/fuzz/Cargo.toml)
6. [`justfile`](file:///Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift-wt-p02-tools/justfile)

 Unmodified files verified in scope:
- [`Cargo.toml`](file:///Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift-wt-p02-tools/Cargo.toml): Coordinator commits `187757e` and `71332ee` already registered `ariad-engine-io = { path = "crates/ariad-engine-io", version = "0.2.0" }` and `ariad-pdf = { path = "crates/ariad-pdf", version = "0.2.0" }` under `[workspace.dependencies]` in topological order before `ariad-host`.
- [`dist-workspace.toml`](file:///Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift-wt-p02-tools/dist-workspace.toml): `cargo-dist` packages only binary targets (`ashift` in `ariad-cli`); library crates opt out via `[package.metadata.dist] dist = false` in their crate manifests. No change required in `dist-workspace.toml`.

---

## Summary of Changes

### 1. Topological Crate Publishing & Release Workflow Check (`.github/workflows/publish-crates.yml`, `scripts/check-release-workflow.sh`)

- Updated `crates=(...)` in `.github/workflows/publish-crates.yml` from `("ariad-core" "ariad-typst" "ariad-host" "ariad-cli")` to `("ariad-core" "ariad-pdf" "ariad-engine-io" "ariad-typst" "ariad-host" "ariad-cli")`, placing `ariad-pdf` and `ariad-engine-io` after `ariad-core` and before `ariad-host`.
- Extended section 8 (`# --- 8. Check publish-crates.yml if present ---`) in `scripts/check-release-workflow.sh` to validate that the `Publish crates in dependency order` step defines `crates=(...)` containing the exact six publishable crates (`ariad-core`, `ariad-pdf`, `ariad-engine-io`, `ariad-typst`, `ariad-host`, `ariad-cli`) in valid topological order (`ariad-core` first; `ariad-core`, `ariad-pdf`, `ariad-engine-io`, `ariad-typst` before `ariad-host`; `ariad-host` before `ariad-cli`).
- Confirmed that `plans/261031-1422-next-stage-pdf-suite/phase-03-structural-pdf-tools.md` (lines 79 and 92) already documents and gates `cargo publish --dry-run -p ariad-pdf -p ariad-engine-io`. Running `cargo publish --dry-run` inside `scripts/check-release-workflow.sh` is intentionally avoided because `scripts/test-check-release-workflow.sh` executes `check-release-workflow.sh` 79 times offline against temporary workflow files, and `ariad-core 0.2.0` with the new `tools` module is not yet published on `crates.io`.

### 2. CI Path Scope Classifier (`scripts/ci-scope.sh`, `scripts/test-ci-scope.sh`)

- Added `pdf` and `engine-io` to `web_paths` (`^crates/ariad-(core|pdf|engine-io|wasm|typst)/`) and `cloud_paths` (`crates/ariad-(server|core|pdf|engine-io|host|cli|wasm|typst)/`) in `scripts/ci-scope.sh`. Because `heavy` runs for any path not matching `$docs_paths|$js_paths|$e2e_paths`, changes under `crates/ariad-pdf/` and `crates/ariad-engine-io/` now classify as `heavy=true|web=true|code change` and `cloud=true`.
- Added table-driven classification tests and cloud E2E scope assertions for `crates/ariad-pdf/src/lib.rs` and `crates/ariad-engine-io/src/lib.rs` in `scripts/test-ci-scope.sh`.

### 3. Fuzz Target Registration (`fuzz/Cargo.toml`, `justfile`)

- Added `ariad-pdf = { path = "../crates/ariad-pdf", optional = true }` to `[dependencies]` in `fuzz/Cargo.toml` and updated `[features]` `pdf-reader = ["ariad-core/pdf", "dep:ariad-pdf"]`.
- Registered three `[[bin]]` targets in `fuzz/Cargo.toml` (`test = false`, `doc = false`, `bench = false`):
  - `page_range` (`path = "fuzz_targets/page_range.rs"`)
  - `recipe_json` (`path = "fuzz_targets/recipe_json.rs"`)
  - `pdf_open_bounded` (`path = "fuzz_targets/pdf_open_bounded.rs"`, `required-features = ["pdf-reader"]`)
- Updated the `fuzz` recipe `case "$t"` in `justfile` so `pdf_open_bounded` builds with `--dev --features pdf-reader` alongside `pdf_reader`.

---

## New Dependencies and Licences

- None. `ariad-pdf` added to `fuzz/Cargo.toml` is a path dependency to the workspace crate (`MIT OR Apache-2.0`).

---

## Verification

- `sh scripts/test-ci-scope.sh`: **82 passed, 0 failed**
- `sh scripts/check-release-workflow.sh`: **OK** (`.github/workflows/release.yml` and `.github/workflows/publish-crates.yml` passed all release security and structural checks)
- `sh scripts/test-check-release-workflow.sh`: **78 passed, 2 skipped (`pwsh` not installed locally), 0 failed**
- `uv run --no-project --with pyyaml==6.0.3 python scripts/test-fuzz-gate.py`: **3 passed, 0 failed**
- `cargo check --manifest-path fuzz/Cargo.toml --bins --features pdf-reader,typst-writer`: **OK** (all 21 fuzz binaries including `page_range`, `recipe_json`, and `pdf_open_bounded` type-checked cleanly)
- `cargo fmt --manifest-path fuzz/Cargo.toml --check`: **OK**

---

## Coordinator Integration Notes (Unowned Files)

The following items live in files outside T8's file ownership list and should be applied by the coordinator during wave integration:

1. **`crates/ariad-pdf/Cargo.toml` publish metadata**:
   Worker T3 set `publish = false` in `crates/ariad-pdf/Cargo.toml` and omitted `readme`, `keywords`, `categories`, and `[package.metadata.dist]`. To match `crates/ariad-engine-io/Cargo.toml` and `.github/workflows/publish-crates.yml`, update `crates/ariad-pdf/Cargo.toml`:
   ```toml
   publish = true
   description = "Bounded in-process PDF foundation and document manipulation for AriadShift."
   readme = "../../README.md"
   keywords = ["document", "pdf", "lopdf"]
   categories = ["command-line-utilities", "encoding", "parser-implementations"]

   [package.metadata.dist]
   dist = false
   ```
2. **`apps/web/scripts/sync-notices.mjs` WASM workspace closure ignore list**:
   Once `ariad-wasm` depends on `ariad-pdf`, add `"ariad-pdf"` alongside `"ariad-core"` and `"ariad-wasm"` in `computeWasmRustProductionClosure` (`apps/web/scripts/sync-notices.mjs:403`).
3. **`justfile` non-fuzz recipes (`package-check`, `wasm`)**:
   T8 only owned the `fuzz` case in `justfile`. During final integration, `justfile`'s `package-check` recipe (`line 278`) can be extended to include `ariad-pdf` and `ariad-engine-io` before `ariad-host`.
4. **`.github/workflows/ci.yml` fuzz target list and `fuzz/seeds/` + `fixtures/manifest.toml`**:
   - `crates/ariad-cli/tests/fuzz_seed_manifest.rs` enforces a strict 1-to-1 bijection between files under `fuzz/seeds/<target>/*` and `[[fuzz_seed]]` entries in `fixtures/manifest.toml`.
   - When adding seed directories `fuzz/seeds/page_range/`, `fuzz/seeds/recipe_json/`, and `fuzz/seeds/pdf_open_bounded/`, register each seed file in `fixtures/manifest.toml` with `target`, `path`, `sha256`, `source = "AriadShift project (synthetic seed)"`, `license = "MIT OR Apache-2.0"`, and `coverage_note`, and add `page_range`, `recipe_json`, and `pdf_open_bounded` to the `for t in ...` loop in `.github/workflows/ci.yml` (adding `pdf_open_bounded)` to its `case "$t"` for `--features pdf-reader`).
5. **`fuzz/Cargo.lock` reconciliation**:
   - Because `fuzz/` is an independent workspace (`[workspace] members = ["."]`) and `just fuzz` invokes `cargo test --locked --manifest-path fuzz/Cargo.toml --bin docx_meta`, run `cargo generate-lockfile --manifest-path fuzz/Cargo.toml` (or `cargo check --manifest-path fuzz/Cargo.toml`) during lockfile reconciliation and commit `fuzz/Cargo.lock` alongside root `Cargo.lock`.
