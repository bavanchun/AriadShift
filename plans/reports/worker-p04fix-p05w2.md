# Pack resolve path comparison

The qpdf adapter canonicalizes its install root while the install report retains the original temporary path. Windows can represent the same directory with an 8.3 short name or a canonical verbatim path; macOS can resolve `/var` to `/private/var`. The test now canonicalizes both sides of the install-root comparison and still requires the root to be present and equal.

The other path assertions in `pack_resolve_verify.rs` and shared test support were inspected. The Docling symlink entrypoint comparisons check the declared path returned by the non-adapter branch; canonicalizing them would discard that distinction. The PDFium comparison uses the same non-adapter branch. No changes were needed to those assertions. This assumes the paths remain accessible during the test, as required by pack resolution itself.

Validation on Linux through the shared build lock:

- `cargo fmt --all --check`: passed after correcting initial formatting.
- `cargo test --locked -p ariad-host --features test-probe --test pack_resolve_verify`: passed, 9 tests.
- `cargo clippy --locked -p ariad-host --tests -- -D warnings`: passed.

Windows and macOS were not available for a native run; platform behavior is based on the CI failure and path canonicalization semantics.
