# DOCX metadata fuzz crash

## Cause and fix

The issue 156 artifact contains an archive comment with embedded ZIP end-of-central-directory (EOCD) signatures. The stamper copied that comment into a rewritten archive. After the layout changed, `zip::ZipArchive` selected a decoy EOCD in the preserved comment: the original input had two entries and a 512-byte comment, but the stamped output was read with a zero-byte comment and a central directory that failed the independent raw-record walk. The pre-fix regression failed with `InvalidArchive("invalid central directory record")`.

`stamp_into` now rejects archive comments containing an EOCD signature before writing. This conservative rule applies to DOCX and EPUB; it also rejects comments whose signature would happen to be harmless. Native stamping uses a temporary file and preserves the original on error. Normal comments remain preserved. The fuzz assertion was not changed.

The original crash bytes are retained as `fuzz/seeds/docx_meta/embedded-eocd-comment.zip` (SHA-256 `6d96477ebb15e5e2cda97e162d9e2f00a49a1847e170a7187da4d79b6724e3b7`). The test `package_meta::tests::rejects_crash_seed_with_ambiguous_archive_comment` feeds those same bytes into the stamper. The existing test `package_meta::tests::stamps_a_memory_package_and_preserves_archive_comment_and_unrelated_data` covers ordinary comments.

## Validation

All Cargo commands ran through the shared build lock.

- PASS: focused regression test after the fix; the prior raw-directory assertion failed before the fix.
- PASS: `cargo fmt --all --check`.
- PASS: `cargo clippy -p ariad-core --all-targets -- -D warnings`.
- PASS: `cargo test -p ariad-core` (existing ignored tests remain ignored).
- PASS: `cd fuzz && cargo check --locked`.
- NOT RUN: `cargo +nightly-2026-10-05 fuzz run docx_meta <crash file>`; that nightly is not installed. CI must verify the ASan fuzz reproduction on its configured nightly. No other OS-specific behavior was changed or tested.
- Commit lint is run after the report is committed, as required by the task.

Assumptions: rejecting an ambiguous ZIP comment is preferable to modifying or discarding the comment, and is permitted by the fuzz target's success-or-error contract. The specified commit subject with the required `ariad-core` scope is 75 characters, conflicting with the common 72-character limit; the more specific task subject is used and the commit lint result is reported separately.

## Suggested PR

Title: Fix unreadable central directories after metadata stamping

Body: Fixes #156. Reject archive comments containing ambiguous ZIP end-record signatures before stamping, and retain the original fuzz crash as a regression seed. Verified with the `ariad-core` test suite, clippy, formatting, and a locked fuzz build; the nightly ASan reproduction remains for CI.
