# Worker 7 t4b: local MCP sidecar image confinement

Branch: `fix/v1-security-audit--mcp-assets` (based on `origin/dev` at `3b85281`).

## Outcome

MCP `convert` no longer reopens the input's parent by path when it embeds images. Confinement keeps the directory handle it opened the input file from. Conversion resolves Markdown and HTML image references beneath that handle, under the same hidden-component rule as the main input. Ordinary CLI, batch, desktop and server conversion keep the existing path-based resolution.

No build or test was run, as the brief requires. `rustfmt --edition 2024` was run on the touched files only.

## Commits

- `7836169 fix(mcp): resolve sidecar images through the confined input handle`
- The commit that adds this report.

## Files changed

- `crates/ariad-host/src/assets.rs`
  - New public `AssetBase` type: an open `cap_std::fs::Dir` plus a private policy. The policy is `Standard` or `Confined`, and `AssetBase::confined(dir)` builds a confined base.
  - New `resolve_in(document, &AssetBase, limits)`. The existing `resolve(document, path, limits)` now opens the path and delegates with the `Standard` policy, so its behaviour is unchanged.
  - In confined mode, any href segment starting with `.` produces the warning "href contains a hidden path component". Reads go through a new `read_image_nofollow`, which:
    - opens each directory component without following symlinks;
    - opens the file with `O_NOFOLLOW | O_NONBLOCK` on Unix;
    - after opening, rechecks the entry is not a symlink and has the same device and inode as the handle.
  - `read_image` now shares its bounded read and image sniffing with the confined path through `read_open_image`. The `ReadImageError` variants are unchanged, so `media.rs` needs no edit.
- `crates/ariad-host/src/confine.rs`
  - `ConfinedInput` has a new public field `parent_dir: cap_std::fs::Dir`. It is the handle the input file was opened from.
  - `open_file_nofollow` now wraps a new `open_file_and_parent_nofollow`. That function opens the parent through a new `pub(crate) open_dir_nofollow`, which walks one component at a time. After each `open_dir` it rechecks the entry with `symlink_metadata` and compares device and inode. Before this change, cap-std `open_dir` followed symlinks that stayed inside the root.
  - New `pub(crate) same_inode` helper. Off Unix it always returns true, the same best-effort level as the existing checks.
- `crates/ariad-host/src/convert.rs`
  - A private `Confinement { destination_parent, file_name, asset_base }` struct replaces the `Option<(&Dir, &Path)>` parameter of `convert_executor` and `convert_for_route`. `RouteRun` carries `asset_base`.
  - **Public signature change:** `convert_confined_in` takes a new `asset_base: &AssetBase` argument, placed after `file_name`. Its only caller is MCP `tools.rs`. `convert_confined` passes `None` and is unchanged.
  - The native Markdown/HTML → IR step uses `resolve_in` with the handle whenever the source stage is confined. It never falls back to `asset_base_dir` or the input path in that case. Engine workspace artifacts still resolve from their workspace path.
- `crates/ariad-cli/src/mcp/tools.rs`: `convert` builds `AssetBase::confined(validated_input.parent_dir.try_clone())`, passes it to `convert_confined_in`, and sets `asset_base_dir: None`. The old `original_parent` path is removed.
- Tests:
  - `crates/ariad-host/tests/confine.rs`
    - `test_parent_swap_after_input_copy_never_embeds_a_private_image` runs `confine_input`, then the isolated copy, then renames `sub` and replaces it with a symlink to a private directory, then calls `convert_confined_in` to IR JSON. It asserts the private PNG's SHA-256 id is absent and the authorized sibling's id is present. The swap is deterministic.
    - `test_confined_input_parent_dir_is_the_input_directory`.
  - `crates/ariad-host/tests/assets.rs`. In confined mode:
    - a visible sibling and a nested visible image embed;
    - `.private/photo.png`, `.photo.png` and `visible/.photo.png` are refused with a "hidden" warning;
    - symlinks that stay inside the base are refused, for both a directory component and the final file (Unix);
    - a base whose path is replaced after it was opened still reads the original directory (Unix);
    - a FIFO is refused without blocking (Unix).
    - A further test, `ordinary_resolution_still_embeds_hidden_path_images`, pins the unchanged CLI policy.
  - `crates/ariad-cli/tests/mcp_stdio.rs`: in `test_convert_refuses_hidden_sidecar_images_and_embeds_visible_ones`, end-to-end MCP `convert` to `json` embeds `photo.png`. The hidden-directory and hidden-file references give two `image_not_embedded` warnings, and their bytes are absent from the output.

The existing main-input and output-directory swap tests are kept unchanged.

## Verification for the coordinator

```sh
cargo test -p ariad-host --test assets
cargo test -p ariad-host --test confine
cargo test -p ariad-host --test workspace
cargo test -p ariad-host --lib convert
cargo test -p ariad-cli --test mcp_stdio
cargo clippy --workspace --all-targets -- -D warnings
just ci
```

## Behaviour changes and concerns

- **MCP behaviour narrowing (intended):** MCP no longer embeds images reached through any symlinked path component, even one that stays inside the input directory. The main input already refuses a symlinked final component. Image references that previously embedded through such links now give `image_not_embedded`.
- **Shared parent walk:** `open_file_nofollow` also serves `Workspace::open_artifact` (`workspace.rs:139`), so a symlinked directory inside an engine workspace's output directory is now refused too. Engines are not expected to create such links. `cargo test -p ariad-host --test workspace` and the engine route tests should confirm this.
- **Pre-existing gap, unchanged:** `assets::resolve` (both policies) walks only `document.body`, not `document.furniture`. The native Markdown and HTML readers do not produce furniture, so this fix has no exposure there.
- **Docs follow-up (not owned):** `docs/mcp.md`, "Hidden Components" item near line 87, could add: "Images referenced by a converted Markdown or HTML document resolve beneath the input's directory as opened during confinement; references with a hidden or symlinked path component are not embedded and produce an `image_not_embedded` warning." The audit report's SEC-02 status can move to Fixed once the coordinator verifies.
- **Windows:** the device and inode comparison is Unix-only, matching the existing `open_file_nofollow` checks. On Windows the handle still pins the authorized directory, and the symlink-metadata rechecks remain.

Status: DONE_WITH_CONCERNS
