# Worker Report: Fix Red CI Jobs on `dev` (`fix/dev-ci-red`)

## Overview

Investigated and resolved the 6 red CI items on `dev` across workflow runs `41234198441`, `41266573042`, and `41266591997`.

---

## 1. Bench Truth-File Count (`bench/tests/test_references.py`)

- **Cause & Evidence**:
  - In workflow run `41234198441` (`Bench tests`), `TestReferences.test_all_truth_files_conform_to_canonical_schema` failed at `bench/tests/test_references.py:112-115` (`AssertionError: 23 != 1`) because the test hard-coded `self.assertEqual(scored_count, 41)` and `expected_unscored = 1 + len(list(self.root.glob("fixtures/pdf-tools/*.truth.json")))` instead of deriving expected truth companions from the authoritative `fixtures/manifest.toml`.
- **Fix**:
  - Updated `test_all_truth_files_conform_to_canonical_schema` in `bench/tests/test_references.py` to parse `fixtures/manifest.toml` via `tomllib`, collect all `.truth.json` companion paths declared in `[[fixture]]` entries, assert exact set equality between `.truth.json` files on disk and `.truth.json` companions in `fixtures/manifest.toml`, verify every unscored companion includes a non-empty `unscored_reason` string and every scored companion loads cleanly into `CanonicalDoc`, and assert `scored_count + unscored_count == len(manifest_truth_paths)` with both counts `> 0`.
- **Verification**:
  - `ASHIFT_PANDOC=/home/vchun/Codes/01-AriadShift/.tools/pandoc/bin/pandoc uv run --package ariad-bench pytest bench/tests/test_references.py` -> **PASS** (`8 passed`).
- **CI-Only Scope**:
  - None; verified locally against `fixtures/manifest.toml` and all truth companions on disk.

---

## 2. Server Container Smoke Port Clash (`scripts/test-server-container.sh`, `docs/server.md`)

- **Cause & Evidence**:
  - In workflow run `41234198441` (job `124637701828`, `Server integration`), `scripts/test-server-container.sh` failed at line 17 (`listener.bind(("127.0.0.1", 58042))` -> `OSError: [Errno 98] Address already in use`). Port `58042` lies inside the Linux default ephemeral port range (`net.ipv4.ip_local_port_range = 32768 60999`), allowing client connections from earlier steps in `just test-server` to hold `58042` when the container smoke test starts.
- **Fix**:
  - Changed the default container smoke port in `scripts/test-server-container.sh` from `58042` to `28042` (below `32768`) with override support via `ARIAD_CONTAINER_SMOKE_PORT="${ARIAD_CONTAINER_SMOKE_PORT:-28042}"` across the pre-bind check, container `ARIAD_BIND`/`ARIAD_PUBLIC_URL` environment variables, and Python smoke client `base` URL. Preserved loud failure when the port is already in use.
  - Updated `docs/server.md` line 241 to document deterministic port `28042` and `ARIAD_CONTAINER_SMOKE_PORT`.
- **Verification**:
  - `bash -n scripts/test-server-container.sh` -> **PASS**.
  - Busy-port refusal check (holding an ephemeral port with a Python socket and invoking `ARIAD_TEST_DATABASE_URL=postgres://unused ARIAD_CONTAINER_SMOKE_PORT=<port> bash scripts/test-server-container.sh`) -> **PASS** (exited `1` with `Address already in use`).
- **CI-Only Scope**:
  - Full `just test-server-container` Docker image build and end-to-end PostgreSQL/S3 container smoke run in the `Server integration` workflow job (skipped locally to respect Phase-3 build-lock priority and narrow check scope).

---

## 3. Static Checks CI Wiring (`scripts/test-hardening-ci-wiring.sh`)

- **Cause & Evidence**:
  - In workflow run `41266573042` (`Static checks`), `scripts/test-hardening-ci-wiring.sh` failed at line 40 (`AssertionError` on `assert 'pdf_reader) build_options=(--dev --features pdf-reader)' in recipes`) because commit `c015077f` extended the `justfile` `fuzz-smoke` recipe arm to `pdf_reader|pdf_open_bounded) build_options=(--dev --features pdf-reader) ;;`.
- **Fix**:
  - Updated line 40 of `scripts/test-hardening-ci-wiring.sh` to assert `'pdf_reader|pdf_open_bounded) build_options=(--dev --features pdf-reader)' in recipes`.
- **Verification**:
  - `bash scripts/test-hardening-ci-wiring.sh` -> **PASS** (`CI wiring regression passed`).
- **CI-Only Scope**:
  - None; the exact static check script passed locally.

---

## 4. `windows-2025` Lint & Verify Warnings (`crates/ariad-host/tests/tool_runner_limits.rs`)

- **Cause & Evidence**:
  - In workflow run `41266573042` (`Lint (windows-2025)` and `Verify (windows-2025)`), `CARGO_BUILD_WARNINGS=deny` rejected `crates/ariad-host/tests/tool_runner_limits.rs` due to:
    1. `unused import: self` in `sandbox::{self, SandboxMode, SandboxOutcome}` at line 17 (`sandbox::support()` is only called in a `#[cfg(any(target_os = "linux", target_os = "macos"))]` test).
    2. `unused variable: metrics` at lines 215 and 245 because `metrics` was only read inside `#[cfg(unix)] assert_eq!(metrics.get("params_file_mode"), ...)`.
