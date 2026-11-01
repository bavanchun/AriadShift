# Nightly fuzz matrix

The workflow listed only nine of the 22 binaries declared in `fuzz/Cargo.toml`, so the remaining targets could not run nightly. The manifest requires `pdf-reader` for both bounded PDF targets and `typst-writer` for the Typst target. The local `just fuzz` recipe also supplies those features for `pdf_open_bounded` and `typst_writer`; it does not yet include `pdf_tools_structural` in its feature case.

Added the missing targets: `front_matter`, `limits_validate`, `typst_writer`, `pack_manifest`, `engine_protocol`, `capabilities_json`, `asset_resolve`, `uri_parsers`, `ir_json_versioned`, `page_range`, `recipe_json`, `pdf_open_bounded`, and `pdf_tools_structural`. The workflow now passes the required feature for each PDF and Typst target. Assumption: the manifest is authoritative for the structural target's feature because the recipe predates that target. No issue number was supplied in the task.

## Verification

- PASS: manifest-to-matrix comparison: all 22 targets appear exactly once, with required features; every declared source exists. The first temporary comparison script failed due to its own whitespace handling; the corrected comparison passed.
- PASS: `/home/vchun/Codes/01-AriadShift/.tools/bin/actionlint .github/workflows/fuzz-nightly.yml`.
- PASS: `bash scripts/test-hardening-ci-wiring.sh` (CI wiring regression).
- PASS: `bash scripts/test-ci-scope.sh` (82 checks).
- PASS: `git diff --check`.
- SKIPPED: `cd fuzz && /home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh cargo check --locked` waited for a lock held by another worktree for over an hour. The waiting command was stopped before cargo started. The manager directed proceeding without this local check; the CI fuzz job builds the targets.

The scheduled run and actual ASan fuzz execution require GitHub Actions after integration into `dev`; local checks cannot establish the first nightly result.

## Suggested PR

Title: `ci(fuzz): run all fuzz targets nightly`

The nightly matrix omitted 13 declared fuzz targets, including the structural PDF target needed by the phase gate. Add every missing target and supply the PDF and Typst features required by the fuzz manifest. No issue number was supplied.

Validation: actionlint, CI wiring and scope checks, and manifest-to-matrix comparison passed. The local locked fuzz crate check was skipped because of the shared build lock; CI will build the targets.
