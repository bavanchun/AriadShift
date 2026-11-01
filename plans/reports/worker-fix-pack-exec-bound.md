# Worker Report: Bound Recorded Pack Executables by Signed Data on Every Target (`pack-exec-bound`)

## Issue #137 — Bound recorded pack executables by signed component flags on every target

### Cause & Evidence
- In `crates/ariad-host/src/packs/install.rs`:
  1. `manifest_verified_entries` (lines 1011–1050 before this fix) accepted any path listed in the local unsigned `.locks/<name>-<version>.manifest` (`ManifestRecord.executables`) as long as that path appeared in `record.files`, without checking whether the file was verified as executable at install time (`ReceiptFile.executable`) or declared in `manifest.notices`.
  2. Pruning of non-executable `Unpack::File` components (`if !component.executable && !declared_executables.contains(&rel) { allowed_executables.remove(&rel); }`) was gated behind `#[cfg(unix)]`, and `install()` used `is_executable_file` (which matches `.exe`/`.bat`/`.cmd` on Windows) even for `Unpack::File` components whose signed manifest declared `executable = false`.
  3. Reproducer evidence: running `tampered_manifest_record_executables_cannot_widen_allowed_set` before the fix failed at `crates/ariad-host/tests/pack_resolve_verify.rs:576` because adding `"models/weights.bin"` to `.locks/qpdf-1.0.0.manifest`'s `executables` list and marking `models/weights.bin` executable on disk allowed `resolve_pack_in(&ctx, "qpdf")` to succeed instead of failing verification.

### Fix
- **`crates/ariad-host/src/packs/install.rs`**:
  - Added `#[serde(default, skip_serializing_if = "std::ops::Not::not")] executable: bool` to `ReceiptFile` so both `receipt.toml` and `.locks/<name>-<version>.manifest` bind each installed file's executable status from install-time verified extraction.
  - Bounded install-time `file.executable` in `install()` so `Unpack::File` components only count as executable when `component.executable` is true in the signed manifest or the path is in `manifest_allowed_executables(&target)` on every target, and archive members never mark `manifest.notices` paths executable unless declared.
  - Updated `manifest_verified_entries` to:
    - Only accept recorded archive executables when `target.components` contains an archive component, the path is marked `executable = true` in both `record.files` and `receipt.files`, and the path is not a notice in `manifest.notices`.
    - Enforce `component.executable` and `declared_executables` for `Unpack::File` components on every target (dropping the `#[cfg(unix)]` gate) and reject any `Unpack::File` record entry whose `executable` flag conflicts with the signed component.
  - `extract.rs` did not need changes because `extract::FileRecord` already records `executable: bool`.
- **`crates/ariad-host/tests/pack_resolve_verify.rs`**:
  - Added `tampered_manifest_record_executables_cannot_widen_allowed_set` testing that:
    - Extra paths added to `.locks/<name>-<version>.manifest`'s `executables` list are ignored when no unlisted executable is on disk.
    - Dropping an executable at a path added to `executables` in `.locks/<name>-<version>.manifest` is refused with `tool_missing` (`unlisted executable`).
    - Marking a non-executable archive member (`models/weights.bin`) executable after adding it to `.locks/<name>-<version>.manifest`'s `executables` is refused.
    - A non-executable `Unpack::File` component (`bin/helper.exe` with `executable = false` in the signed manifest) added to `.locks/<name>-<version>.manifest`'s `executables` is refused on every target.

### Security Review (`/ak-security`)
- **Trust Boundary & Tampering (STRIDE — Tampering / Elevation of Privilege)**:
  - Verified that editing `.locks/<name>-<version>.manifest`'s `executables` array cannot widen `allowed_executables` on any target, whether for unlisted files, non-executable `Unpack::File` components, `manifest.notices` files, or non-executable archive members.
  - Verified that editing `[[files]]` in `.locks/<name>-<version>.manifest` to flip `executable = true` is caught by `Unpack::File` component verification and `check_receipt_matches_manifest_record` against `receipt.toml`.
  - No secrets, credentials, or external dependencies added or modified.

### Test Names
- `tampered_manifest_record_executables_cannot_widen_allowed_set` (`crates/ariad-host/tests/pack_resolve_verify.rs`)
- `archive_pack_with_symlink_entrypoint_and_runtime_executables_resolves_cleanly` (`crates/ariad-host/tests/pack_resolve_verify.rs`)

### Verification Commands & Results
- `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh cargo fmt --all --check` -> **PASS**
- `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh cargo clippy -p ariad-host --all-targets --features test-probe -- -D warnings` -> **PASS**
- `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh cargo test -p ariad-host --features test-probe --test pack_resolve_verify` -> **PASS** (`9 passed`)
- `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh cargo test -p ariad-host --lib packs` -> **PASS** (`20 passed`)
- `/home/vchun/Codes/01-AriadShift/.tools/bin/committed --config committed.toml origin/dev..HEAD` -> **PASS** (`sh scripts/check-commits.sh origin/dev..HEAD` failed only because `.tools/bin/committed` is absent in this worktree)

### CI / Other OS Verification Scope
- Native Windows (`windows-2025`) and macOS (`macos-15` / `macos-26`) execution of `tampered_manifest_record_executables_cannot_widen_allowed_set` (where `.exe` extension detection in `is_executable_file` runs natively on Windows without `#[cfg(unix)]` mode bit setup), plus full end-to-end `pack-docling.yml` verification in CI.

### Assumptions Recorded
- Extra entries in `.locks/<name>-<version>.manifest`'s `executables` list that are not backed by signed component declarations or install-verified archive executable records are ignored when building `allowed_executables`, so resolving a clean pack directory ignores the extraneous strings while any corresponding executable file on disk is refused with `tool_missing` (`unlisted executable`).

### Suggested PR Text
- **Title**: `fix(host): bound recorded pack executables by signed data on all targets`
- **Body**:
  ```markdown
  Fixes #137.

  - Drop the `#[cfg(unix)]` gate in `manifest_verified_entries` so `Unpack::File` components are bounded by the signed `component.executable` flag and declared entrypoint on every target.
  - Bind install-time verified `executable` flags in `ReceiptFile` across both `receipt.toml` and `.locks/<name>-<version>.manifest`, and only allow recorded archive executables when backed by an archive component and matching verified file records (excluding `manifest.notices`).
  - Add `tampered_manifest_record_executables_cannot_widen_allowed_set` in `crates/ariad-host/tests/pack_resolve_verify.rs` while keeping Docling archive resolution green.
  ```
