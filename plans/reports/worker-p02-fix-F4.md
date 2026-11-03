# Worker Report — F4: Pack Resolve-Time Verification against Signed Manifest

## Summary

Resolved the resolve-time pack verification finding (`M1` in `code-reviewer-261101-0716-phase2-security.md`) so that `resolve_installed` and `verify` do not trust unsigned file tables in `receipt.toml`, reject unlisted executables in the pack directory, include `ctime` and directory file count in the verification cache stamp, and document the residual verify-then-exec window in `ARCHITECTURE.md` §11.1.

## Changes

- **`crates/ariad-host/src/packs/install.rs`**:
  - Bound installed file and symlink digests to the minisign-signed `pack.toml` manifest via `ManifestRecord` (`<data>/packs/.locks/<name>-<version>.manifest`), written atomically on activation and cleaned up on removal.
  - Enforced in `manifest_verified_entries` that `ManifestRecord` matches the SHA-256 of the minisign-verified `pack.toml` and target triple, that any `Unpack::File` component matches its signed `component.sha256` and `component.size` directly from `Manifest`, that the recorded unpacked byte total matches the signed `Manifest` `unpacked_size` sum, and that `receipt.toml`'s `[[files]]` and `[[links]]` tables match the manifest-bound record.
  - Added recursive pack directory scanning (`scan_pack_dir`) and `manifest_allowed_executables` in `resolve_installed` so any executable file on disk not declared as an executable in the signed manifest (`target.entrypoint[0]` or a `Component` with `unpack == Unpack::File && component.executable`) fails verification with `tool_missing`, and any extra unlisted file or symlink in the pack directory is rejected.
  - Extended `FileStamp` in `VERIFY_CACHE` with `ctime_sec`, `ctime_nsec` (on Unix), and `file_count` so replacing or rewriting a file and restoring its `mtime` (or changing the number of files in the pack directory) invalidates cached SHA-256 digests.
- **`crates/ariad-host/tests/pack_resolve_verify.rs`**:
  - Added `receipt_edited_to_match_tampered_file_is_refused` covering forged `receipt.toml` entries for both archive-unpacked and direct `Unpack::File` components.
  - Added `extra_unlisted_executable_in_pack_directory_is_refused` covering extra executable binaries in the pack directory (with and without a matching `receipt.toml` entry) and non-executable files marked `+x`.
  - Added `replacing_file_and_restoring_mtime_invalidates_verification_cache` verifying that in-place modification with restored `mtime` invalidates the verification cache via `ctime` and fails verification.
- **`ARCHITECTURE.md`**:
  - Added an entry under `§11.1 Threat Model -> Accepted residual risks` documenting why the narrow local resolve-to-`execve` window is accepted rather than copying multi-hundred-MB / multi-GB engine packs into per-job workspaces.

## Verification

- `cargo test -p ariad-host --test pack_resolve_verify --features ariad-host/test-probe`
- `cargo test -p ariad-host --test packs --features ariad-host/test-probe`
- `cargo clippy -p ariad-host --all-targets --features ariad-host/test-probe -- -D warnings`
- `cargo fmt --all --check`
