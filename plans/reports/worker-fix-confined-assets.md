# Worker Fix Report: Confined Asset Base & Windows Hidden-Name Checks (`confined-assets`)

## Summary & Assumptions

- **Branch:** `fix/confined-asset-base` (worktree `/home/vchun/Codes/01-AriadShift-wt-i6`)
- **Assumptions & Decisions:**
  1. `ariad-host` is `0.x`, so updating `convert_confined` and `convert_confined_in` to require `asset_base: &AssetBase` and making `Confinement.asset_base` non-optional (`&'a AssetBase`) is an accepted breaking API change that structurally prevents any confined conversion from falling back to `request.asset_base_dir` or `request.input.parent()`.
  2. `convert_confined_assets_in` is retained as a thin wrapper around `convert_confined_in` with the identical signature so `crates/ariad-cli/src/mcp/tools.rs` (marked read-only in the task brief) continues to compile and operate without modification.
  3. `docs/release-notes.md` currently has no unreleased section (only `## 0.2.0` and `## 0.1.0`), so per the task directive ("Add a docs/release-notes.md entry for the API change if that file has an unreleased section") it was left unchanged.
  4. On Windows, post-open reparse-point rejection (`open_entry_nofollow` with `FILE_FLAG_OPEN_REPARSE_POINT`) and directory handle identity (`same_directory` via `same-file`) were already present on `origin/dev`; the remaining gap in confined asset resolution was Windows `FILE_ATTRIBUTE_HIDDEN` (`0x2`) and 8.3 short-name aliases (`~` followed by ASCII digits before the extension).

---

## Issue #71: Require an `AssetBase` capability handle in `convert_confined`

### Cause & Evidence
- `convert_confined` (`crates/ariad-host/src/convert.rs:540-565`) and `convert_confined_in` (`crates/ariad-host/src/convert.rs:571-597`) constructed `Confinement` with `asset_base: None`.
- In `RouteRun::native_step` (`crates/ariad-host/src/convert.rs:955-991`), when `asset_base` was `None`, Markdown and HTML conversions fell back to `assets::resolve(&mut document, base_dir, self.limits)` using `request.asset_base_dir` or `request.input.parent()`, allowing relative image references to resolve outside a confined directory handle.

### Fix
- Made `Confinement.asset_base` non-optional (`&'a AssetBase`) in `crates/ariad-host/src/convert.rs`.
- Updated `convert_confined` and `convert_confined_in` to require `asset_base: &AssetBase`, and made `convert_confined_assets_in` delegate directly to `convert_confined_in`.
- Simplified `RouteRun::native_step` so any conversion carrying `self.asset_base` unconditionally resolves images via `assets::resolve_in`.
- **Workspace call sites updated:**
  - `crates/ariad-host/src/convert.rs`: `convert_confined`, `convert_confined_in`, `convert_confined_assets_in`, `Confinement`, `RouteRun::native_step`, `convert_for_route`.
  - `crates/ariad-host/tests/confine.rs`: `test_convert_confined_parent_swap_race` and `confined_conversions_resolve_assets_only_within_explicit_asset_base` (replacing the deliberate old-signature test `pooled_confined_conversion_keeps_the_existing_call_signature`).
  - `crates/ariad-cli/src/mcp/tools.rs`: calls `convert_confined_assets_in` (unchanged, read-only).

### Tests
- `confined_conversions_resolve_assets_only_within_explicit_asset_base` (`crates/ariad-host/tests/confine.rs`) — verifies for both `convert_confined` and `convert_confined_in` that image references outside the given `AssetBase` (both via ambient `request.asset_base_dir` / `request.input.parent()` and via `../` traversal) fail to embed and emit `image_not_embedded` warnings, while an image within the given `AssetBase` is embedded cleanly.
- `test_convert_confined_parent_swap_race` (`crates/ariad-host/tests/confine.rs`) — updated to pass `&asset_base`.

---

## Issue #70: Windows hidden-attribute and 8.3 short-name checks in confined asset resolution

### Cause & Evidence
- `has_hidden_component` (`crates/ariad-host/src/assets.rs:328-332`) only inspected path components for a leading `.`.
- On Windows, confined asset resolution (`read_image_nofollow`, `crates/ariad-host/src/assets.rs:395-420`) did not check `FILE_ATTRIBUTE_HIDDEN` (`0x2`) on opened directory/file metadata and did not reject 8.3 short-name aliases (such as `PRIVAT~1` for `.private` or `PHOTO~1.PNG` for `.photo.png`) when they did not match the entry's long name.

