# Cargo dependency update

Status: incomplete. The required license notice check fails because
`THIRD_PARTY_LICENSES` lists the old versions. The task permits changes only
to Cargo manifests, lockfiles, required call sites, and this report, so the
notice was not regenerated. The branch requires that file to be updated and
`just licenses-check` rerun before it is ready to ship.

## Changes and assumptions

- #171 and #174: comrak 0.56.0 to 0.57.0. Wikilink embeds and OFM comments
  are new upstream options; existing parser options remain unchanged. Existing
  Markdown reader tests pass and no call-site API change was needed.
- #169 and #173: jsonschema 0.59.0 to 0.60.2. The `Enum` and `Constant`
  validation error values now use `Arc<Value>`. The repository does not match
  those variants directly, so no call-site adaptation was necessary. Upstream
  includes numeric validation and recursive-reference fixes; schema and host
  tests pass. Associated jsonschema crates were updated in both lockfiles.
- #170 and #175: saphyr-parser 0.2.2 to 0.3.3, including the fuzz manifest.
  The parser changes empty terminal block scalar results and adds alias
  protection and flow-mapping fixes. The front matter collector uses event
  spans to preserve the previous empty terminal scalar result, while keeping
  existing anchor and alias rejection. The regression test is
  `reader::front_matter::tests::preserves_empty_terminal_block_scalar_values`.

Each requested version was verified as present and not yanked using the
crates.io API. The assumption is that preserving the old empty-scalar result
is required even where the upstream parser corrected its interpretation.
No zip, rmcp, tokio, or uuid version was changed.

## Validation

All Cargo and just commands ran individually through the shared build lock.

- PASS: `cargo fmt --all --check`.
- PASS: `cargo clippy --locked -p ariad-core -p ariad-host -p ariad-cli --all-targets --features ariad-host/test-probe -- -D warnings`.
- PASS: `cargo test --locked -p ariad-core`.
- PASS: `cargo test --locked -p ariad-host --features test-probe`.
- PASS: `cargo test --locked -p ariad-cli --features ariad-host/test-probe`.
- PASS: `cargo check --locked` in `fuzz/`.
- PASS: `env UV_NO_CONFIG=1 just deny` (duplicate dependency warnings).
- FAIL: `env UV_NO_CONFIG=1 just licenses-check`: `THIRD_PARTY_LICENSES`
  is stale for comrak, jsonschema, referencing and saphyr-parser; the check
  stopped before the desktop and web notice checks.
- PASS: `env UV_NO_CONFIG=1 just pandoc` installed the pinned Pandoc.
  Initial host test runs failed seven tests with missing Pandoc, including
  `doctor_describe_failure_produces_failed_report`; direct Cargo commands need
  `ASHIFT_PANDOC` set, which `just` normally exports. Host and CLI passed
  after setting it to the installed binary.
- The worktree lacks `.tools/bin/committed`; commit lint used the binary from
  the main checkout with this worktree's `committed.toml` instead.

Tests ran on Linux. CI and macOS/Windows behavior have not been verified.

## Suggested PR

Title: `build(deps): update Cargo parser and schema dependencies`

Body: Update comrak, jsonschema and saphyr-parser with both root and fuzz
lockfiles synchronized, preserving the previous empty front matter scalar
behavior. Supersedes #169, #170, #171, #173, #174 and #175. Regenerate the
third-party notice and rerun the license check before merging.
