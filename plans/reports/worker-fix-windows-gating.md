# Worker Fix Report: `windows-gating`

## Issue / PR #146: Gate unix-only test imports and bindings so Windows builds warning-free

### Cause and Evidence
- In `crates/ariad-host/tests/tool_secrets.rs:18`, `REDACTED` and `SecretBag` were imported unconditionally (`use ariad_host::tools::{InputRef, OutputTarget, REDACTED, RecipeRequest, SecretBag, ToolRequest, run_recipe, run_tool};`), whereas their only call sites in that integration test file are inside two `#[cfg(unix)]` test functions:
  - `engine_secret_delivery_uses_0600_params_file_and_scrubs_warnings_errors_and_stderr` (`crates/ariad-host/tests/tool_secrets.rs:197`)
  - `recipe_isolates_secrets_per_step_tool_spec` (`crates/ariad-host/tests/tool_secrets.rs:457`)
  On the `Lint (windows-2025)` CI job (`cargo clippy --workspace --all-targets --all-features -- -D warnings`), `rustc` rejected `REDACTED` and `SecretBag` with `error: unused imports: REDACTED and SecretBag` under `-D warnings`.
- A workspace-wide audit of every `#[cfg(...)]` occurrence (`rg -n "#!?\[cfg" crates/ apps/`, covering 40+ files across `crates/ariad-host`, `crates/ariad-cli`, `crates/ariad-server`, and `apps/desktop/src-tauri`) found one additional unix-only consumer of an unconditional binding:
  - In `crates/ariad-host/src/packs/install.rs:1011-1047` (`manifest_verified_entries`), `let declared_executables = manifest_allowed_executables(target);` was cloned into `let mut allowed_executables = declared_executables.clone();` and subsequently read only inside `#[cfg(unix)] if !component.executable && !declared_executables.contains(&rel)`. On `not(unix)` targets (Windows), `declared_executables` has no consumer after `.clone()`, which is susceptible to `clippy::redundant_clone` under `-D warnings`.
- All other platform-gated modules, imports, helpers, and tests across `crates/` and `apps/` properly pair `#[cfg(unix)]` with `#[cfg(windows)]` / `#[cfg(not(unix))]` or scope imports and helpers inside the `#[cfg(...)]` blocks.

### Assumptions Recorded
- Gating `use ariad_host::tools::{REDACTED, SecretBag};` with `#[cfg(unix)]` at the top of `crates/ariad-host/tests/tool_secrets.rs` is cleaner than duplicating the import inside each `#[cfg(unix)]` test function and preserves all existing tests without adding any `#[allow(unused)]`.
- In `crates/ariad-host/src/packs/install.rs`, gating `declared_executables` and its `.clone()` with `#[cfg(unix)]` and initializing `allowed_executables` directly from `manifest_allowed_executables(target)` under `#[cfg(not(unix))]` keeps runtime behavior identical on both Unix and Windows while eliminating the unconsumed clone on Windows.
- When `cargo clippy --target x86_64-pc-windows-gnu -p ariad-host -p ariad-cli --all-targets --features ariad-host/test-probe -- -D warnings` failed in `ring v0.17.14`'s C build script (`ToolNotFound: failed to find tool "x86_64-w64-mingw32-gcc"`), per the task specification we did not install system packages or debug the mingw toolchain, and instead relied on the workspace-wide `#[cfg(...)]` audit plus Linux verification and Windows CI.

### Security Review (`ak:security`)
- **Scope:** `crates/ariad-host/tests/tool_secrets.rs`, `crates/ariad-host/src/packs/install.rs`.
- **Secrets & `.env`:** No secrets, credentials, or `.env*` files added or modified. All secret-redaction and `0600` delivery tests in `tool_secrets.rs` remain intact.
- **STRIDE:** Purely compile-time `#[cfg(unix)]` / `#[cfg(not(unix))]` gating of unused imports and a redundant clone on Windows; zero behavioral change to pack verification, executable allow-listing, or secret scrubbing on any platform.
- **Licensing:** No dependencies, fixtures, or third-party assets added or changed.

### Fix
- `crates/ariad-host/tests/tool_secrets.rs`: Split `REDACTED` and `SecretBag` out of the unconditional `ariad_host::tools` import into a `#[cfg(unix)] use ariad_host::tools::{REDACTED, SecretBag};` import.
- `crates/ariad-host/src/packs/install.rs`: Gated `declared_executables` and `declared_executables.clone()` under `#[cfg(unix)]` and initialized `allowed_executables` directly with `manifest_allowed_executables(target)` under `#[cfg(not(unix))]`.

### Regression Tests
- `crates/ariad-host/tests/tool_secrets.rs` (8 integration tests including `engine_secret_delivery_uses_0600_params_file_and_scrubs_warnings_errors_and_stderr` and `recipe_isolates_secrets_per_step_tool_spec`)
- `crates/ariad-host/tests/pack_resolve_verify.rs` (8 integration tests covering pack manifest and executable verification)
- `crates/ariad-host/src/packs/` unit tests (20 unit tests)

### Verification Commands
- `rustup target add x86_64-pc-windows-gnu` + `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh cargo clippy --target x86_64-pc-windows-gnu -p ariad-host -p ariad-cli --all-targets --features ariad-host/test-probe -- -D warnings`: stopped at `ring v0.17.14` build script (`failed to find tool "x86_64-w64-mingw32-gcc": No such file or directory`); noted per Step 3 instructions.
- `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh cargo fmt --all --check`: **PASS**
- `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh cargo clippy -p ariad-host -p ariad-cli --all-targets --features ariad-host/test-probe -- -D warnings`: **PASS**
- `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh cargo test -p ariad-host --features test-probe --test tool_secrets`: **PASS** (8 passed, 0 failed)
- `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh cargo test -p ariad-host --lib packs`: **PASS** (20 passed, 0 failed)
- `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh cargo test -p ariad-host --test pack_resolve_verify`: **PASS** (8 passed, 0 failed)

### What Only CI or Another OS Can Verify
- Full `cargo clippy --workspace --all-targets --all-features -- -D warnings` and `cargo nextest run` on `windows-2025` (where MSVC/C toolchains are installed for `ring` and native Windows targets).

### Suggested PR Text
**Title:**
`fix(host): build the runner limits test and gate unix-only imports on Windows`

**Body:**
```markdown
## Summary
- Export `apply_pre_exec_rlimits` on Windows in `ariad-host::runner::limits` so `crates/ariad-host/tests/tool_runner_limits.rs` compiles on Windows.
- Gate unix-only `REDACTED` and `SecretBag` imports in `crates/ariad-host/tests/tool_secrets.rs` behind `#[cfg(unix)]` so `Lint (windows-2025)` builds cleanly under `-D warnings`.
- Gate `declared_executables` and its `.clone()` in `crates/ariad-host/src/packs/install.rs` behind `#[cfg(unix)]` (using `#[cfg(not(unix))]` direct initialization on Windows) to avoid a redundant clone warning when the unix-only executable-bit check is compiled out.
```
