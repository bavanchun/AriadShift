# Windows pack verification cache: blocked

Status: incomplete. The requested implementation conflicts with the permitted
file scope and the host crate's unsafe-code prohibition.

## Evidence and assumptions

Issue #157 reports repeated Windows hashing. `FileStamp::from_metadata` in
`crates/ariad-host/src/packs/install.rs` returns `None` on every non-Unix
platform, and `verified_sha256` only reads or writes the cache when a stamp is
available. The existing `resolve_pack_verifies_signature_and_reuses_only_safe_cached_digests`
test explicitly expects repeated hashing on non-Unix; the existing
`replacing_file_and_restoring_mtime_invalidates_verification_cache` test checks
in-place tampering with restored mtime, while
`tampered_executable_or_payload_is_rejected_on_resolve_with_tool_missing_and_reinstall_hint`
checks replacement.

The requested `windows-sys` call is unsafe. `crates/ariad-host/src/lib.rs`
uses `#![forbid(unsafe_code)]`, and the workspace also denies unsafe code.
The assigned scope permits host `src/packs`, tests, and its Cargo.toml, but not
`src/lib.rs` or workspace lint configuration. A child module cannot override
`forbid`. The existing safe `same-file` and `cap-std` identity APIs do not expose
NTFS ChangeTime. Assuming the stated scope and prohibition must be respected,
no compliant implementation using the specified API is possible. Changing the
crate-level policy or providing an approved safe wrapper in another crate would
require a scope and architecture decision.

`windows-sys` 0.61.2 is already a workspace dependency and appears in the locked
target-all dependency graph. No dependency or code was changed. The Windows
failure could not be reproduced locally because this machine cannot build the
Windows target, as specified in the assignment.

## Checks

- `cargo tree -i windows-sys`: FAIL; the package name is ambiguous across four
  locked versions. This initial read-only command was inadvertently run without
  the mandatory build lock.
- `build-lock.sh cargo tree --locked --target all -i windows-sys@0.61.2`: PASS.
- Formatting, clippy, `pack_resolve_verify`, and `cargo deny`: not run; there is
  no implementation or dependency change to validate.
- Windows CI lint and tests remain necessary after a compliant implementation.

## Suggested PR text

Title: Cache Windows pack verification using NTFS change time

Body: Fixes #157. Reuse verified pack digests on Windows using NTFS change time
and file identity, while rehashing when metadata cannot be read. Verify unchanged
files use the cache and tampered files with restored modification times fail.
This PR must not be opened until the implementation and checks are complete.