- **Fix**:
  - Removed `self` from the top-level `sandbox` import and referenced `ariad_host::sandbox::support()` directly inside the Linux/macOS-gated test.
  - Added `#[cfg(not(unix))] assert!(metrics.is_object(), "probe metrics must be a JSON object");` after both `#[cfg(unix)]` mode assertions so `metrics` is meaningfully asserted on Windows as well.
- **Verification**:
  - `CARGO_BUILD_WARNINGS=deny cargo clippy -p ariad-host -p ariad-cli --all-targets --features ariad-host/test-probe -- -D warnings` -> **PASS**.
- **CI-Only Scope**:
  - Native `x86_64-pc-windows-msvc` compilation on the `windows-2025` runner.

---

## 5. `macos-26` Linker Warning on `ashift` (`crates/ariad-cli/src/main.rs`)

- **Cause & Evidence**:
  - In workflow run `41266573042` (`Verify (macos-26)`), `just test` sets `CARGO_BUILD_WARNINGS=deny`. When linking the unstripped `ashift` binary on macOS 26, `ld` emitted:
    `warning: linker stderr: ld: __eh_frame section too large (max 16MB) to encode dwarf unwind offsets in compact unwind table, performance of exception handling might be affected`
    which triggered `#[warn(linker_messages)]` in `ariad-cli (bin "ashift")` and failed the build under `build.warnings = "deny"`.
- **Fix**:
  - Added `#![cfg_attr(target_os = "macos", allow(linker_messages))]` at the top of `crates/ariad-cli/src/main.rs` so the macOS linker's informational `__eh_frame` compact-unwind size notice on `ashift` does not fail `CARGO_BUILD_WARNINGS=deny`.
- **Verification**:
  - `CARGO_BUILD_WARNINGS=deny cargo clippy -p ariad-host -p ariad-cli --all-targets --features ariad-host/test-probe -- -D warnings` -> **PASS**.
- **CI-Only Scope**:
  - Apple `ld` linking of `ashift` on `macos-26`.

---

## 6. Docling Pack Resolve Verification (`crates/ariad-host/src/packs/extract.rs`, `crates/ariad-host/src/packs/install.rs`, `crates/ariad-host/tests/pack_resolve_verify.rs`)

- **Cause & Evidence**:
  - In workflow run `41266591997` (`Docling Engine Pack / verify (ubuntu-24.04)` and `verify (macos-15)`), `crates/ariad-cli/tests/docling_pack.rs:98` failed after `ashift engines install docling --from <signed>` when running `ashift convert --engine docling`, printing:
    `ashift: the docling engine is not installed; run ashift engines install docling or set ASHIFT_DOCLING_ENGINE for development`.
  - Root cause: `convert` resolves Docling via `ariad_host::packs::resolve("docling")`, which calls `try_resolve("docling").ok().flatten()`. Commit `5dfee1ea` added an executable allowlist check in `install::resolve_installed` (`scan.found_executables` vs `manifest_allowed_executables(target)`), where `manifest_allowed_executables` only allowed `target.entrypoint[0]` and `Unpack::File` components with `executable = true`.
  - For archive-based packs (`Unpack::TarZst` / `Unpack::TarGz` / `Unpack::Zip`) such as `docling`:
    1. `target.entrypoint[0]` (`runtime/bin/python3`) is a symlink to `runtime/bin/python3.14` (`0o755`), so `runtime/bin/python3.14` is scanned as a regular executable file that is not equal to `target.entrypoint[0]`.
    2. The hash-verified `runtime.tar.zst` archive also contains legitimate `0o755` binaries and shared libraries (`runtime/bin/*`, `runtime/lib/*.so`).
    3. `resolve_installed` therefore rejected `runtime/bin/python3.14` with `PackError::VerificationFailed` (`unlisted executable ...`), which `packs::resolve` turned into `None` (`ConvertError::DoclingMissing`).
    4. The same bug also broke `symlinks_and_hard_links_inside_the_root_are_kept` in `crates/ariad-host/tests/packs.rs:897`.
- **Fix**:
  - Recorded `executable: bool` on `extract::FileRecord` during component extraction (`crates/ariad-host/src/packs/extract.rs`).
  - Persisted the manifest-verified executable file list (`executables: Vec<String>`) in `ManifestRecord` (`<data>/packs/.locks/<name>-<version>.manifest`) when `install()` extracts SHA-256-verified components from the minisign-verified manifest (`crates/ariad-host/src/packs/install.rs`).
  - Updated `manifest_verified_entries` and `resolve_installed` to allow executables recorded in `ManifestRecord.executables` (while still enforcing `component.executable` on `Unpack::File` components and rejecting any unlisted executable added to the pack directory or `chmod +x`'d after installation, even if appended to the unsigned `receipt.toml`).
  - Added regression test `archive_pack_with_symlink_entrypoint_and_runtime_executables_resolves_cleanly` in `crates/ariad-host/tests/pack_resolve_verify.rs`.
- **Verification**:
  - `cargo test -p ariad-host --features ariad-host/test-probe --test pack_resolve_verify --test packs` -> **PASS**.
- **CI-Only Scope**:
  - Full `pack-docling.yml` workflow building the multi-hundred-MB Python/PyTorch/Docling wheel bundle and running `crates/ariad-cli/tests/docling_pack.rs` on `ubuntu-24.04` and `macos-15`.