### Fix
- Added pure, cross-platform decision helpers in `crates/ariad-host/src/assets.rs`:
  - `looks_like_windows_short_name(name: &str) -> bool` and `has_short_alias_tilde(stem: &str) -> bool`: detect components with a non-empty prefix, `~`, and ASCII digits before the extension.
  - `is_windows_hidden_component(requested_name: &str, long_name: Option<&str>, file_attributes: u32) -> bool`: rejects dot-prefixed requested or long names, entries carrying `WINDOWS_FILE_ATTRIBUTE_HIDDEN` (`0x2`), and components that look like an 8.3 short alias unless `long_name` matches `requested_name` case-insensitively.
- Added `#[cfg(windows)]` enforcement in `read_image_nofollow` and `open_confined_asset_dir` (`crates/ariad-host/src/assets.rs`) that inspects `file_attributes()` on every directory and file component and resolves matching long names from the open capability directory handle (`Dir::entries()`) when a component looks like an 8.3 short alias, mapping rejections to `ReadNofollowError::Hidden` (`"href contains a hidden path component"`).

### Tests
- `assets::windows_hidden_tests::windows_short_name_detection_identifies_tilde_digit_aliases` (`crates/ariad-host/src/assets.rs`)
- `assets::windows_hidden_tests::windows_hidden_component_rejects_hidden_attribute_and_unmatched_short_aliases` (`crates/ariad-host/src/assets.rs`)
- `assets::windows_hidden_tests::windows_confined_read_rejects_hidden_attribute_and_allows_matching_tilde_name` (`crates/ariad-host/src/assets.rs`, `#[cfg(windows)]`)

---

## Security Review (`/ak-security`)

- **Trust boundary:** Confined conversions and sidecar image resolution under MCP (`ashift mcp`) and host callers using capability directory handles (`cap_std::fs::Dir` / `AssetBase`).
- **STRIDE / OWASP A01 (Broken Access Control):**
  - Making `Confinement.asset_base` non-optional eliminates ambient path fallback (`request.asset_base_dir` / `request.input.parent()`) across all confined conversion entry points.
  - Checking `FILE_ATTRIBUTE_HIDDEN` and 8.3 short-name aliases against the open `cap_std::fs::Dir` entries on every path component prevents bypassing hidden-file restrictions via DOS 8.3 names or Windows hidden attributes without introducing ambient path lookups.
  - Keeping `ReadImageError` unchanged and introducing a private `ReadNofollowError` preserves existing error contracts in `crates/ariad-host/src/media.rs`.

---

## Verification Commands & Results

- `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh cargo fmt --all --check` — **PASS**
- `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh cargo clippy -p ariad-host -p ariad-cli --all-targets --features ariad-host/test-probe -- -D warnings` — **PASS**
- `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh cargo test -p ariad-host --features test-probe --test confine` — **PASS** (31 passed)
- `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh cargo test -p ariad-host --lib assets` — **PASS** (3 passed)
- `sh scripts/check-commits.sh origin/dev..HEAD` — fails only because `.tools/bin/committed` is missing in this worktree; `/home/vchun/Codes/01-AriadShift/.tools/bin/committed --config committed.toml origin/dev..HEAD` — **PASS**
- **What only CI or another OS can verify:** Cross-compiling `ariad-host` to `x86_64-pc-windows-gnu` on this Linux host stops in `psm` (`serde_stacker`) because `x86_64-w64-mingw32-gcc` is not installed locally; Windows CI verifies the `#[cfg(windows)]` metadata and directory-entry code paths and runs `windows_confined_read_rejects_hidden_attribute_and_allows_matching_tilde_name`.

---

## Suggested PR Text

**Title:** `fix(host): require AssetBase in confined conversions and harden Windows hidden asset checks`

**Body:**
```markdown
## Summary
- Require an explicit `&AssetBase` capability handle in `convert_confined` and `convert_confined_in`, and make `Confinement.asset_base` non-optional so confined conversions never fall back to `request.asset_base_dir` or the input path.
- Reject Windows confined asset path components that carry `FILE_ATTRIBUTE_HIDDEN` or look like 8.3 short-name aliases (`~` followed by digits before the extension) unless they match the entry's long name.
- Add unit and integration tests covering confined conversions with inside/outside asset references and the pure Windows hidden-component decision helpers.

Closes #70
Closes #71
```
