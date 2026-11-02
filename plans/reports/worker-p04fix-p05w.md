# Windows pack program path verification

The qpdf adapter receives the signed manifest entrypoint joined to the absolute
pack root. Resolution verifies its canonical path remains inside the canonical
pack root, and the adapter separately checks that the canonical path matches the
manifest entrypoint. The test compared this non-canonical environment value to a
canonical expected path. On Windows, canonicalization may add the verbatim path
prefix, so equivalent paths can compare unequal.

The test now requires the variable to exist and be absolute, canonicalizes it,
checks containment in the pack root, and compares it with the canonical signed
entrypoint. Product checks are unchanged. The fixture's `bin/engine` is the
signed entrypoint; the assumption is that Windows CI can access that file via
both representations.

Validation (all cargo commands used the shared build lock):

- `cargo test --locked -p ariad-host --features test-probe --test pack_resolve_verify`: passed, 9 tests, including `resolve_pack_verifies_signature_and_reuses_only_safe_cached_digests`.
- `cargo fmt --all --check`: passed.
- `cargo clippy --locked -p ariad-host --tests -- -D warnings`: passed on Linux.
- The same clippy command with `--target x86_64-pc-windows-gnu` failed while building `ring`: the Windows GNU C compiler `x86_64-w64-mingw32-gcc` is not installed. Windows execution remains for CI.
