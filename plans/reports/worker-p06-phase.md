# Overlay and watermark implementation status

Status: incomplete. The phase is not ready for integration or release.

The approved phase starts from `1bdab0f511d3d3fbd2ee994241dbb03c042d4317`.
The branch contains the following preparatory changes:

- `3bc4fb91` adds an optional caller-supplied `ToolClock` to `ToolCtx` and tests
  that the value survives cloning and builder use. The host and WASM do not yet
  supply a clock; date resolution is not implemented.
- `f4acbe7a` adds shared anchor, font, date format, RGB colour and opacity
  parameter types with deserialization and schema checks.
- `23483e6a` copies regular and bold Libertinus 7.051 with its OFL notice and
  source receipts. The font hashes match the existing Typst font receipts.
  Nothing embeds or renders the fonts yet.

The drawing layer, date tokens, font subsetting, image builder, all 15 tools,
registration, output contract, schemas, integration, documentation and fuzzing
remain unimplemented. None of the visual, Unicode, platform, security or
independent review acceptance checks has been performed. In particular, no
watermark removal behavior should be inferred from these preparatory commits.

Checks completed:

- `build-lock.sh cargo test --locked -p ariad-pdf --lib clock_is_explicit_and_preserved_when_cloned`: PASS, one test.
- `build-lock.sh cargo test --locked -p ariad-core --lib overlay_common`: PASS, one test.
- `build-lock.sh cargo fmt --all --check`: PASS before the later common types;
  `cargo fmt --all` formatted those types.
- `committed --config committed.toml HEAD`: PASS after each implementation commit.
- SHA-256 of both font files matches `crates/ariad-typst/fonts/SOURCES.txt`.

The full gate has not been run and must not be treated as passed. Assumptions:
use the approved D1–D10 decisions and the marker contract from the slice map;
retain the existing single-worker branch and continue from its current head.
